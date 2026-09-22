//! Applying a plan, undoing it, and recovering from a crash.
//!
//! # The three promises
//!
//! **Apply does only what the plan says.** It reads the stored plan and acts on
//! the groups it was given by name. It never re-plans and never widens the work.
//!
//! **Apply checks every file before it touches it.** A file that changed since
//! the scan stops that row and is reported. Nothing is moved unchecked.
//!
//! **Every step is written down before it happens.** The journal entry reaches
//! the disk first, then the filesystem step runs, then the entry is marked done.
//! A crash therefore leaves at worst one step whose outcome is unknown, and the
//! recovery reads the disk to find out which way it went.
//!
//! # A group is all or nothing
//!
//! A group either completes or leaves the disk as it was. If a step inside one
//! fails, the steps already done in that group are undone in reverse before the
//! next group starts. One group failing never leaves another half done.
//!
//! # Why a duplicate is renamed rather than deleted
//!
//! Replacing a duplicate with a link cannot be one operation. So the file is
//! renamed aside, the link is created, and only then is the renamed file
//! removed. An interruption at any point leaves the bytes present under one
//! name or the other, never gone.

pub mod fsops;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, Result, VaultError};
use crate::install::Install;
use crate::plan::{BlockReason, ConsolidationPlan, PlanGroup};
use crate::platform::Platform;
use crate::progress::{estimate_remaining_ms, CancelToken, ProgressSink, Throttle};
use crate::store::{
    ApplyFailure, ApplyRecord, ApplyState, JournalEntry, JournalState, JournalStep, LinkOrigin,
    LinkRecord, Store, VaultFileRecord,
};
use crate::time_util::Timestamp;

pub use fsops::{MoveKind, VerifyMode, VerifyResult};

/// What the caller asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub plan_id: String,
    /// Only these groups are applied. An empty list applies nothing.
    pub group_ids: Vec<String>,
    #[serde(default = "default_verify")]
    pub verify: VerifyModeArg,
    #[serde(default)]
    pub stop_on_error: bool,
}

fn default_verify() -> VerifyModeArg {
    VerifyModeArg::SizeAndMtime
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerifyModeArg {
    SizeAndMtime,
    Rehash,
}

impl From<VerifyModeArg> for VerifyMode {
    fn from(v: VerifyModeArg) -> Self {
        match v {
            VerifyModeArg::SizeAndMtime => VerifyMode::SizeAndMtime,
            VerifyModeArg::Rehash => VerifyMode::Rehash,
        }
    }
}

/// Which part of an apply is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyPhase {
    Preflight,
    Applying,
    Finalizing,
}

/// What the engine is doing to the current file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyStep {
    Verifying,
    Moving,
    Linking,
    Cleaning,
}

/// A progress update, as the contract defines it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProgress {
    pub apply_id: String,
    pub phase: ApplyPhase,
    pub group_index: u64,
    pub group_total: u64,
    pub current_group_id: Option<String>,
    pub current_path: Option<String>,
    pub step: ApplyStep,
    pub bytes_moved: u64,
    pub bytes_to_move: u64,
    pub bytes_freed: u64,
    pub files_moved: u64,
    pub links_created: u64,
    pub failures: u64,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
}

/// An apply run that stopped in the middle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedApply {
    pub apply_id: String,
    pub plan_id: String,
    pub started_at: Timestamp,
    pub steps_done: u64,
    pub steps_pending: u64,
    /// One sentence for the person.
    pub description: String,
    pub affected_paths: Vec<String>,
}

/// Applies plans, undoes them, and recovers from a crash.
pub struct Applier<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
}

/// Running state for one apply run.
struct Run {
    apply_id: String,
    seq: u64,
    clock: std::time::Instant,
    throttle: Throttle,
    bytes_to_move: u64,
    bytes_moved: u64,
    bytes_freed: u64,
    files_moved: u64,
    links_created: u64,
    groups_applied: u64,
    failures: Vec<ApplyFailure>,
}

