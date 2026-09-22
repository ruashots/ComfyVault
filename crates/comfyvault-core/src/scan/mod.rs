//! Walking the installs and identifying every weight file.
//!
//! # What a scan reads
//!
//! For each install: the `models` tree, every folder named in
//! `extra_model_paths.yaml`, and the five folders under `output/` that ComfyUI
//! registers as model search paths at startup. It also walks `custom_nodes` and
//! the Hugging Face cache, but only to count them. Those files are never moved:
//! a custom node reaches its bundled weights by a path relative to its own
//! folder, so moving one breaks the node.
//!
//! # Following links, and counting a file once
//!
//! The walk follows symbolic links, because ComfyUI does, and the two have to
//! agree about which files exist. Following links means one physical file can
//! be reached by two routes. The scan therefore remembers the real location of
//! everything it has taken and skips a repeat.
//!
//! That is not a tidiness measure. Without it, a plan could hold two entries
//! that are one file, and applying it would move the file and then try to link
//! it to itself.

pub mod hash;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultError};
use crate::install::{detect::RootOrigin, Install};
use crate::platform::Platform;
use crate::progress::{estimate_remaining_ms, CancelToken, ProgressSink, Throttle};
use crate::settings::Settings;
use crate::store::{
    Classification, HashCacheRecord, InstallScanTotals, ScanEntryRecord, ScanError, ScanRecord,
    ScanTotals, Store,
};
use crate::time_util::Timestamp;

/// How many files are hashed at once.
///
/// Reading is the limit, not the hashing. One thread saturates a SATA solid
/// state drive and several are needed for a fast NVMe drive, while too many
/// would make a spinning disk seek itself to a standstill. Eight is the
/// compromise.
const MAX_HASH_THREADS: usize = 8;

/// How deep the walk goes before it gives up.
///
/// A model tree is a few levels deep. A limit this high is only a guard against
/// a chain of directory links the walker's own loop detection does not catch.
const MAX_WALK_DEPTH: usize = 24;

/// Which part of the scan is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Enumerating,
    Hashing,
    Finalizing,
}

/// A progress update, as the contract defines it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: String,
    pub phase: ScanPhase,
    pub install_id: Option<String>,
    pub install_label: Option<String>,
    pub files_seen: u64,
    pub files_to_hash: u64,
    pub files_hashed: u64,
    pub bytes_to_hash: u64,
    pub bytes_hashed: u64,
    pub bytes_from_cache: u64,
    pub current_path: Option<String>,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
}

/// Everything one scan produced.
pub struct ScanOutcome {
    pub record: ScanRecord,
    pub entries: Vec<ScanEntryRecord>,
}

/// A file the walk found, before it is hashed.
#[derive(Debug, Clone)]
struct Candidate {
    abs_path: PathBuf,
    rel_path: PathBuf,
    install_id: String,
    category: String,
    size_bytes: u64,
    mtime_nanos: i128,
    classification: Classification,
    link_target: Option<PathBuf>,
    sha256: Option<String>,
}

/// The folders the Hugging Face libraries download into.
///
/// These hold real model weights and can run to hundreds of gigabytes, so the
/// person deserves to see the number. They are never moved: the libraries
/// address files by a layout they own, and rearranging it breaks them.
pub fn huggingface_cache_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() && !out.contains(&p) {
            out.push(p);
        }
    };

    if let Ok(v) = std::env::var("HUGGINGFACE_HUB_CACHE") {
        push(PathBuf::from(v));
    }
    if let Ok(v) = std::env::var("HF_HOME") {
        push(PathBuf::from(v).join("hub"));
    }
    if let Some(home) = std::env::var("USERPROFILE").ok().or_else(|| std::env::var("HOME").ok()) {
        push(PathBuf::from(home).join(".cache").join("huggingface").join("hub"));
    }
    out
}

/// Walks the installs and identifies every weight file.
pub struct Scanner<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
    settings: Settings,
}

