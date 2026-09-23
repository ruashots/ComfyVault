//! Turning a scan into a plan the person reads before anything moves.
//!
//! A plan is a dry run. Building one changes nothing on disk. The person reads
//! it, unticks whatever they want left alone, and presses Apply. Apply then does
//! only what the plan says, for the groups it was given by name.
//!
//! # One group per unique content
//!
//! Every file with the same hash is one group, whatever it is called and
//! whatever folder it sits in. That is the normal case, not the exception: the
//! same weights live under `loras\awesomeloras\` in one install and under
//! `loras\newloras\` in another, and they become one vault file with a link
//! left at each old location.
//!
//! # Names
//!
//! A link always keeps the name the file had in that install, so a saved
//! workflow keeps working. The vault file's name can differ from it, for two
//! reasons: a name clash with different content forces an adjusted name, and a
//! group whose copies carry different names has to pick one. Every other name
//! in a group becomes a link beside the vault file, so the vault shows every
//! name the content was known by, and the person can change which one the vault
//! keeps later.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::install::Install;
use crate::platform::Platform;
use crate::store::{Classification, ScanEntryRecord, Store};
use crate::time_util::Timestamp;

/// Why a file cannot move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockReason {
    /// Another program holds it open. Windows refuses to move it.
    FileLocked,
    /// The size or the modification time changed after the scan read it.
    FileChanged,
    FileMissing,
    PermissionDenied,
    InCustomNodes,
    InHuggingFaceCache,
    AlreadyInVault,
    ExternalLink,
    SymlinkUnsupported,
    VaultInsideInstall,
    /// Something that is not a link already sits at the vault path.
    TargetExistsNotLink,
    /// The vault path this row would use lands outside the vault.
    ///
    /// The folder name comes from a category in `extra_model_paths.yaml`, which
    /// a person edits by hand and which launchers and node packs also write.
    UnsafeVaultPath,
    NotEnoughSpace,
    ReadError,
}

impl BlockReason {
    /// One sentence for the person, with no file name in it: the interface
    /// shows the path beside the reason.
    pub fn message(self) -> &'static str {
        match self {
            Self::FileLocked => "Another program has this file open. Close ComfyUI and scan again.",
            Self::FileChanged => "This file changed after the scan read it, so it was left alone.",
            Self::FileMissing => "This file is no longer there.",
            Self::PermissionDenied => "Windows refused access to this file.",
            Self::InCustomNodes => "This file belongs to a custom node, which reaches it by its own folder. Moving it would break the node.",
            Self::InHuggingFaceCache => "This file is in the Hugging Face cache, which manages its own layout.",
            Self::AlreadyInVault => "This is already a link into the vault.",
            Self::ExternalLink => "This is a link to somewhere outside the vault, so there is nothing here to move.",
            Self::SymlinkUnsupported => "This computer cannot create the links this app uses. Turn Developer Mode on.",
            Self::VaultInsideInstall => "The vault folder is inside this install, which would make the app move files into themselves.",
            Self::TargetExistsNotLink => "A different file already sits at the vault path this would use.",
            Self::UnsafeVaultPath => "This model's folder name would put it outside the vault, so it was left alone. Check the model folder names in extra_model_paths.yaml.",
            Self::NotEnoughSpace => "The vault drive does not have room for this file.",
            Self::ReadError => "This file could not be read.",
        }
    }
}

/// Why one copy was chosen to become the vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceChoice {
    /// It already sits on the vault's drive, so the move is a rename: instant,
    /// and it needs no extra space.
    SameVolume,
    OnlyCopy,
    FirstByPath,
}

/// The copy that becomes the vault file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSource {
    pub install_id: String,
    pub install_label: String,
    pub abs_path: PathBuf,
    pub rel_path: PathBuf,
    pub same_volume_as_vault: bool,
    pub chosen_because: SourceChoice,
    pub size_bytes: u64,
    pub mtime_nanos: i128,
}