impl<'a> Applier<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform) -> Self {
        Self { store, platform }
    }

    // -- apply ------------------------------------------------------------

    /// Applies the groups the caller named.
    pub fn apply(
        &self,
        apply_id: &str,
        plan: &ConsolidationPlan,
        req: &ApplyRequest,
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ApplyProgress>,
    ) -> Result<ApplyRecord> {
        // Named explicitly, never inferred. Apply must not quietly do more or
        // less than the person ticked.
        let groups: Vec<PlanGroup> = plan.select(&req.group_ids)?.into_iter().cloned().collect();

        if !self.platform.symlink_capability().supported {
            return Err(VaultError::new(
                ErrorCode::SymlinkUnsupported,
                crate::platform::DEVELOPER_MODE_GUIDANCE,
            ));
        }

        let mut run = Run {
            apply_id: apply_id.to_string(),
            seq: 0,
            clock: std::time::Instant::now(),
            throttle: Throttle::per_second(4),
            bytes_to_move: groups.iter().map(|g| g.size_bytes).sum(),
            bytes_moved: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            groups_applied: 0,
            failures: Vec::new(),
        };

        let mut record = ApplyRecord {
            apply_id: apply_id.to_string(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: Timestamp::now(),
            finished_at: None,
            groups_requested: groups.len() as u64,
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            failures: Vec::new(),
            revertible: true,
        };
        // Written before any work, so a crash leaves a record to recover from.
        self.store.put_apply(&record)?;

        let installs = self.store.installs()?;
        let mut cancelled = false;

        for (index, group) in groups.iter().enumerate() {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            self.emit(
                sink, &mut run, ApplyPhase::Applying, index as u64, groups.len() as u64,
                Some(group), ApplyStep::Verifying, None, true,
            );

            match self.apply_group(&mut run, group, &installs, req.verify.into(), cancel, sink, index, groups.len()) {
                Ok(()) => run.groups_applied += 1,
                Err(e) => {
                    let reason = block_reason_for(&e);
                    run.failures.push(ApplyFailure {
                        group_id: group.group_id.clone(),
                        abs_path: crate::paths::display_path(&group.source.abs_path),
                        reason,
                        detail: e.message.clone(),
                    });
                    if e.code == ErrorCode::Cancelled {
                        cancelled = true;
                        break;
                    }
                    if req.stop_on_error {
                        break;
                    }
                }
            }
        }

        record.state = if cancelled {
            ApplyState::Cancelled
        } else if run.failures.is_empty() {
            ApplyState::Completed
        } else {
            ApplyState::CompletedWithErrors
        };
        record.finished_at = Some(Timestamp::now());
        record.groups_applied = run.groups_applied;
        record.groups_failed = run.failures.len() as u64;
        record.bytes_freed = run.bytes_freed;
        record.files_moved = run.files_moved;
        record.links_created = run.links_created;
        record.failures = run.failures.clone();
        record.revertible = run.groups_applied > 0;
        self.store.put_apply(&record)?;

        run.throttle.force_next();
        self.emit(
            sink, &mut run, ApplyPhase::Finalizing, groups.len() as u64, groups.len() as u64,
            None, ApplyStep::Cleaning, None, true,
        );
        Ok(record)
    }

    /// Applies one group, or leaves the disk exactly as it was.
    #[allow(clippy::too_many_arguments)]
    fn apply_group(
        &self,
        run: &mut Run,
        group: &PlanGroup,
        installs: &[Install],
        verify: VerifyMode,
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ApplyProgress>,
        index: usize,
        total: usize,
    ) -> Result<()> {
        let vault_root = self.store.vault_root().to_path_buf();
        let temp_dir = self.store.temp_dir();

        // Proved here, at the write, not assumed from a plan that was built
        // earlier and stored. The folder name inside a vault path comes from a
        // category in extra_model_paths.yaml, so it is a value that travelled
        // through four modules before arriving. A stored plan from an older
        // build could still carry a bad one.
        let vault_path = crate::paths::resolve_new_path_within(&vault_root, &group.vault_rel_path)
            .map_err(|e| {
                VaultError::new(
                    ErrorCode::PathOutsideBoundary,
                    BlockReason::UnsafeVaultPath.message(),
                )
                .with_detail(e.message)
                .with_path(&group.vault_rel_path)
            })?;

        // --- check everything before touching anything --------------------
        // A group that cannot complete must not start, or the person is left
        // with a half-consolidated model.
        // `links` already covers every copy, the source included.
        let checks: Vec<(&Path, u64, i128)> = group
            .links
            .iter()
            .map(|l| (l.abs_path.as_path(), l.size_bytes, l.mtime_nanos))
            .collect();

        for (path, size, mtime) in &checks {
            cancel.check()?;
            let result = fsops::verify(path, *size, *mtime, &group.sha256, verify, cancel);
            if let Some(reason) = result.to_block_reason() {
                return Err(VaultError::new(code_for(reason), reason.message()).with_path(path));
            }
            let lock = self.platform.lock_state(path);
            if lock.locked {
                return Err(VaultError::new(ErrorCode::FileLocked, BlockReason::FileLocked.message())
                    .with_path(path));
            }
        }

        // --- do the work, remembering how to undo it ----------------------
        let mut done: Vec<JournalEntry> = Vec::new();

        let outcome = (|| -> Result<()> {
            if let Some(parent) = vault_path.parent() {
                if !parent.is_dir() {
                    let entry = self.step(run, group, JournalStep::CreateDir { path: parent.to_path_buf() })?;
                    fsops::ensure_dir(parent)?;
                    done.push(self.finish(run, entry)?);
                }
            }

            // 1. The chosen copy becomes the vault file.
            self.emit(sink, run, ApplyPhase::Applying, index as u64, total as u64,
                Some(group), ApplyStep::Moving, Some(&group.source.abs_path), false);

            let entry = self.step(run, group, JournalStep::MoveToVault {
                from: group.source.abs_path.clone(),
                to: vault_path.clone(),
                copied: false,
                sha256: group.sha256.clone(),
                size_bytes: group.size_bytes,
            })?;
            let kind = fsops::move_file(
                self.platform, &group.source.abs_path, &vault_path, &group.sha256, &temp_dir, cancel,
            )?;
            let entry = JournalEntry {
                step: JournalStep::MoveToVault {
                    from: group.source.abs_path.clone(),
                    to: vault_path.clone(),
                    copied: kind == MoveKind::Copied,
                    sha256: group.sha256.clone(),
                    size_bytes: group.size_bytes,
                },
                ..entry
            };
            done.push(self.finish(run, entry)?);
            run.files_moved += 1;
            run.bytes_moved += group.size_bytes;

            // 2. Every copy's old place gets a link.
            //
            //    The source's place is already empty, because its bytes just
            //    moved into the vault, so it only needs the link. Every other
            //    copy is renamed aside first and removed last, so an
            //    interruption leaves its bytes under one name or the other.
            for link in &group.links {
                cancel.check()?;
                self.emit(sink, run, ApplyPhase::Applying, index as u64, total as u64,
                    Some(group), ApplyStep::Linking, Some(&link.abs_path), false);

                if link.is_source {
                    done.push(self.create_link_step(run, group, &link.abs_path, &vault_path)?);
                    run.links_created += 1;
                    continue;
                }

                let entry = self.step(run, group, JournalStep::StashOriginal {
                    path: link.abs_path.clone(),
                    stash: PathBuf::new(),
                })?;
                let stash_path = fsops::stash(&link.abs_path, &short_id(&run.apply_id))?;
                let entry = JournalEntry {
                    step: JournalStep::StashOriginal {
                        path: link.abs_path.clone(),
                        stash: stash_path.clone(),
                    },
                    ..entry
                };
                done.push(self.finish(run, entry)?);

                done.push(self.create_link_step(run, group, &link.abs_path, &vault_path)?);
                run.links_created += 1;

                self.emit(sink, run, ApplyPhase::Applying, index as u64, total as u64,
                    Some(group), ApplyStep::Cleaning, Some(&link.abs_path), false);
                let entry = self.step(run, group, JournalStep::DeleteStash {
                    stash: stash_path.clone(),
                    original: link.abs_path.clone(),
                    vault_path: vault_path.clone(),
                    sha256: group.sha256.clone(),
                    size_bytes: link.size_bytes,
                })?;
                std::fs::remove_file(&stash_path).map_err(|e| {
                    VaultError::from_io(&e, &stash_path, "removing the duplicate")
                })?;
                done.push(self.finish(run, entry)?);
                run.bytes_freed += link.size_bytes;
            }

            // 4. Every other name this content carries becomes a link beside
            //    the vault file, so the vault shows every name it was known by.
            for alias in &group.vault_aliases {
                cancel.check()?;
                // Proved the same way as the vault path itself: an alias is a
                // second name for the same content, and its folder comes from
                // the same untrusted category.
                let alias_rel = PathBuf::from(&group.category).join(alias);
                let alias_path =
                    crate::paths::resolve_new_path_within(&vault_root, &alias_rel).map_err(|e| {
                        VaultError::new(
                            ErrorCode::PathOutsideBoundary,
                            BlockReason::UnsafeVaultPath.message(),
                        )
                        .with_detail(e.message)
                        .with_path(&alias_rel)
                    })?;
                if alias_path.exists() || self.platform.is_symlink(&alias_path) {
                    continue;
                }
                done.push(self.create_link_step(run, group, &alias_path, &vault_path)?);
            }
            Ok(())
        })();

        if let Err(e) = outcome {
            // Undo this group, so the disk looks the way it did before it
            // started. Any failure to undo is reported with the original cause.
            self.undo_entries(&mut done, cancel)?;
            return Err(e);
        }

        self.record_group(group, &vault_path, installs, &run.apply_id)?;
        Ok(())
    }

    /// Writes the vault file and the links into the database.
    fn record_group(
        &self,
        group: &PlanGroup,
        vault_path: &Path,
        installs: &[Install],
        apply_id: &str,
    ) -> Result<()> {
        let canonical_name = vault_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        self.store.put_vault_file(&VaultFileRecord {
            sha256: group.sha256.clone(),
            canonical_name,
            category: group.category.clone(),
            size_bytes: group.size_bytes,
            added_at: Timestamp::now(),
            aliases: group.vault_aliases.clone(),
        })?;

        let record_link = |install_id: &str, abs: &Path, name: &str| -> Result<()> {
            let rel = installs
                .iter()
                .find(|i| i.id == install_id)
                .and_then(|i| abs.strip_prefix(&i.root).ok())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(name));
            self.store.put_link(&LinkRecord {
                id: uuid::Uuid::new_v4().to_string(),
                install_id: install_id.to_string(),
                abs_path: abs.to_path_buf(),
                rel_path: rel,
                link_name: name.to_string(),
                sha256: group.sha256.clone(),
                vault_rel_path: group.vault_rel_path.clone(),
                created_at: Timestamp::now(),
                created_by: LinkOrigin::Apply,
                apply_id: Some(apply_id.to_string()),
            })
        };

        // `links` already covers every place, the source included.
        for l in &group.links {
            record_link(&l.install_id, &l.abs_path, &l.link_name)?;
        }
        Ok(())
    }

    // -- journal ----------------------------------------------------------

    /// Writes a pending entry and waits for it to reach the disk.
    fn step(&self, run: &mut Run, group: &PlanGroup, step: JournalStep) -> Result<JournalEntry> {
        let entry = JournalEntry {
            apply_id: run.apply_id.clone(),
            seq: run.seq,
            group_id: group.group_id.clone(),
            step,
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        };
        run.seq += 1;
        self.store.append_journal(&entry)?;
        Ok(entry)
    }

    fn finish(&self, _run: &mut Run, entry: JournalEntry) -> Result<JournalEntry> {
        let done = JournalEntry {
            state: JournalState::Done,
            finished_at: Some(Timestamp::now()),
            ..entry
        };
        self.store.update_journal(&done)?;
        Ok(done)
    }

    fn create_link_step(
        &self,
        run: &mut Run,
        group: &PlanGroup,
        link: &Path,
        target: &Path,
    ) -> Result<JournalEntry> {
        let entry = self.step(run, group, JournalStep::CreateLink {
            link: link.to_path_buf(),
            target: target.to_path_buf(),
        })?;
        self.platform.create_file_symlink(link, target)?;
        self.finish(run, entry)
    }

    // -- undo -------------------------------------------------------------

    /// Undoes a list of steps, newest first.
    fn undo_entries(&self, entries: &mut Vec<JournalEntry>, cancel: &CancelToken) -> Result<()> {
        while let Some(entry) = entries.pop() {
            self.undo_step(&entry, cancel)?;
            let reverted = JournalEntry { state: JournalState::Reverted, ..entry };
            self.store.update_journal(&reverted)?;
        }
        Ok(())
    }

    /// Undoes one step.
    ///
    /// Every branch reads the disk first, so the same code undoes a step that
    /// finished and a step that was interrupted halfway. That is what makes
    /// recovery after a crash possible: the journal says what was attempted,
    /// and the disk says how far it got.
    fn undo_step(&self, entry: &JournalEntry, cancel: &CancelToken) -> Result<()> {
        match &entry.step {
            JournalStep::CreateDir { path } => {
                // Only if still empty. A folder the person put something in
                // stays.
                fsops::remove_dir_if_empty(path)?;
                Ok(())
            }
            JournalStep::MoveToVault { from, to, sha256, .. } => {
                let source_there = std::fs::symlink_metadata(from).is_ok();
                let vault_there = to.is_file();
                match (source_there, vault_there) {
                    // The move happened. Put it back.
                    (false, true) => {
                        fsops::move_file(
                            self.platform, to, from, sha256, &self.store.temp_dir(), cancel,
                        )?;
                        Ok(())
                    }
                    // The move never happened, or a copy was interrupted before
                    // the source was removed. Either way the source is intact,
                    // so the vault copy is the thing to drop.
                    (true, true) => {
                        std::fs::remove_file(to).map_err(|e| {
                            VaultError::from_io(&e, to, "removing the copy made in the vault")
                        })?;
                        Ok(())
                    }
                    (true, false) => Ok(()),
                    (false, false) => Err(VaultError::new(
                        ErrorCode::IoError,
                        "A file could not be put back, because it is in neither place.",
                    )
                    .with_path(from)),
                }
            }
            JournalStep::StashOriginal { path, stash } => {
                if stash.as_os_str().is_empty() {
                    return Ok(());
                }
                if !stash.exists() {
                    return Ok(());
                }
                // The link may already sit in the original's place, from a
                // later step that has been undone or is about to be.
                if self.platform.is_symlink(path) {
                    self.platform.remove_symlink(path)?;
                }
                fsops::unstash(stash, path)
            }
            JournalStep::CreateLink { link, .. } => {
                if self.platform.is_symlink(link) {
                    self.platform.remove_symlink(link)?;
                }
                Ok(())
            }
            JournalStep::DeleteStash { stash, original, vault_path, sha256, .. } => {
                // The delete may not have happened.
                if stash.exists() {
                    return Ok(());
                }
                // The bytes are gone from this path, but the vault holds the
                // same content by hash, which is why removing them was safe.
                if self.platform.is_symlink(original) {
                    self.platform.remove_symlink(original)?;
                }
                if std::fs::symlink_metadata(original).is_ok() {
                    return Ok(());
                }
                fsops::restore_from_vault(vault_path, original, sha256, cancel)
            }
            JournalStep::RemoveLink { link, target } => {
                if std::fs::symlink_metadata(link).is_ok() {
                    return Ok(());
                }
                self.platform.create_file_symlink(link, target)
            }
        }
    }

    // -- revert and recovery ----------------------------------------------

    /// Undoes a whole apply run.
    pub fn revert(
        &self,
        apply_id: &str,
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ApplyProgress>,
    ) -> Result<ApplyRecord> {
        let mut record = self
            .store
            .apply(apply_id)?
            .ok_or_else(|| VaultError::not_found("That run is not in this vault's history."))?;

        if record.state == ApplyState::Reverted {
            return Err(VaultError::conflict("That run was already undone."));
        }

        let entries = self.store.journal(apply_id)?;
        let mut to_undo: Vec<JournalEntry> = entries
            .into_iter()
            .filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending))
            .collect();

        // Undoing needs room on the install's drive for every duplicate that
        // has to be copied back out of the vault. Checked first, so a revert
        // does not stop halfway for lack of space.
        self.check_revert_space(&to_undo)?;

        let total = to_undo.len() as u64;
        let clock = std::time::Instant::now();
        let throttle = Throttle::per_second(4);
        let mut undone = 0u64;

        while let Some(entry) = to_undo.pop() {
            cancel.check()?;
            self.undo_step(&entry, cancel)?;
            let reverted = JournalEntry { state: JournalState::Reverted, ..entry.clone() };
            self.store.update_journal(&reverted)?;
            self.forget_group_records(&entry)?;
            undone += 1;

            if throttle.ready() {
                sink.emit(&ApplyProgress {
                    apply_id: apply_id.to_string(),
                    phase: ApplyPhase::Applying,
                    group_index: undone,
                    group_total: total,
                    current_group_id: Some(entry.group_id.clone()),
                    current_path: None,
                    step: ApplyStep::Cleaning,
                    bytes_moved: 0,
                    bytes_to_move: 0,
                    bytes_freed: 0,
                    files_moved: 0,
                    links_created: 0,
                    failures: 0,
                    elapsed_ms: clock.elapsed().as_millis() as u64,
                    eta_ms: estimate_remaining_ms(clock.elapsed().as_millis() as u64, undone, total),
                });
            }
        }

        record.state = ApplyState::Reverted;
        record.finished_at = Some(Timestamp::now());
        record.revertible = false;
        self.store.put_apply(&record)?;

        sink.emit(&ApplyProgress {
            apply_id: apply_id.to_string(),
            phase: ApplyPhase::Finalizing,
            group_index: total,
            group_total: total,
            current_group_id: None,
            current_path: None,
            step: ApplyStep::Cleaning,
            bytes_moved: 0,
            bytes_to_move: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            failures: 0,
            elapsed_ms: clock.elapsed().as_millis() as u64,
            eta_ms: None,
        });
        Ok(record)
    }

    /// A revert copies removed duplicates back out of the vault, so it needs
    /// room. Better to refuse than to stop halfway.
    fn check_revert_space(&self, entries: &[JournalEntry]) -> Result<()> {
        let mut needed: std::collections::HashMap<crate::platform::VolumeId, u64> =
            std::collections::HashMap::new();

        for e in entries {
            if let JournalStep::DeleteStash { original, size_bytes, stash, .. } = &e.step {
                if stash.exists() {
                    continue;
                }
                if let Ok(v) = self.platform.volume_id(original) {
                    *needed.entry(v).or_insert(0) += size_bytes;
                }
            }
        }
        for e in entries {
            if let JournalStep::MoveToVault { from, size_bytes, copied, .. } = &e.step {
                if !copied {
                    continue;
                }
                if let Ok(v) = self.platform.volume_id(from) {
                    *needed.entry(v).or_insert(0) += size_bytes;
                }
            }
        }

        for e in entries {
            let path = match &e.step {
                JournalStep::DeleteStash { original, .. } => original,
                JournalStep::MoveToVault { from, .. } => from,
                _ => continue,
            };
            let Ok(volume) = self.platform.volume_id(path) else { continue };
            let Some(want) = needed.remove(&volume) else { continue };
            let free = self.platform.disk_space(path).map(|s| s.free_bytes).unwrap_or(u64::MAX);
            if want > free {
                return Err(VaultError::new(
                    ErrorCode::IoError,
                    "There is not enough room on that drive to put the files back. Free some space and try again.",
                )
                .with_detail(format!("needs {want} bytes, {free} free"))
                .with_path(path));
            }
        }
        Ok(())
    }

    /// Removes the database rows a reverted step created.
    fn forget_group_records(&self, entry: &JournalEntry) -> Result<()> {
        match &entry.step {
            JournalStep::CreateLink { link, .. } => {
                if let Some(record) = self.store.link_at_path(link)? {
                    self.store.delete_link(&record.id)?;
                }
                Ok(())
            }
            JournalStep::MoveToVault { sha256, .. } => {
                self.store.delete_vault_file(sha256)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Apply runs that stopped in the middle.
    pub fn interrupted(&self) -> Result<Vec<InterruptedApply>> {
        let mut out = Vec::new();
        for record in self.store.applies()? {
            if record.state != ApplyState::Running {
                continue;
            }
            let entries = self.store.journal(&record.apply_id)?;
            let done = entries.iter().filter(|e| e.state == JournalState::Done).count() as u64;
            let pending = entries.iter().filter(|e| e.state == JournalState::Pending).count() as u64;
            let affected: Vec<String> = entries
                .iter()
                .filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending))
                .filter_map(|e| step_path(&e.step))
                .map(|p| crate::paths::display_path(&p))
                .collect();

            out.push(InterruptedApply {
                description: format!(
                    "An earlier run stopped before it finished. It completed {done} steps and left {pending} unfinished. Nothing was lost: every file is either in its old place or in the vault."
                ),
                apply_id: record.apply_id.clone(),
                plan_id: record.plan_id.clone(),
                started_at: record.started_at,
                steps_done: done,
                steps_pending: pending,
                affected_paths: affected,
            });
        }
        Ok(out)
    }

    /// Finishes a run that stopped in the middle.
    ///
    /// The group that was in progress is undone first, so the disk is in a
    /// state the plan describes. Then the groups that were never started are
    /// applied, with every file checked again.
    pub fn resume(
        &self,
        apply_id: &str,
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ApplyProgress>,
    ) -> Result<ApplyRecord> {
        let record = self
            .store
            .apply(apply_id)?
            .ok_or_else(|| VaultError::not_found("That run is not in this vault's history."))?;
        let plan = self
            .store
            .plan(&record.plan_id)?
            .ok_or_else(|| VaultError::not_found("The plan for that run is no longer in this vault."))?;

        let entries = self.store.journal(apply_id)?;

        // A group is finished when its last step is done and nothing in it is
        // pending. Everything else is rolled back and applied again.
        let mut incomplete: Vec<JournalEntry> = Vec::new();
        let mut complete_groups: std::collections::HashSet<String> = Default::default();
        let mut groups_seen: Vec<String> = Vec::new();

        for e in &entries {
            if !groups_seen.contains(&e.group_id) {
                groups_seen.push(e.group_id.clone());
            }
        }
        for gid in &groups_seen {
            let mine: Vec<&JournalEntry> = entries.iter().filter(|e| &e.group_id == gid).collect();
            let any_pending = mine.iter().any(|e| e.state == JournalState::Pending);
            let finished = !any_pending && self.group_looks_finished(&plan, gid);
            if finished {
                complete_groups.insert(gid.clone());
            } else {
                incomplete.extend(
                    mine.into_iter()
                        .filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending))
                        .cloned(),
                );
            }
        }

        self.undo_entries(&mut incomplete, cancel)?;

        let remaining: Vec<String> = plan
            .groups
            .iter()
            .map(|g| g.group_id.clone())
            .filter(|id| !complete_groups.contains(id))
            .collect();

        self.apply(
            apply_id,
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: remaining,
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            cancel,
            sink,
        )
    }

    /// Is this group's work actually on the disk?
    fn group_looks_finished(&self, plan: &ConsolidationPlan, group_id: &str) -> bool {
        let Some(g) = plan.group(group_id) else { return false };
        let vault_path = self.store.vault_root().join(&g.vault_rel_path);
        if !vault_path.is_file() {
            return false;
        }
        if !self.platform.is_symlink(&g.source.abs_path) {
            return false;
        }
        g.links.iter().all(|l| self.platform.is_symlink(&l.abs_path))
    }

    // -- progress ---------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        sink: &dyn ProgressSink<ApplyProgress>,
        run: &mut Run,
        phase: ApplyPhase,
        index: u64,
        total: u64,
        group: Option<&PlanGroup>,
        step: ApplyStep,
        path: Option<&Path>,
        force: bool,
    ) {
        if force {
            run.throttle.force_next();
        }
        if !run.throttle.ready() {
            return;
        }
        let elapsed_ms = run.clock.elapsed().as_millis() as u64;
        sink.emit(&ApplyProgress {
            apply_id: run.apply_id.clone(),
            phase,
            group_index: index,
            group_total: total,
            current_group_id: group.map(|g| g.group_id.clone()),
            current_path: path.map(crate::paths::display_path),
            step,
            bytes_moved: run.bytes_moved,
            bytes_to_move: run.bytes_to_move,
            bytes_freed: run.bytes_freed,
            files_moved: run.files_moved,
            links_created: run.links_created,
            failures: run.failures.len() as u64,
            elapsed_ms,
            eta_ms: estimate_remaining_ms(elapsed_ms, run.bytes_moved, run.bytes_to_move),
        });
    }
}

