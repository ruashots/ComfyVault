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

pub mod boundary;
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

/// Which part of an undo is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevertPhase {
    Restoring,
    Finalizing,
}

/// What an undo is doing to the current path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevertAction {
    /// Removing a link the run made.
    RemovingLink,
    /// Renaming a file back to where it was. Instant, and takes no room.
    RenamingBack,
    /// Copying a file back out of the vault. Takes time and room.
    CopyingBack,
    /// Folders and other bookkeeping.
    Tidying,
}

/// A progress update for an undo.
///
/// Not [`ApplyProgress`]: files moved into the vault, links created and space
/// returned are the opposite of what an undo does, and reported as zeros they
/// told the person nothing for minutes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertProgress {
    pub apply_id: String,
    pub phase: RevertPhase,
    pub step_index: u64,
    pub step_total: u64,
    pub current_path: Option<String>,
    pub action: RevertAction,
    pub files_put_back: u64,
    pub files_to_put_back: u64,
    pub links_removed: u64,
    pub bytes_copied: u64,
    pub bytes_to_copy: u64,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
}

/// What undoing a run will cost, read before it starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertPreview {
    pub apply_id: String,
    /// Files an earlier undo of this run already put back, which is not zero
    /// only for a run that is partly undone.
    pub files_already_back: u64,
    /// Put back by a rename. Instant, and they take no room.
    pub files_renamed_back: u64,
    /// Put back by copying the vault file, because their own bytes were
    /// deleted when the run freed the room.
    pub files_copied_back: u64,
    /// The size of the files the copies write. This is what sets the time.
    pub bytes_to_copy: u64,
    pub drives: Vec<RevertDrive>,
}

/// The room an undo takes on one drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertDrive {
    pub volume: String,
    /// What the copies are expected to occupy on this drive. A sparse or
    /// compressed file is copied as one, so this can be far below the size of
    /// the files.
    pub predicted_room_bytes: u64,
    /// Read off the drive. Nothing when the drive did not answer.
    pub free_bytes: Option<u64>,
}

/// What undoing one journal step does, decided before it runs.
enum UndoAction {
    RemoveLink { counted: bool },
    RenameBack,
    CopyBack { bytes: u64, room: u64, place: PathBuf },
    Tidy,
}

impl UndoAction {
    fn puts_a_file_back(&self) -> bool {
        matches!(self, Self::RenameBack | Self::CopyBack { .. })
    }

    fn bytes_to_copy(&self) -> u64 {
        match self {
            Self::CopyBack { bytes, .. } => *bytes,
            _ => 0,
        }
    }

    fn progress_action(&self) -> RevertAction {
        match self {
            Self::RemoveLink { .. } => RevertAction::RemovingLink,
            Self::RenameBack => RevertAction::RenamingBack,
            Self::CopyBack { .. } => RevertAction::CopyingBack,
            Self::Tidy => RevertAction::Tidying,
        }
    }
}

/// Running state for one undo.
struct RevertRun {
    apply_id: String,
    clock: std::time::Instant,
    throttle: Throttle,
    step_index: u64,
    step_total: u64,
    current_path: Option<String>,
    action: RevertAction,
    files_put_back: u64,
    files_to_put_back: u64,
    links_removed: u64,
    bytes_copied: u64,
    bytes_to_copy: u64,
}