/// A place that ends up holding a link.
///
/// **Every** copy in the group is here, including the one that becomes the
/// vault file, because the product's promise is that a link takes the place of
/// every file that was there. So `links.len()` always equals `occurrences`.
/// `is_source` marks the one whose bytes move into the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanLink {
    pub install_id: String,
    pub install_label: String,
    pub abs_path: PathBuf,
    pub rel_path: PathBuf,
    /// The name the link keeps, which is the name the file had here.
    pub link_name: String,
    pub name_differs_from_vault: bool,
    /// This copy's bytes become the vault file. Its old place gets a link like
    /// every other, but nothing is deleted here: the file moved.
    pub is_source: bool,
    /// This path is a second name for a file already counted in this group,
    /// so removing it returns no space. Two hard links are two names for one
    /// set of bytes, and the bytes stay while any name remains.
    pub shares_bytes_with_another: bool,
    pub size_bytes: u64,
    pub mtime_nanos: i128,
}

/// One unique content, and everything that happens to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanGroup {
    pub group_id: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub category: String,
    pub vault_rel_path: PathBuf,
    /// True when a clash forced a name other than the file's own.
    pub vault_name_adjusted: bool,
    /// The content that already holds the plain name, when there was a clash.
    pub clashes_with: Option<String>,
    /// Other names this content is known by, each becoming a link beside the
    /// vault file. This is what the cleanup screen later works on.
    pub vault_aliases: Vec<String>,
    /// Which copy becomes the vault file. It also appears in `links`, flagged.
    pub source: PlanSource,
    /// Every place that ends up holding a link, the source included. Always
    /// `occurrences` long.
    pub links: Vec<PlanLink>,
    pub occurrences: u64,
    /// How many real files the group's paths actually are. Lower than
    /// `occurrences` when some of them are hard links to each other.
    pub distinct_files: u64,
    pub bytes_freed: u64,
    /// One copy only. It moves into the vault and frees nothing.
    pub single_copy: bool,
    pub cross_volume: bool,
}

/// A file that cannot move, and why.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedRow {
    pub abs_path: PathBuf,
    pub install_id: Option<String>,
    pub install_label: Option<String>,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub reason: BlockReason,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTotals {
    pub groups: u64,
    pub groups_freeing_space: u64,
    pub single_copy_groups: u64,
    pub name_clashes: u64,
    pub cross_volume_groups: u64,
    pub bytes_freed: u64,
    pub bytes_moved: u64,
    pub files_moved: u64,
    pub links_created: u64,
    pub blocked_rows: u64,
    pub blocked_bytes: u64,
    pub vault_free_bytes_after: u64,
}

/// The whole plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsolidationPlan {
    pub plan_id: String,
    pub scan_id: String,
    pub created_at: Timestamp,
    pub vault_root: PathBuf,
    /// Can this computer create the links the plan needs?
    ///
    /// When `false` the groups are still built, so the person can read what
    /// they would gain before deciding to turn Developer Mode on. Apply refuses
    /// while it is `false`.
    pub symlinks_supported: bool,
    pub groups: Vec<PlanGroup>,
    pub blocked: Vec<BlockedRow>,
    pub totals: PlanTotals,
}

impl ConsolidationPlan {
    pub fn group(&self, id: &str) -> Option<&PlanGroup> {
        self.groups.iter().find(|g| g.group_id == id)
    }

    /// The groups named by the caller, in plan order.
    ///
    /// An identifier that names nothing is reported, never ignored: Apply must
    /// not quietly do less than the caller asked.
    pub fn select(&self, ids: &[String]) -> Result<Vec<&PlanGroup>> {
        let mut out = Vec::new();
        for id in ids {
            let g = self.group(id).ok_or_else(|| {
                crate::VaultError::not_found("One of the selected rows is no longer in this plan. Scan again.")
                    .with_detail(format!("unknown group id {id}"))
            })?;
            out.push(g);
        }
        Ok(out)
    }
}

/// Builds a plan from a scan.
pub struct Planner<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
}