fn short_id(apply_id: &str) -> String {
    apply_id.chars().filter(|c| c.is_alphanumeric()).take(8).collect()
}

fn step_path(step: &JournalStep) -> Option<PathBuf> {
    match step {
        JournalStep::CreateDir { path } => Some(path.clone()),
        JournalStep::MoveToVault { from, .. } => Some(from.clone()),
        JournalStep::StashOriginal { path, .. } => Some(path.clone()),
        JournalStep::CreateLink { link, .. } => Some(link.clone()),
        JournalStep::DeleteStash { original, .. } => Some(original.clone()),
        JournalStep::RemoveLink { link, .. } => Some(link.clone()),
    }
}

fn code_for(reason: BlockReason) -> ErrorCode {
    match reason {
        BlockReason::FileLocked => ErrorCode::FileLocked,
        BlockReason::FileChanged => ErrorCode::FileChanged,
        BlockReason::FileMissing => ErrorCode::NotFound,
        BlockReason::PermissionDenied => ErrorCode::PermissionDenied,
        BlockReason::SymlinkUnsupported => ErrorCode::SymlinkUnsupported,
        BlockReason::TargetExistsNotLink => ErrorCode::Conflict,
        _ => ErrorCode::IoError,
    }
}

fn block_reason_for(e: &VaultError) -> BlockReason {
    match e.code {
        ErrorCode::FileLocked => BlockReason::FileLocked,
        ErrorCode::FileChanged => BlockReason::FileChanged,
        ErrorCode::NotFound => BlockReason::FileMissing,
        ErrorCode::PermissionDenied => BlockReason::PermissionDenied,
        ErrorCode::SymlinkUnsupported => BlockReason::SymlinkUnsupported,
        ErrorCode::Conflict => BlockReason::TargetExistsNotLink,
        _ => BlockReason::ReadError,
    }
}

#[cfg(test)]
mod tests;