impl RevertRun {
    fn emit(&self, sink: &dyn ProgressSink<RevertProgress>, phase: RevertPhase, force: bool) {
        if force {
            self.throttle.force_next();
        }
        if !self.throttle.ready() {
            return;
        }
        let elapsed_ms = self.clock.elapsed().as_millis() as u64;
        // The copies are nearly all of an undo's time when there are any.
        let eta_ms = match phase {
            RevertPhase::Finalizing => None,
            RevertPhase::Restoring if self.bytes_to_copy > 0 => {
                estimate_remaining_ms(elapsed_ms, self.bytes_copied, self.bytes_to_copy)
            }
            RevertPhase::Restoring => estimate_remaining_ms(elapsed_ms, self.step_index, self.step_total),
        };
        sink.emit(&RevertProgress {
            apply_id: self.apply_id.clone(),
            phase,
            step_index: self.step_index,
            step_total: self.step_total,
            current_path: self.current_path.clone(),
            action: self.action,
            files_put_back: self.files_put_back,
            files_to_put_back: self.files_to_put_back,
            links_removed: self.links_removed,
            bytes_copied: self.bytes_copied,
            bytes_to_copy: self.bytes_to_copy,
            elapsed_ms,
            eta_ms,
        });
    }
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
    /// Read a duplicate again before deleting it, rather than trusting a hash
    /// from an earlier scan.
    verify_before_delete: bool,
    /// Where this run may touch the disk, read once when it starts.
    places: boundary::Places,
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
        self.apply_from(apply_id, plan, req, 0, None, cancel, sink)
    }

    /// Applies, continuing an earlier pass when one is given.
    ///
    /// `start_seq` continues the journal instead of restarting it. A resumed
    /// pass that began at zero overwrote the first pass's entries, and the
    /// journal is the only record of how to undo work that already happened.
    ///
    /// `carry` is the first pass's record. Its totals are added to rather than
    /// replaced, so the history says what really happened across both passes.
    #[allow(clippy::too_many_arguments)]
    fn apply_from(
        &self,
        apply_id: &str,
        plan: &ConsolidationPlan,
        req: &ApplyRequest,
        start_seq: u64,
        carry: Option<ApplyRecord>,
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
            seq: start_seq,
            clock: std::time::Instant::now(),
            throttle: Throttle::per_second(4),
            // Only the groups whose move is a real copy. A same-drive move is
            // a rename, so counting its bytes made the time remaining
            // pessimistic at the start and then jump.
            bytes_to_move: groups
                .iter()
                .filter(|g| g.cross_volume)
                .map(|g| g.size_bytes)
                .sum(),
            bytes_moved: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            groups_applied: 0,
            failures: Vec::new(),
            verify_before_delete: self.store.settings()?.verify_before_delete,
            places: boundary::Places::read(self.store)?,
        };

        // Read from the drive before anything moves. A resumed pass keeps the
        // first pass's reading, because "before" means before the work began,
        // not before this attempt at it.
        let free_before = match &carry {
            Some(first) => first.vault_free_bytes_before,
            None => self.vault_free_bytes(),
        };

        let mut record = match &carry {
            // A resumed pass keeps the first pass's identity and its numbers.
            Some(first) => ApplyRecord {
                state: ApplyState::Running,
                finished_at: None,
                ..first.clone()
            },
            None => ApplyRecord {
                apply_id: apply_id.to_string(),
                plan_id: plan.plan_id.clone(),
                state: ApplyState::Running,
                started_at: Timestamp::now(),
                finished_at: None,
                groups_requested: groups.len() as u64,
                group_ids: req.group_ids.clone(),
                groups_applied: 0,
                groups_failed: 0,
                bytes_freed: 0,
                files_moved: 0,
                links_created: 0,
                vault_free_bytes_before: free_before,
                vault_free_bytes_after: None,
                failures: Vec::new(),
                revertible: true,
                last_undo_step_at: None,
            },
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
                    // A group stopped because the person pressed cancel did
                    // not fail. Recording it as a failure tells them something
                    // went wrong when they are the one who stopped it.
                    if e.code == ErrorCode::Cancelled {
                        cancelled = true;
                        break;
                    }
                    let reason = block_reason_for(&e);
                    run.failures.push(ApplyFailure {
                        group_id: group.group_id.clone(),
                        abs_path: crate::paths::display_path(&group.source.abs_path),
                        reason,
                        detail: e.message.clone(),
                    });
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
        // Added to, not replaced. A resumed pass that overwrote these left the
        // history claiming one group and half the bytes for work that did two.
        let before = carry.as_ref();
        record.groups_applied = before.map(|c| c.groups_applied).unwrap_or(0) + run.groups_applied;
        record.groups_failed = run.failures.len() as u64;
        record.bytes_freed = before.map(|c| c.bytes_freed).unwrap_or(0) + run.bytes_freed;
        record.files_moved = before.map(|c| c.files_moved).unwrap_or(0) + run.files_moved;
        record.links_created = before.map(|c| c.links_created).unwrap_or(0) + run.links_created;
        record.failures = run.failures.clone();
        record.vault_free_bytes_before = free_before;
        // Read again, after the last file has moved and the last duplicate has
        // gone. Measured, never derived from the one above.
        record.vault_free_bytes_after = self.vault_free_bytes();
        record.revertible = record.groups_applied > 0;
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

        // The plan is read from the vault's database, which may not have been
        // written on this computer. Every place it names is proved first.
        let refused = run.places.refused_in_group(group);
        if !refused.is_empty() {
            return Err(boundary::refusal(&refused));
        }

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
                    if let Err(e) = fsops::ensure_dir(parent) {
                        return Err(self.failed(entry, e));
                    }
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
            let kind = match fsops::move_file(
                self.platform, &group.source.abs_path, &vault_path, &group.sha256, &temp_dir, cancel,
                &mut |_| {},
            ) {
                Ok(kind) => kind,
                // Refused before anything moved: something already sits at
                // the vault path. The step is closed as failed, so no undo
                // later takes it for a move that happened and deletes that
                // file, and no later run is told this run owns the path.
                Err(e) if e.code == ErrorCode::Conflict => return Err(self.failed(entry, e)),
                Err(e) => return Err(e),
            };
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
            if group.cross_volume {
                run.bytes_moved += group.size_bytes;
            }

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

                // Named in the journal before the rename, so a crash at any
                // point leaves the bytes where the journal says they are.
                let stash_path = fsops::stash_name(&link.abs_path, &short_id(&run.apply_id))?;
                let entry = self.step(run, group, JournalStep::StashOriginal {
                    path: link.abs_path.clone(),
                    stash: stash_path.clone(),
                })?;
                if let Err(e) = fsops::stash_to(&link.abs_path, &stash_path) {
                    return Err(self.failed(entry, e));
                }
                done.push(self.finish(run, entry)?);

                done.push(self.create_link_step(run, group, &link.abs_path, &vault_path)?);
                run.links_created += 1;

                self.emit(sink, run, ApplyPhase::Applying, index as u64, total as u64,
                    Some(group), ApplyStep::Cleaning, Some(&link.abs_path), false);

                // Written before the check below, which reads the whole file.
                // A crash during that read left no step for the delete, so a
                // recovery took the group for finished and the duplicate's
                // bytes stayed on disk for good under their set-aside name.
                let entry = self.step(run, group, JournalStep::DeleteStash {
                    stash: stash_path.clone(),
                    original: link.abs_path.clone(),
                    vault_path: vault_path.clone(),
                    sha256: group.sha256.clone(),
                    size_bytes: link.size_bytes,
                    mtime_nanos: Some(link.mtime_nanos),
                })?;
                // Proved, not assumed. This is the only irreversible step in
                // the whole run, and until now the proof that these bytes were
                // a duplicate was a hash from an earlier scan, which may have
                // come from a cache row rather than from the file itself.
                if run.verify_before_delete {
                    let actual = match crate::scan::hash::hash_file_cancellable(&stash_path, cancel) {
                        Ok(h) => h,
                        Err(e) => return Err(self.failed(entry, e)),
                    };
                    if !actual.eq_ignore_ascii_case(&group.sha256) {
                        return Err(self.failed(
                            entry,
                            VaultError::new(
                                ErrorCode::FileChanged,
                                "This file is not the same as the copy being kept, so it was not deleted. It has been put back.",
                            )
                            .with_detail(format!("expected {}, the file is {actual}", group.sha256))
                            .with_path(&link.abs_path),
                        ));
                    }
                }
                if let Err(e) = std::fs::remove_file(&stash_path) {
                    return Err(self.failed(entry, VaultError::from_io(&e, &stash_path, "removing the duplicate")));
                }
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
            // started. If the undo itself fails, the person needs both: what
            // went wrong, and the fact that putting it back also went wrong.
            // Reporting only the second leaves them without the cause.
            if let Err(undo_failed) = self.undo_entries(&mut done) {
                return Err(VaultError::new(
                    undo_failed.code,
                    format!("{} {}", e.message, undo_failed.message),
                )
                .with_detail(format!(
                    "what went wrong: {}; putting it back also failed: {}",
                    e.detail.clone().unwrap_or_else(|| e.message.clone()),
                    undo_failed.detail.unwrap_or(undo_failed.message)
                )));
            }
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

        // Merged, not replaced. A later apply touching a content the vault
        // already holds would otherwise drop names recorded earlier, while the
        // links for those names stay on disk: the vault screen stops showing a
        // name the model is still known by, and the health check calls that
        // link a stray file.
        let existing = self.store.vault_file(&group.sha256)?;
        let mut aliases = existing
            .as_ref()
            .map(|e| e.aliases.clone())
            .unwrap_or_default();
        for a in &group.vault_aliases {
            if !aliases.iter().any(|x| x.eq_ignore_ascii_case(a)) {
                aliases.push(a.clone());
            }
        }
        aliases.retain(|a| !a.eq_ignore_ascii_case(&canonical_name));
        aliases.sort();

        self.store.put_vault_file(&VaultFileRecord {
            sha256: group.sha256.clone(),
            canonical_name,
            category: group.category.clone(),
            size_bytes: group.size_bytes,
            added_at: existing.map(|e| e.added_at).unwrap_or_else(Timestamp::now),
            aliases,
        })?;

        let record_link = |install_id: &str, abs: &Path, name: &str| -> Result<()> {
            // Once per place. A resumed run records again the groups a crash
            // cut off between their last step and their records.
            if self.store.link_at_path(abs)?.is_some() {
                return Ok(());
            }
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
        if let Err(e) = self.platform.create_file_symlink(link, target) {
            return Err(self.failed(entry, e));
        }
        self.finish(run, entry)
    }

    /// Closes a step whose operation failed before it changed anything, and
    /// hands the error back.
    ///
    /// Left pending, such a step outlived its group: an undo later took it
    /// for a step that had happened, and a later run was told this run owned
    /// its path. If the journal cannot be written the step stays pending, and
    /// every undo reads the disk before it acts, so that is still safe.
    fn failed(&self, entry: JournalEntry, e: VaultError) -> VaultError {
        let closed = JournalEntry {
            state: JournalState::Failed,
            finished_at: Some(Timestamp::now()),
            error: Some(e.message.clone()),
            ..entry
        };
        let _ = self.store.update_journal(&closed);
        e
    }

    // -- undo -------------------------------------------------------------

    /// Undoes a list of steps, newest first.
    ///
    /// This is clean-up that must finish, so it never hears a Stop. It runs
    /// because a group went wrong or was stopped, and the Stop that got it here
    /// is still raised: passing it on stopped the clean-up at its first copy,
    /// and left a model half consolidated with no record of its links.
    fn undo_entries(&self, entries: &mut Vec<JournalEntry>) -> Result<()> {
        let refilled = paths_filled_later(entries);
        let never = CancelToken::new();
        while let Some(entry) = entries.pop() {
            self.undo_step(&entry, &refilled, &never, &mut |_| {})?;
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
    ///
    /// `copied` hears every byte a step copies, so a long copy can be shown.
    ///
    /// **A path the person's installs load from is never left empty.** A file
    /// comes back by one rename onto the link that stood in for it, so the
    /// path holds the link or the file at every instant, whether the undo
    /// finishes, is stopped, fails, or the power goes. `refilled` names the
    /// links a later step of the same undo replaces that way, and those are
    /// left in place rather than removed ahead of time.
    fn undo_step(
        &self,
        entry: &JournalEntry,
        refilled: &std::collections::HashSet<PathBuf>,
        cancel: &CancelToken,
        copied: &mut dyn FnMut(u64),
    ) -> Result<()> {
        match &entry.step {
            JournalStep::CreateDir { path } => {
                // Only if still empty. A folder the person put something in
                // stays.
                fsops::remove_dir_if_empty(path)?;
                Ok(())
            }
            JournalStep::MoveToVault { from, to, sha256, size_bytes, .. } => {
                // A real file, not merely something. The link that stands in
                // for the moved file is left there until the file replaces
                // it, and counting that link as "the source is intact" would
                // delete the only copy, the one in the vault.
                let source_there = std::fs::symlink_metadata(from)
                    .map(|m| m.file_type().is_file())
                    .unwrap_or(false);
                let vault_there = to.is_file();
                // The vault path holding *something* is not the same as it
                // holding the file this step put there. A later in-vault
                // rename leaves a link at that path, and treating that as "the
                // move happened" renamed the link into the person's model
                // folder and left them holding a link instead of their file.
                if vault_there && self.platform.is_symlink(to) {
                    return Err(VaultError::conflict(
                        "The file in the vault is not the one this step moved, so it was not put back. Undo the later change first.",
                    )
                    .with_path(to));
                }

                match (source_there, vault_there) {
                    // The move happened. Put it back, over its link.
                    (false, true) => {
                        fsops::move_back(self.platform, to, from, sha256, cancel, copied)?;
                        Ok(())
                    }
                    // A real file at both places. The vault file is dropped
                    // only when both hold this step's bytes, which is the one
                    // case this branch is for: a copy cut off before the
                    // source was removed.
                    //
                    // Anything else is not that. A different file at the
                    // source's place means the move happened and something
                    // replaced its link since, so the vault file is the only
                    // copy of the model. A different file at the vault path
                    // means the move never happened and that file is someone
                    // else's. Deleting on "some file is there" lost the only
                    // copy of a model and reported success.
                    (true, true) => {
                        if !holds(from, sha256, *size_bytes, cancel)? {
                            return Err(kept_copy_conflict(from, to));
                        }
                        if holds(to, sha256, *size_bytes, cancel)? {
                            std::fs::remove_file(to).map_err(|e| {
                                VaultError::from_io(&e, to, "removing the copy made in the vault")
                            })?;
                        }
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
                    // Written by an older build, which recorded the name only
                    // after the rename. If the place is empty, the bytes are
                    // under the name that build would have chosen.
                    if std::fs::symlink_metadata(path).is_ok() {
                        return Ok(());
                    }
                    let tried = fsops::stash_names_tried(path, &short_id(&entry.apply_id));
                    return match tried.last() {
                        Some(found) if boundary::is_stash_of(path, found) => fsops::unstash(found, path),
                        _ => Ok(()),
                    };
                }
                if !stash.exists() {
                    return Ok(());
                }
                // Renamed over the link, if one still stands there.
                fsops::unstash(stash, path)
            }
            JournalStep::CreateLink { link, .. } => {
                if refilled.contains(link) {
                    return Ok(());
                }
                if self.platform.is_symlink(link) {
                    self.platform.remove_symlink(link)?;
                }
                Ok(())
            }
            JournalStep::DeleteStash { stash, original, vault_path, sha256, mtime_nanos, .. } => {
                // The delete may not have happened.
                if stash.exists() {
                    return Ok(());
                }
                // Already a real file: put back by an earlier attempt.
                if std::fs::symlink_metadata(original).map(|m| m.file_type().is_file()).unwrap_or(false) {
                    return Ok(());
                }
                // The bytes are gone from this path, but the vault holds the
                // same content by hash, which is why removing them was safe.
                // The copy replaces the link in one rename.
                fsops::restore_from_vault(vault_path, original, sha256, *mtime_nanos, cancel, copied)
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
        sink: &dyn ProgressSink<RevertProgress>,
    ) -> Result<ApplyRecord> {
        let (mut record, mut to_undo) = self.revertible_steps(apply_id)?;
        let refilled = paths_filled_later(&to_undo);
        self.remove_leftovers(&to_undo);

        // Read before anything moves: what each step will cost depends on the
        // disk as the run left it.
        let mut actions: Vec<UndoAction> = to_undo.iter().map(|e| self.undo_action(e)).collect();

        // Undoing needs room on the install's drive for every duplicate that
        // has to be copied back out of the vault. Checked first, so a revert
        // does not stop halfway for lack of space.
        self.check_revert_space(&actions)?;

        // Written before the first file moves, so an undo that is stopped,
        // fails, or dies with the power is still on record as partly done.
        record.state = ApplyState::PartlyReverted;
        self.store.put_apply(&record)?;

        let mut run = RevertRun {
            apply_id: apply_id.to_string(),
            clock: std::time::Instant::now(),
            throttle: Throttle::per_second(4),
            step_index: 0,
            step_total: to_undo.len() as u64,
            current_path: None,
            action: RevertAction::Tidying,
            files_put_back: 0,
            files_to_put_back: actions.iter().filter(|a| a.puts_a_file_back()).count() as u64,
            links_removed: 0,
            bytes_copied: 0,
            bytes_to_copy: actions.iter().map(UndoAction::bytes_to_copy).sum(),
        };

        while let (Some(entry), Some(action)) = (to_undo.pop(), actions.pop()) {
            cancel.check()?;
            run.action = action.progress_action();
            run.current_path = step_path(&entry.step).map(|p| crate::paths::display_path(&p));
            // Not forced: a run of small steps would break the four a second
            // the contract promises. A long copy reports within a quarter of
            // a second anyway, from inside the copy.
            run.emit(sink, RevertPhase::Restoring, false);

            // Before the step, so no step reaches the disk unrecorded.
            record.last_undo_step_at = Some(Timestamp::now());
            self.store.put_apply(&record)?;

            self.undo_step(&entry, &refilled, cancel, &mut |n| {
                run.bytes_copied += n;
                run.emit(sink, RevertPhase::Restoring, false);
            })?;
            let undone = JournalEntry { state: JournalState::Undone, ..entry.clone() };
            self.store.update_journal(&undone)?;
            self.forget_group_records(&entry)?;

            run.step_index += 1;
            if action.puts_a_file_back() {
                run.files_put_back += 1;
            }
            if let UndoAction::RemoveLink { counted: true } = action {
                run.links_removed += 1;
            }
            run.emit(sink, RevertPhase::Restoring, false);
        }

        record.state = ApplyState::Reverted;
        record.finished_at = Some(Timestamp::now());
        record.revertible = false;
        self.store.put_apply(&record)?;

        run.current_path = None;
        run.action = RevertAction::Tidying;
        run.emit(sink, RevertPhase::Finalizing, true);
        Ok(record)
    }

    /// What undoing a run would cost, without doing it.
    ///
    /// Refuses exactly when [`Applier::revert`] would refuse before starting,
    /// so the question can be answered before the person commits to it.
    pub fn preview_revert(&self, apply_id: &str) -> Result<RevertPreview> {
        let (_, to_undo) = self.revertible_steps(apply_id)?;
        let actions: Vec<UndoAction> = to_undo.iter().map(|e| self.undo_action(e)).collect();

        let mut drives: Vec<RevertDrive> = Vec::new();
        for (volume, room, place) in self.room_needed(&actions) {
            drives.push(RevertDrive {
                volume: volume.0,
                predicted_room_bytes: room,
                free_bytes: self.platform.disk_space(&place).ok().map(|s| s.free_bytes),
            });
        }
        drives.sort_by(|a, b| a.volume.cmp(&b.volume));

        // Read from the journal, which an undo marks step by step, so the
        // count survives the app closing. A place counts once it holds a real
        // file again, whichever step put it there.
        let already_back: std::collections::HashSet<PathBuf> = self
            .store
            .journal(apply_id)?
            .into_iter()
            .filter(|e| e.state == JournalState::Undone)
            .filter_map(|e| match e.step {
                JournalStep::MoveToVault { from, .. } => Some(from),
                JournalStep::DeleteStash { original, .. } => Some(original),
                JournalStep::StashOriginal { path, .. } => Some(path),
                _ => None,
            })
            .filter(|p| std::fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false))
            .collect();

        Ok(RevertPreview {
            apply_id: apply_id.to_string(),
            files_already_back: already_back.len() as u64,
            files_renamed_back: actions.iter().filter(|a| matches!(a, UndoAction::RenameBack)).count() as u64,
            files_copied_back: actions.iter().filter(|a| matches!(a, UndoAction::CopyBack { .. })).count() as u64,
            bytes_to_copy: actions.iter().map(UndoAction::bytes_to_copy).sum(),
            drives,
        })
    }

    /// The steps a revert undoes, after the refusals that apply before any
    /// file is touched.
    fn revertible_steps(&self, apply_id: &str) -> Result<(ApplyRecord, Vec<JournalEntry>)> {
        let record = self
            .store
            .apply(apply_id)?
            .ok_or_else(|| VaultError::not_found("That run is not in this vault's history."))?;

        if record.state == ApplyState::Reverted {
            return Err(VaultError::conflict("That run was already undone."));
        }

        let to_undo: Vec<JournalEntry> = self
            .store
            .journal(apply_id)?
            .into_iter()
            .filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending))
            .collect();

        // Every path comes from the vault's database. Proved before anything
        // else, so a run naming a place outside the vault and the installs is
        // refused whole, with nothing touched.
        let refused = boundary::Places::read(self.store)?.refused_in_steps(&to_undo);
        if !refused.is_empty() {
            return Err(boundary::refusal(&refused));
        }

        // A different file where a model was consolidated from stops the
        // undo before it starts, so the preview says so and nothing is half
        // undone around it. Only a real file there is read; a link costs
        // nothing.
        for e in &to_undo {
            if let JournalStep::MoveToVault { from, to, sha256, size_bytes, .. } = &e.step {
                let real_file = std::fs::symlink_metadata(from).map(|m| m.file_type().is_file()).unwrap_or(false);
                if real_file && to.is_file() && !holds(from, sha256, *size_bytes, &CancelToken::new())? {
                    return Err(kept_copy_conflict(from, to));
                }
            }
        }

        // The contract promises this and nothing implemented it. Renaming a
        // model in the vault, or a later run touching the same files, leaves
        // this run's steps describing a world that no longer exists. Undoing
        // them anyway reported success while leaving the tree consolidated,
        // and left a live link the database knew nothing about.
        self.check_nothing_later_depends_on(apply_id, &to_undo)?;
        Ok((record, to_undo))
    }

    /// What undoing one step will do, read off the disk as it is now.
    ///
    /// A file can only come back by rename when its own bytes still exist
    /// somewhere. The kept copy's bytes are the vault file, so it is renamed
    /// back. A duplicate's bytes were deleted, which is what freed the room,
    /// so it can only come back as a copy of the vault file. One file cannot
    /// be renamed into two places.
    fn undo_action(&self, entry: &JournalEntry) -> UndoAction {
        let is_real_file = |p: &Path| {
            std::fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false)
        };
        let copy_of = |vault_file: &Path, size_bytes: u64, place: &Path| UndoAction::CopyBack {
            bytes: size_bytes,
            // A sparse or compressed vault file is copied as one, so the copy
            // takes what the vault file takes, not what its size says.
            room: crate::platform::size_on_disk(vault_file).unwrap_or(size_bytes),
            place: place.to_path_buf(),
        };
        match &entry.step {
            JournalStep::CreateDir { .. } | JournalStep::RemoveLink { .. } => UndoAction::Tidy,
            JournalStep::MoveToVault { from, to, copied, size_bytes, .. } => {
                if is_real_file(from) {
                    // The move never happened. Undoing it only drops a copy.
                    UndoAction::Tidy
                } else if *copied {
                    copy_of(to, *size_bytes, from)
                } else {
                    UndoAction::RenameBack
                }
            }
            JournalStep::StashOriginal { stash, .. } => {
                if !stash.as_os_str().is_empty() && stash.exists() {
                    UndoAction::RenameBack
                } else {
                    UndoAction::Tidy
                }
            }
            JournalStep::CreateLink { link, .. } => UndoAction::RemoveLink {
                // The names a vault file carries are links too, but they sit
                // inside the vault. The person counts the links in their
                // installs, which is what apply counted as it made them.
                counted: !link.starts_with(self.store.vault_root()),
            },
            JournalStep::DeleteStash { stash, original, vault_path, size_bytes, .. } => {
                if stash.exists() {
                    UndoAction::Tidy
                } else {
                    copy_of(vault_path, *size_bytes, original)
                }
            }
        }
    }

    /// The room the copies take on each drive, with one path on that drive to
    /// read its free space from.
    fn room_needed(&self, actions: &[UndoAction]) -> Vec<(crate::platform::VolumeId, u64, PathBuf)> {
        let mut needed: Vec<(crate::platform::VolumeId, u64, PathBuf)> = Vec::new();
        for a in actions {
            let UndoAction::CopyBack { room, place, .. } = a else { continue };
            let Ok(volume) = self.platform.volume_id(place) else { continue };
            match needed.iter_mut().find(|(v, _, _)| *v == volume) {
                Some((_, total, _)) => *total += room,
                None => needed.push((volume, *room, place.clone())),
            }
        }
        needed
    }

    /// Refuses a revert when a later operation touched the paths this run
    /// created.
    fn check_nothing_later_depends_on(
        &self,
        apply_id: &str,
        to_undo: &[JournalEntry],
    ) -> Result<()> {
        let mine: std::collections::HashSet<PathBuf> = to_undo
            .iter()
            .flat_map(|e| match &e.step {
                JournalStep::MoveToVault { to, .. } => vec![to.clone()],
                JournalStep::CreateLink { link, target } => vec![link.clone(), target.clone()],
                JournalStep::DeleteStash { vault_path, .. } => vec![vault_path.clone()],
                _ => Vec::new(),
            })
            .collect();

        let started = self
            .store
            .apply(apply_id)?
            .map(|r| r.started_at)
            .unwrap_or_default();

        let mut conflicts: Vec<String> = Vec::new();
        for other in self.store.journal_ids()? {
            if other == apply_id {
                continue;
            }
            // A later apply, or any in-vault rename, which has no apply record.
            let later = match self.store.apply(&other)? {
                Some(r) => r.started_at >= started && r.state != ApplyState::Reverted,
                None => true,
            };
            if !later {
                continue;
            }
            for e in self.store.journal(&other)? {
                if matches!(e.state, JournalState::Reverted | JournalState::Undone | JournalState::Failed) {
                    continue;
                }
                for p in step_paths_touched(&e.step) {
                    if mine.contains(&p) {
                        conflicts.push(crate::paths::display_path(&p));
                    }
                }
            }
        }

        // A link made by hand writes no journal entry, so the journals alone
        // cannot see it. Undoing a run while a third install links to a vault
        // file it created leaves that install holding a link to nothing, which
        // is the worst state this product can produce: ComfyUI lists the model
        // and then fails to load it, and a node re-downloading the "missing"
        // model writes straight through the dead link.
        let vault_root = self.store.vault_root();
        for link in self.store.links()? {
            if link.apply_id.as_deref() == Some(apply_id) {
                continue;
            }
            let target = vault_root.join(&link.vault_rel_path);
            if mine.contains(&target) {
                conflicts.push(crate::paths::display_path(&link.abs_path));
            }
        }

        if conflicts.is_empty() {
            return Ok(());
        }
        conflicts.sort();
        conflicts.dedup();
        Err(VaultError::conflict(
            "Something done after this run still uses these files, so it was not undone. Undo the later change first.",
        )
        .with_detail(conflicts.join(", ")))
    }

    /// A revert copies removed duplicates back out of the vault, so it needs
    /// room. Better to refuse than to stop halfway.
    fn check_revert_space(&self, actions: &[UndoAction]) -> Result<()> {
        for (_, want, place) in self.room_needed(actions) {
            let free = self.platform.disk_space(&place).map(|s| s.free_bytes).unwrap_or(u64::MAX);
            if want > free {
                return Err(VaultError::new(
                    ErrorCode::IoError,
                    "There is not enough room on that drive to put the files back. Free some space and try again.",
                )
                .with_detail(format!("needs {want} bytes, {free} free"))
                .with_path(&place));
            }
        }
        Ok(())
    }

    /// Removes the database rows a reverted step created.
    fn forget_group_records(&self, entry: &JournalEntry) -> Result<()> {
        match &entry.step {
            // A link record goes when its link is gone from the disk, not
            // before: a link left for a later step to replace is still one a
            // partly undone run's installs load through.
            JournalStep::CreateLink { link: path, .. }
            | JournalStep::StashOriginal { path, .. }
            | JournalStep::DeleteStash { original: path, .. } => self.forget_link_if_gone(path),
            JournalStep::MoveToVault { from, sha256, .. } => {
                self.forget_link_if_gone(from)?;
                self.store.delete_vault_file(sha256)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Removes the half-written copies an earlier run or undo of these steps
    /// left when its process ended in the middle of a copy. Only beside the
    /// places the steps name, which were proved before this is called.
    fn remove_leftovers(&self, entries: &[JournalEntry]) {
        let mut dirs: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        dirs.insert(self.store.temp_dir());
        for e in entries {
            for p in step_paths_touched(&e.step).into_iter().chain(step_path(&e.step)) {
                if let Some(parent) = p.parent() {
                    dirs.insert(parent.to_path_buf());
                }
            }
        }
        for d in dirs {
            fsops::remove_leftovers(&d);
        }
    }

    fn forget_link_if_gone(&self, path: &Path) -> Result<()> {
        if self.platform.is_symlink(path) {
            return Ok(());
        }
        if let Some(record) = self.store.link_at_path(path)? {
            self.store.delete_link(&record.id)?;
        }
        Ok(())
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
        // Only a run cut off in the middle is finished. Resuming a run the
        // person undid applied the whole of it again: files moved back into
        // the vault and duplicates deleted, for a run they had taken back.
        if record.state != ApplyState::Running {
            return Err(VaultError::conflict(
                "Only a run that was cut off part way can be finished. This one is not.",
            ));
        }
        let plan = self
            .store
            .plan(&record.plan_id)?
            .ok_or_else(|| VaultError::not_found("The plan for that run is no longer in this vault."))?;

        let entries = self.store.journal(apply_id)?;

        // The journal and the plan are read from the vault's database. Every
        // place they name is proved before a file is touched.
        let places = boundary::Places::read(self.store)?;
        let mut refused = places.refused_in_steps(
            entries.iter().filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending)),
        );
        for gid in &record.group_ids {
            if let Some(g) = plan.group(gid) {
                refused.extend(places.refused_in_group(g));
            }
        }
        if !refused.is_empty() {
            refused.sort();
            refused.dedup();
            return Err(boundary::refusal(&refused));
        }
        let proved: Vec<JournalEntry> = entries
            .iter()
            .filter(|e| matches!(e.state, JournalState::Done | JournalState::Pending))
            .cloned()
            .collect();
        self.remove_leftovers(&proved);

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
            // Every duplicate's delete must be on record as done. Links at
            // every place are not enough: a crash after the links and before
            // a delete leaves the duplicate's bytes under their set-aside
            // name, and calling that group finished hid them for good.
            let deletes_done = plan.group(gid).is_some_and(|g| {
                g.links.iter().filter(|l| !l.is_source).all(|l| {
                    mine.iter().any(|e| {
                        e.state == JournalState::Done
                            && matches!(&e.step, JournalStep::DeleteStash { original, .. } if *original == l.abs_path)
                    })
                })
            });
            let finished = !any_pending && deletes_done && self.group_looks_finished(&plan, gid);
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

        self.undo_entries(&mut incomplete)?;

        // The groups finished before the cut are part of this run, but the
        // record written when the run began holds none of them: its totals
        // are written only when a pass ends. Counted from the plan, the way
        // the pass that did them counts, so the finished run describes all of
        // itself rather than only what was done after the cut. Their records
        // are written too, in case the cut fell between a group's last step
        // and its records.
        let installs = self.store.installs()?;
        let mut carry = ApplyRecord {
            groups_applied: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            ..record.clone()
        };
        for gid in &record.group_ids {
            if !complete_groups.contains(gid) {
                continue;
            }
            let Some(g) = plan.group(gid) else { continue };
            let vault_path = self.store.vault_root().join(&g.vault_rel_path);
            self.record_group(g, &vault_path, &installs, apply_id)?;
            carry.groups_applied += 1;
            carry.files_moved += 1;
            carry.links_created += g.links.len() as u64;
            carry.bytes_freed += g.links.iter().filter(|l| !l.is_source).map(|l| l.size_bytes).sum::<u64>();
        }

        // Only what the person originally ticked, and only what is not done.
        //
        // Taking every group in the plan here was the worst defect in the
        // engine: tick three of five hundred, crash, press the recovery
        // button, and all five hundred move into the vault and every duplicate
        // is deleted. A record from an older build carries no selection, and
        // then nothing is resumed, which is the safe direction.
        let remaining: Vec<String> = record
            .group_ids
            .iter()
            .filter(|id| !complete_groups.contains(*id))
            .filter(|id| plan.group(id).is_some())
            .cloned()
            .collect();

        // Continue the journal where the first pass stopped. Restarting at
        // zero overwrote the entries describing work that had already
        // happened, which is the only record of how to undo it.
        let next_seq = entries.iter().map(|e| e.seq).max().map(|m| m + 1).unwrap_or(0);

        self.apply_from(
            apply_id,
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: remaining,
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            next_seq,
            Some(carry),
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
    /// Free space on the vault's drive right now, or nothing if it cannot be
    /// read. A failure here must not stop a run that has already moved files.
    fn vault_free_bytes(&self) -> Option<u64> {
        self.platform.disk_space(self.store.vault_root()).ok().map(|s| s.free_bytes)
    }

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

/// Does the real file at `path` hold these bytes? The size is compared first,
/// so a file that differs costs no read.
fn holds(path: &Path, sha256: &str, size_bytes: u64, cancel: &CancelToken) -> Result<bool> {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return Ok(false) };
    if !meta.file_type().is_file() || meta.len() != size_bytes {
        return Ok(false);
    }
    Ok(crate::scan::hash::hash_file_cancellable(path, cancel)?.eq_ignore_ascii_case(sha256))
}

/// The refusal when a different file sits where a model was consolidated from.
fn kept_copy_conflict(place: &Path, vault_file: &Path) -> VaultError {
    VaultError::conflict(
        "A different file now sits where this model was, so the undo stopped. The model is kept in the vault. Move that file somewhere else, then undo again.",
    )
    .with_detail(format!(
        "the file at {} is not this model; the model is at {}",
        crate::paths::display_path(place),
        crate::paths::display_path(vault_file)
    ))
    .with_path(place)
}

/// The paths a later step of an undo fills again with a file, by renaming it
/// over the link there. The link at such a path must stay until then.
///
/// Read once, before anything moves, from the steps to undo.
fn paths_filled_later(entries: &[JournalEntry]) -> std::collections::HashSet<PathBuf> {
    entries
        .iter()
        .filter_map(|e| match &e.step {
            JournalStep::MoveToVault { from, .. } => Some(from.clone()),
            JournalStep::StashOriginal { path, stash }
                if !stash.as_os_str().is_empty() && stash.exists() =>
            {
                Some(path.clone())
            }
            _ => None,
        })
        .collect()
}

fn short_id(apply_id: &str) -> String {
    apply_id.chars().filter(|c| c.is_alphanumeric()).take(8).collect()
}

/// Every path a step created or acted on, for the dependency check.
///
/// A link step gives both ends. The link itself matters because a later run
/// may have replaced it, and the target matters because a later link pointing
/// at a vault file this run created is a reason not to undo the run.
fn step_paths_touched(step: &JournalStep) -> Vec<PathBuf> {
    match step {
        JournalStep::MoveToVault { to, .. } => vec![to.clone()],
        JournalStep::CreateLink { link, target } => vec![link.clone(), target.clone()],
        JournalStep::RemoveLink { link, target } => vec![link.clone(), target.clone()],
        _ => Vec::new(),
    }
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
        ErrorCode::PathOutsideBoundary => BlockReason::UnsafeVaultPath,
        ErrorCode::Conflict => BlockReason::TargetExistsNotLink,
        _ => BlockReason::ReadError,
    }
}

#[cfg(test)]
mod tests;