impl<'a> Planner<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform) -> Self {
        Self { store, platform }
    }

    /// Builds the plan. Reads the disk, writes nothing.
    pub fn build(
        &self,
        plan_id: &str,
        scan_id: &str,
        entries: &[ScanEntryRecord],
        installs: &[Install],
    ) -> Result<ConsolidationPlan> {
        let vault_root = self.store.vault_root().to_path_buf();
        let by_install: BTreeMap<&str, &Install> =
            installs.iter().map(|i| (i.id.as_str(), i)).collect();

        let symlinks_ok = self.platform.symlink_capability().supported;
        let vault_volume = self.platform.volume_id(&vault_root).ok();

        let mut groups: Vec<PlanGroup> = Vec::new();
        let mut blocked: Vec<BlockedRow> = Vec::new();

        // A vault inside an install would make the engine move files into
        // themselves, so the whole install is refused rather than half handled.
        let vault_inside: Vec<&str> = installs
            .iter()
            .filter(|i| vault_root.starts_with(&i.root))
            .map(|i| i.id.as_str())
            .collect();

        // --- gather the movable entries into groups by content -------------
        let mut by_hash: BTreeMap<&str, Vec<&ScanEntryRecord>> = BTreeMap::new();

        for e in entries {
            let label = by_install.get(e.install_id.as_str()).map(|i| i.label.clone());

            if vault_inside.contains(&e.install_id.as_str()) {
                blocked.push(row(e, label, BlockReason::VaultInsideInstall));
                continue;
            }
            match e.classification {
                // Counted, never moved. These are reported by the scan totals
                // as bytes; listing thousands of them here would bury the rows
                // the person can actually act on.
                Classification::CustomNodes | Classification::HuggingFaceCache => continue,
                Classification::AlreadyInVault => continue,
                Classification::ExternalLink => {
                    blocked.push(row(e, label, BlockReason::ExternalLink));
                    continue;
                }
                Classification::Unreadable => {
                    blocked.push(row(e, label, BlockReason::ReadError));
                    continue;
                }
                Classification::Movable => {}
            }
            let Some(sha) = e.sha256.as_deref() else {
                blocked.push(row(e, label, BlockReason::ReadError));
                continue;
            };
            // Re-check the file now, because a plan is built after a scan and
            // the person may have moved on with their day in between.
            if let Some(reason) = self.recheck(&e.abs_path, e.size_bytes, e.mtime_nanos) {
                blocked.push(row(e, label, reason));
                continue;
            }
            by_hash.entry(sha).or_default().push(e);
        }

        // --- turn each content into a group --------------------------------
        // Sorted by hash so a plan built twice from one scan is identical.
        let mut taken_names: BTreeMap<(String, String), String> = BTreeMap::new();
        for f in self.store.vault_files()? {
            for n in f.all_names() {
                taken_names.insert((f.category.clone(), n.to_lowercase()), f.sha256.clone());
            }
        }

        // Names are handed out in this order: the content with the most copies
        // keeps the plain name, and the hash breaks a tie so the result never
        // depends on the order the disk returned. The rule matters when two
        // different files share one name: the widely used one keeps the name
        // people recognize, and the other takes an adjusted one.
        let mut ordered: Vec<(&str, Vec<&ScanEntryRecord>)> = by_hash.into_iter().collect();
        ordered.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));

        for (sha, mut members) in ordered {
            members.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));
            let size_bytes = members[0].size_bytes;
            let category = members[0].category.clone();

            let source_idx = choose_source(&members, self.platform, vault_volume.as_ref());
            let source_entry = members[source_idx];
            let source_name = file_name_of(&source_entry.abs_path);

            // Settle the vault name. A name already held by different content
            // forces an adjusted name, so both survive.
            let key = (category.clone(), source_name.to_lowercase());
            let (vault_name, adjusted, clashes_with) = match taken_names.get(&key) {
                Some(owner) if owner != sha => {
                    // Widen the hash fragment until the adjusted name is free
                    // too. Claiming a name another content owns would only
                    // fail later, at the write, after the person read a plan
                    // that said it would work.
                    let mut width = 8;
                    let mut candidate = adjusted_name(&source_name, sha, width);
                    while width < 64
                        && taken_names
                            .get(&(category.clone(), candidate.to_lowercase()))
                            .map(|o| o != sha)
                            .unwrap_or(false)
                    {
                        width += 4;
                        candidate = adjusted_name(&source_name, sha, width);
                    }
                    (candidate, true, Some(owner.clone()))
                }
                _ => (source_name.clone(), false, None),
            };
            taken_names.insert((category.clone(), vault_name.to_lowercase()), sha.to_string());

            // Every other name this content carries becomes a link beside the
            // vault file, unless that name is already taken by other content.
            let mut aliases: Vec<String> = Vec::new();
            for m in &members {
                let n = file_name_of(&m.abs_path);
                if n.eq_ignore_ascii_case(&vault_name) || aliases.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
                    continue;
                }
                let k = (category.clone(), n.to_lowercase());
                if taken_names.get(&k).map(|o| o != sha).unwrap_or(false) {
                    continue;
                }
                taken_names.insert(k, sha.to_string());
                aliases.push(n);
            }

            let same_volume = vault_volume
                .as_ref()
                .zip(self.platform.volume_id(&source_entry.abs_path).ok().as_ref())
                .map(|(a, b)| a == b)
                .unwrap_or(false);

            let source = PlanSource {
                install_id: source_entry.install_id.clone(),
                install_label: label_of(&by_install, &source_entry.install_id),
                abs_path: source_entry.abs_path.clone(),
                rel_path: source_entry.rel_path.clone(),
                same_volume_as_vault: same_volume,
                chosen_because: if members.len() == 1 {
                    SourceChoice::OnlyCopy
                } else if same_volume {
                    SourceChoice::SameVolume
                } else {
                    SourceChoice::FirstByPath
                },
                size_bytes,
                mtime_nanos: source_entry.mtime_nanos,
            };

            // Which of these paths are actually the same file. Two hard links
            // are two names for one set of bytes, so removing one returns
            // nothing. Counting them as reclaimable overstates the one number
            // the person presses Apply for and checks afterwards, and it is
            // better to come in under than over.
            let mut seen_files: Vec<crate::platform::FileIdentity> = Vec::new();
            let mut shares: Vec<bool> = Vec::with_capacity(members.len());
            for m in &members {
                match self.platform.file_identity(&m.abs_path) {
                    Some(id) => {
                        let already = seen_files.contains(&id);
                        if !already {
                            seen_files.push(id);
                        }
                        shares.push(already);
                    }
                    // The system could not say, so count it as its own file.
                    // That is the conservative direction for a space figure.
                    None => shares.push(false),
                }
            }
            let distinct_files =
                shares.iter().filter(|s| !**s).count().max(1) as u64;

            // Every copy, the source included: a link takes the place of each
            // file that was there, so the count the person reads is the count
            // of places that change.
            let links: Vec<PlanLink> = members
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let name = file_name_of(&m.abs_path);
                    PlanLink {
                        shares_bytes_with_another: shares[i],
                        install_id: m.install_id.clone(),
                        install_label: label_of(&by_install, &m.install_id),
                        abs_path: m.abs_path.clone(),
                        rel_path: m.rel_path.clone(),
                        name_differs_from_vault: !name.eq_ignore_ascii_case(&vault_name),
                        link_name: name,
                        is_source: i == source_idx,
                        size_bytes: m.size_bytes,
                        mtime_nanos: m.mtime_nanos,
                    }
                })
                .collect();

            let occurrences = members.len() as u64;
            // Prove the vault path lands inside the vault, here, against the
            // real resolved location. The category came from a YAML key four
            // modules ago, so trusting it at the write would be trusting a
            // value that travelled. Every alias is proved the same way.
            let vault_rel_path = PathBuf::from(&category).join(&vault_name);
            let every_vault_path: Vec<PathBuf> = std::iter::once(vault_rel_path.clone())
                .chain(aliases.iter().map(|a| PathBuf::from(&category).join(a)))
                .collect();
            let install_label = Some(label_of(&by_install, &source_entry.install_id));

            // Two things must hold, and containment alone is not enough. A
            // category of `a/b` stays inside the vault but builds a folder
            // inside a folder, and the whole vault layout is one level:
            // category, then file. So the category must also be one safe path
            // component, which is what the parser now requires of it.
            let unsafe_path = crate::paths::validate_file_name(&category).is_err()
                || every_vault_path
                    .iter()
                    .any(|rel| crate::paths::resolve_new_path_within(&vault_root, rel).is_err());
            if unsafe_path {
                blocked.push(row(source_entry, install_label, BlockReason::UnsafeVaultPath));
                continue;
            }

            // A file already sitting at the vault path that the database does
            // not know about, left by an earlier crash or copied in by hand.
            // Apply would refuse it, so the plan must not promise it.
            let settled = vault_root.join(&vault_rel_path);
            if std::fs::symlink_metadata(&settled).is_ok() {
                blocked.push(row(source_entry, install_label, BlockReason::TargetExistsNotLink));
                continue;
            }

            groups.push(PlanGroup {
                group_id: format!("g-{sha}"),
                sha256: sha.to_string(),
                size_bytes,
                vault_rel_path,
                category,
                vault_name_adjusted: adjusted,
                clashes_with,
                vault_aliases: aliases,
                source,
                links,
                occurrences,
                distinct_files,
                bytes_freed: size_bytes * (distinct_files - 1),
                single_copy: occurrences == 1,
                cross_volume: !same_volume,
            });
        }

        // --- space ---------------------------------------------------------
        // Only a cross-drive move needs room in the vault. A same-drive move is
        // a rename and costs nothing.
        let bytes_needed: u64 = groups
            .iter()
            .filter(|g| g.cross_volume)
            .map(|g| g.size_bytes)
            .sum();
        let free_now = self.platform.disk_space(&vault_root).map(|s| s.free_bytes).unwrap_or(0);
        if bytes_needed > free_now {
            // Block the largest first, so the person keeps the most groups they
            // can actually apply.
            let mut idx: Vec<usize> = (0..groups.len()).filter(|&i| groups[i].cross_volume).collect();
            idx.sort_by_key(|&i| std::cmp::Reverse(groups[i].size_bytes));
            let mut still_needed = bytes_needed;
            let mut drop: Vec<usize> = Vec::new();
            for i in idx {
                if still_needed <= free_now {
                    break;
                }
                still_needed -= groups[i].size_bytes;
                drop.push(i);
            }
            drop.sort_unstable_by(|a, b| b.cmp(a));
            for i in drop {
                let g = groups.remove(i);
                blocked.push(BlockedRow {
                    abs_path: g.source.abs_path.clone(),
                    install_id: Some(g.source.install_id.clone()),
                    install_label: Some(g.source.install_label.clone()),
                    size_bytes: g.size_bytes,
                    sha256: Some(g.sha256.clone()),
                    reason: BlockReason::NotEnoughSpace,
                    detail: BlockReason::NotEnoughSpace.message().to_string(),
                });
            }
        }

        // A computer that cannot make links still gets a full plan. The person
        // reads what they would gain, and that is what makes them go and turn
        // Developer Mode on. A blank screen tells them nothing.
        //
        // One row says so, not one per group: the reason is a fact about the
        // computer, not about any file. Apply refuses separately, so nothing
        // can act on a plan that only looks applicable.
        if !symlinks_ok && !groups.is_empty() {
            blocked.push(BlockedRow {
                abs_path: vault_root.clone(),
                install_id: None,
                install_label: None,
                size_bytes: 0,
                sha256: None,
                reason: BlockReason::SymlinkUnsupported,
                detail: BlockReason::SymlinkUnsupported.message().to_string(),
            });
        }

        groups.sort_by(|a, b| b.bytes_freed.cmp(&a.bytes_freed).then(a.sha256.cmp(&b.sha256)));
        blocked.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));

        let totals = totals_for(&groups, &blocked, free_now);
        Ok(ConsolidationPlan {
            plan_id: plan_id.to_string(),
            scan_id: scan_id.to_string(),
            created_at: Timestamp::now(),
            symlinks_supported: symlinks_ok,
            vault_root,
            groups,
            blocked,
            totals,
        })
    }

    /// Is the file still exactly what the scan read?
    fn recheck(&self, path: &Path, size_bytes: u64, mtime_nanos: i128) -> Option<BlockReason> {
        let meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(BlockReason::FileMissing),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Some(BlockReason::PermissionDenied)
            }
            Err(_) => return Some(BlockReason::ReadError),
        };
        if meta.len() != size_bytes || Timestamp::mtime_nanos(&meta) != mtime_nanos {
            return Some(BlockReason::FileChanged);
        }
        let lock = self.platform.lock_state(path);
        if lock.locked {
            return Some(BlockReason::FileLocked);
        }
        None
    }
}