impl<'a> Scanner<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform, settings: Settings) -> Self {
        Self { store, platform, settings }
    }

    /// Runs a whole scan. Changes nothing on disk.
    pub fn scan(
        &self,
        scan_id: &str,
        installs: &[Install],
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ScanProgress>,
    ) -> Result<ScanOutcome> {
        let started_at = Timestamp::now();
        let clock = Instant::now();
        let throttle = Throttle::per_second(4);
        let errors: Mutex<Vec<ScanError>> = Mutex::new(Vec::new());

        // --- enumerate ---------------------------------------------------
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut seen_real: HashSet<PathBuf> = HashSet::new();
        let mut cancelled = false;

        for install in installs {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            self.enumerate_install(
                install,
                &mut candidates,
                &mut seen_real,
                &errors,
                cancel,
                |seen, current| {
                    if throttle.ready() {
                        sink.emit(&ScanProgress {
                            scan_id: scan_id.to_string(),
                            phase: ScanPhase::Enumerating,
                            install_id: Some(install.id.clone()),
                            install_label: Some(install.label.clone()),
                            files_seen: seen,
                            files_to_hash: 0,
                            files_hashed: 0,
                            bytes_to_hash: 0,
                            bytes_hashed: 0,
                            bytes_from_cache: 0,
                            current_path: current,
                            elapsed_ms: clock.elapsed().as_millis() as u64,
                            eta_ms: None,
                        });
                    }
                },
            );
        }

        // The Hugging Face cache belongs to the computer, not to one install,
        // so it is walked once after the installs.
        if !cancelled && !cancel.is_cancelled() {
            for dir in huggingface_cache_dirs() {
                self.enumerate_dir(
                    &dir,
                    &dir,
                    "",
                    None,
                    RootOrigin::HuggingFaceCache,
                    &[],
                    &mut candidates,
                    &mut seen_real,
                    &errors,
                    cancel,
                );
            }
        }
        if cancel.is_cancelled() {
            cancelled = true;
        }

        let files_seen = candidates.len() as u64;
        let to_hash: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.classification.is_movable())
            .map(|(i, _)| i)
            .collect();
        let bytes_to_hash: u64 = to_hash.iter().map(|&i| candidates[i].size_bytes).sum();

        // --- hash --------------------------------------------------------
        let files_hashed = AtomicU64::new(0);
        let bytes_hashed = AtomicU64::new(0);
        let bytes_from_cache = AtomicU64::new(0);
        let bytes_read = AtomicU64::new(0);
        let hashes: Mutex<Vec<(usize, Option<String>)>> = Mutex::new(Vec::new());
        let new_cache_rows: Mutex<Vec<HashCacheRecord>> = Mutex::new(Vec::new());

        if !cancelled {
            sink.emit(&ScanProgress {
                scan_id: scan_id.to_string(),
                phase: ScanPhase::Hashing,
                install_id: None,
                install_label: None,
                files_seen,
                files_to_hash: to_hash.len() as u64,
                files_hashed: 0,
                bytes_to_hash,
                bytes_hashed: 0,
                bytes_from_cache: 0,
                current_path: None,
                elapsed_ms: clock.elapsed().as_millis() as u64,
                eta_ms: None,
            });

            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(
                    std::thread::available_parallelism()
                        .map(|n| n.get().min(MAX_HASH_THREADS))
                        .unwrap_or(2),
                )
                .build()
                .map_err(|e| {
                    VaultError::new(
                        crate::ErrorCode::IoError,
                        "The app could not start the workers that identify files.",
                    )
                    .with_detail(e.to_string())
                })?;

            pool.install(|| {
                to_hash.par_iter().for_each(|&idx| {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let c = &candidates[idx];

                    // The cache stands in for a read only when the size and the
                    // modification time both still match.
                    if self.settings.hash_cache_enabled {
                        if let Ok(Some(row)) = self.store.cached_hash(&c.abs_path) {
                            if row.matches(c.size_bytes, c.mtime_nanos) {
                                hashes.lock().unwrap().push((idx, Some(row.sha256)));
                                files_hashed.fetch_add(1, Ordering::Relaxed);
                                bytes_hashed.fetch_add(c.size_bytes, Ordering::Relaxed);
                                bytes_from_cache.fetch_add(c.size_bytes, Ordering::Relaxed);
                                self.maybe_emit_hash_progress(
                                    scan_id, sink, &throttle, &clock, files_seen,
                                    to_hash.len() as u64, bytes_to_hash, &files_hashed,
                                    &bytes_hashed, &bytes_from_cache, Some(&c.abs_path),
                                );
                                return;
                            }
                        }
                    }

                    let mut read = 0u64;
                    match hash::hash_file_counting(&c.abs_path, cancel, &mut read) {
                        Ok(sha) => {
                            new_cache_rows.lock().unwrap().push(HashCacheRecord {
                                path: c.abs_path.clone(),
                                size_bytes: c.size_bytes,
                                mtime_nanos: c.mtime_nanos,
                                sha256: sha.clone(),
                            });
                            hashes.lock().unwrap().push((idx, Some(sha)));
                            bytes_read.fetch_add(read, Ordering::Relaxed);
                        }
                        Err(e) => {
                            if e.code != crate::ErrorCode::Cancelled {
                                errors.lock().unwrap().push(ScanError {
                                    path: crate::paths::display_path(&c.abs_path),
                                    install_id: Some(c.install_id.clone()),
                                    code: e.code,
                                    detail: e.message.clone(),
                                });
                            }
                            hashes.lock().unwrap().push((idx, None));
                        }
                    }
                    files_hashed.fetch_add(1, Ordering::Relaxed);
                    bytes_hashed.fetch_add(c.size_bytes, Ordering::Relaxed);
                    self.maybe_emit_hash_progress(
                        scan_id, sink, &throttle, &clock, files_seen, to_hash.len() as u64,
                        bytes_to_hash, &files_hashed, &bytes_hashed, &bytes_from_cache,
                        Some(&c.abs_path),
                    );
                });
            });

            if cancel.is_cancelled() {
                cancelled = true;
            }
        }

        // A cancelled scan still keeps the hashes it computed, so restarting it
        // does not read the same terabyte again.
        let rows = new_cache_rows.into_inner().unwrap();
        if self.settings.hash_cache_enabled && !rows.is_empty() {
            self.store.put_cached_hashes(&rows)?;
        }

        // --- finalize ----------------------------------------------------
        for (idx, sha) in hashes.into_inner().unwrap() {
            if let Some(c) = candidates.get_mut(idx) {
                if sha.is_none() {
                    c.classification = Classification::Unreadable;
                }
                c.sha256 = sha;
            }
        }

        let entries: Vec<ScanEntryRecord> = candidates
            .iter()
            .map(|c| ScanEntryRecord {
                abs_path: c.abs_path.clone(),
                rel_path: c.rel_path.clone(),
                install_id: c.install_id.clone(),
                category: c.category.clone(),
                size_bytes: c.size_bytes,
                sha256: c.sha256.clone(),
                mtime_nanos: c.mtime_nanos,
                classification: c.classification,
                link_target: c.link_target.clone(),
            })
            .collect();

        let errors = errors.into_inner().unwrap();
        let duration_ms = clock.elapsed().as_millis() as u64;
        let totals = totals_for(
            &entries,
            bytes_read.load(Ordering::Relaxed),
            bytes_from_cache.load(Ordering::Relaxed),
            duration_ms,
            errors.len() as u64,
        );
        let per_install = installs
            .iter()
            .map(|i| {
                let mine: Vec<ScanEntryRecord> = entries
                    .iter()
                    .filter(|e| e.install_id == i.id)
                    .cloned()
                    .collect();
                InstallScanTotals {
                    install_id: i.id.clone(),
                    install_label: i.label.clone(),
                    totals: totals_for(&mine, 0, 0, 0, 0),
                }
            })
            .collect();

        let record = ScanRecord {
            scan_id: scan_id.to_string(),
            started_at,
            finished_at: Timestamp::now(),
            install_ids: installs.iter().map(|i| i.id.clone()).collect(),
            cancelled,
            totals,
            per_install,
            errors,
        };

        throttle.force_next();
        sink.emit(&ScanProgress {
            scan_id: scan_id.to_string(),
            phase: ScanPhase::Finalizing,
            install_id: None,
            install_label: None,
            files_seen,
            files_to_hash: to_hash.len() as u64,
            files_hashed: files_hashed.load(Ordering::Relaxed),
            bytes_to_hash,
            bytes_hashed: bytes_hashed.load(Ordering::Relaxed),
            bytes_from_cache: bytes_from_cache.load(Ordering::Relaxed),
            current_path: None,
            elapsed_ms: duration_ms,
            eta_ms: None,
        });

        Ok(ScanOutcome { record, entries })
    }

    #[allow(clippy::too_many_arguments)]
    fn maybe_emit_hash_progress(
        &self,
        scan_id: &str,
        sink: &dyn ProgressSink<ScanProgress>,
        throttle: &Throttle,
        clock: &Instant,
        files_seen: u64,
        files_to_hash: u64,
        bytes_to_hash: u64,
        files_hashed: &AtomicU64,
        bytes_hashed: &AtomicU64,
        bytes_from_cache: &AtomicU64,
        current: Option<&Path>,
    ) {
        if !throttle.ready() {
            return;
        }
        let elapsed_ms = clock.elapsed().as_millis() as u64;
        let done = bytes_hashed.load(Ordering::Relaxed);
        sink.emit(&ScanProgress {
            scan_id: scan_id.to_string(),
            phase: ScanPhase::Hashing,
            install_id: None,
            install_label: None,
            files_seen,
            files_to_hash,
            files_hashed: files_hashed.load(Ordering::Relaxed),
            bytes_to_hash,
            bytes_hashed: done,
            bytes_from_cache: bytes_from_cache.load(Ordering::Relaxed),
            current_path: current.map(crate::paths::display_path),
            elapsed_ms,
            eta_ms: estimate_remaining_ms(elapsed_ms, done, bytes_to_hash),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn enumerate_install(
        &self,
        install: &Install,
        out: &mut Vec<Candidate>,
        seen_real: &mut HashSet<PathBuf>,
        errors: &Mutex<Vec<ScanError>>,
        cancel: &CancelToken,
        mut report: impl FnMut(u64, Option<String>),
    ) {
        let roots = install.scan_roots(
            self.settings.follow_extra_model_paths,
            self.settings.scan_output_model_dirs,
        );
        // Anything under custom_nodes is counted, never moved, even when an
        // extra model path points straight into it.
        let custom_nodes = install.custom_nodes_dir();
        let excluded: Vec<PathBuf> = match std::fs::canonicalize(&custom_nodes) {
            Ok(real) if real != custom_nodes => vec![custom_nodes.clone(), real],
            _ => vec![custom_nodes.clone()],
        };

        for root in roots {
            if cancel.is_cancelled() {
                return;
            }
            self.enumerate_dir(
                &root.path,
                &root.path,
                &install.id,
                root.category.as_deref(),
                root.origin,
                &excluded,
                out,
                seen_real,
                errors,
                cancel,
            );
            report(out.len() as u64, Some(crate::paths::display_path(&root.path)));
        }

        // Counted, never moved.
        let custom = custom_nodes;
        if custom.is_dir() {
            self.enumerate_dir(
                &custom,
                &custom,
                &install.id,
                None,
                RootOrigin::CustomNodes,
                &excluded,
                out,
                seen_real,
                errors,
                cancel,
            );
            report(out.len() as u64, Some(crate::paths::display_path(&custom)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn enumerate_dir(
        &self,
        root: &Path,
        _base: &Path,
        install_id: &str,
        root_category: Option<&str>,
        origin: RootOrigin,
        excluded: &[PathBuf],
        out: &mut Vec<Candidate>,
        seen_real: &mut HashSet<PathBuf>,
        errors: &Mutex<Vec<ScanError>>,
        cancel: &CancelToken,
    ) {
        if !root.is_dir() {
            return;
        }
        let vault_root = self.store.vault_root().to_path_buf();

        let walker = walkdir::WalkDir::new(root)
            .follow_links(true)
            .max_depth(MAX_WALK_DEPTH)
            .into_iter()
            .filter_entry(|e| {
                // Never walk into the engine's own folder inside the vault.
                !self.store.is_internal(e.path())
            });

        for entry in walker {
            if cancel.is_cancelled() {
                return;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let path = e.path().map(crate::paths::display_path).unwrap_or_default();
                    errors.lock().unwrap().push(ScanError {
                        path,
                        install_id: Some(install_id.to_string()),
                        code: crate::ErrorCode::IoError,
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let abs_path = entry.path().to_path_buf();

            let Some(name) = abs_path.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if !self.settings.matches_extension(&name) {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    errors.lock().unwrap().push(ScanError {
                        path: crate::paths::display_path(&abs_path),
                        install_id: Some(install_id.to_string()),
                        code: crate::ErrorCode::IoError,
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            let size_bytes = meta.len();
            if size_bytes <= self.settings.min_file_size_bytes {
                continue;
            }

            // One physical file, counted once, however many routes reach it.
            let real_path = std::fs::canonicalize(&abs_path).unwrap_or_else(|_| abs_path.clone());
            if !seen_real.insert(real_path.clone()) {
                continue;
            }

            let is_link = self.platform.is_symlink(&abs_path);
            let link_target = is_link.then(|| real_path.clone());

            let classification = match origin {
                RootOrigin::CustomNodes => Classification::CustomNodes,
                RootOrigin::HuggingFaceCache => Classification::HuggingFaceCache,
                // A folder in extra_model_paths.yaml can point inside
                // custom_nodes. Some packs register their own weights folder
                // that way. Those files must never move, whichever root found
                // them, because the pack also reaches them by its own relative
                // path and a move breaks it.
                _ if excluded.iter().any(|e| abs_path.starts_with(e) || real_path.starts_with(e)) => {
                    Classification::CustomNodes
                }
                _ if is_link && real_path.starts_with(&vault_root) => Classification::AlreadyInVault,
                _ if is_link => Classification::ExternalLink,
                _ => Classification::Movable,
            };

            let rel_path = abs_path.strip_prefix(root).unwrap_or(&abs_path).to_path_buf();
            let category = match root_category {
                Some(c) => c.to_string(),
                None => category_from_rel(&rel_path),
            };

            out.push(Candidate {
                abs_path,
                rel_path,
                install_id: install_id.to_string(),
                category,
                size_bytes,
                mtime_nanos: Timestamp::mtime_nanos(&meta),
                classification,
                link_target,
                sha256: None,
            });
        }
    }
}

/// The category a file belongs to, taken from the first folder below the models
/// root.
///
/// A file lying loose in `models/` belongs to no category, and `misc` keeps it
/// out of the way rather than dropping it.
pub fn category_from_rel(rel: &Path) -> String {
    let mut comps = rel.components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(first)), Some(_)) => first.to_string_lossy().to_string(),
        _ => "misc".to_string(),
    }
}

/// Adds up a set of entries.
pub fn totals_for(
    entries: &[ScanEntryRecord],
    bytes_read: u64,
    bytes_from_cache: u64,
    duration_ms: u64,
    error_count: u64,
) -> ScanTotals {
    let mut t = ScanTotals {
        files_seen: entries.len() as u64,
        bytes_read,
        bytes_from_cache,
        duration_ms,
        error_count,
        ..Default::default()
    };
    let mut unique: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();

    for e in entries {
        match e.classification {
            Classification::Movable => {
                t.movable_files += 1;
                t.movable_bytes += e.size_bytes;
                if let Some(sha) = &e.sha256 {
                    unique.entry(sha.as_str()).or_insert(e.size_bytes);
                }
            }
            Classification::CustomNodes => {
                t.custom_node_files += 1;
                t.custom_node_bytes += e.size_bytes;
            }
            Classification::HuggingFaceCache => {
                t.hf_cache_files += 1;
                t.hf_cache_bytes += e.size_bytes;
            }
            Classification::AlreadyInVault => {
                t.already_linked_files += 1;
                t.already_linked_bytes += e.size_bytes;
            }
            Classification::ExternalLink => t.skipped_files += 1,
            Classification::Unreadable => t.skipped_files += 1,
        }
    }

    t.unique_contents = unique.len() as u64;
    t.unique_bytes = unique.values().sum();
    // Only files that were identified can be counted as duplicates.
    let identified: u64 = entries
        .iter()
        .filter(|e| e.classification.is_movable() && e.sha256.is_some())
        .count() as u64;
    t.duplicate_files = identified.saturating_sub(t.unique_contents);
    t.reclaimable_bytes = entries
        .iter()
        .filter(|e| e.classification.is_movable() && e.sha256.is_some())
        .map(|e| e.size_bytes)
        .sum::<u64>()
        .saturating_sub(t.unique_bytes);
    t
}

#[cfg(test)]
mod tests;