fn label_of(by_install: &BTreeMap<&str, &Install>, id: &str) -> String {
    by_install.get(id).map(|i| i.label.clone()).unwrap_or_else(|| id.to_string())
}

fn row(e: &ScanEntryRecord, label: Option<String>, reason: BlockReason) -> BlockedRow {
    BlockedRow {
        abs_path: e.abs_path.clone(),
        install_id: Some(e.install_id.clone()),
        install_label: label,
        size_bytes: e.size_bytes,
        sha256: e.sha256.clone(),
        reason,
        detail: reason.message().to_string(),
    }
}

/// Picks the copy that becomes the vault file.
///
/// A copy already on the vault's drive wins, because that move is a rename:
/// instant, and it needs no second copy of the bytes. Otherwise the first by
/// sorted path wins, so the choice never depends on the order the disk
/// happened to return.
fn choose_source(
    members: &[&ScanEntryRecord],
    platform: &dyn Platform,
    vault_volume: Option<&crate::platform::VolumeId>,
) -> usize {
    if let Some(vv) = vault_volume {
        for (i, m) in members.iter().enumerate() {
            if platform.volume_id(&m.abs_path).ok().as_ref() == Some(vv) {
                return i;
            }
        }
    }
    0
}

fn file_name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// `lora1.safetensors` becomes `lora1__3F9A2C17.safetensors`.
///
/// The hash fragment makes the name unique without hiding what the file is.
/// `width` grows only when the short form is itself taken.
fn adjusted_name(name: &str, sha256: &str, width: usize) -> String {
    let short: String = sha256.chars().take(width).collect();
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}__{short}.{ext}"),
        _ => format!("{name}__{short}"),
    }
}

fn totals_for(groups: &[PlanGroup], blocked: &[BlockedRow], free_now: u64) -> PlanTotals {
    let bytes_freed: u64 = groups.iter().map(|g| g.bytes_freed).sum();
    let cross: u64 = groups.iter().filter(|g| g.cross_volume).map(|g| g.size_bytes).sum();
    PlanTotals {
        groups: groups.len() as u64,
        groups_freeing_space: groups.iter().filter(|g| !g.single_copy).count() as u64,
        single_copy_groups: groups.iter().filter(|g| g.single_copy).count() as u64,
        name_clashes: groups.iter().filter(|g| g.vault_name_adjusted).count() as u64,
        cross_volume_groups: groups.iter().filter(|g| g.cross_volume).count() as u64,
        bytes_freed,
        bytes_moved: groups.iter().map(|g| g.size_bytes).sum(),
        files_moved: groups.len() as u64,
        links_created: groups.iter().map(|g| g.links.len() as u64 + g.vault_aliases.len() as u64).sum(),
        blocked_rows: blocked.len() as u64,
        blocked_bytes: blocked.iter().map(|b| b.size_bytes).sum(),
        // A same-drive move frees its duplicates and costs nothing; a
        // cross-drive move spends vault space and frees space on the other one.
        vault_free_bytes_after: free_now.saturating_sub(cross),
    }
}

#[cfg(test)]
mod tests;
