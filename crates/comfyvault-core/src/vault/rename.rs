//! Changing the name the vault keeps a file under, and putting it back.
//!
//! A rename is more than one step: the file moves, the record changes, the
//! old name becomes a link beside the file, and every install link that names
//! the old place is pointed at the new one. A crash can land between any two.
//!
//! So a rename is not a list of steps to replay. Its journal records what it
//! is for, the move from one name to the other, before anything touches the
//! disk. [`Vault::settle_rename`] then brings the vault to one of the two
//! names from whatever state it finds, and doing it twice changes nothing.
//! The rename runs it towards the new name, its undo towards the old one, and
//! the next vault open runs it for a rename a crash cut off.
//!
//! Nothing is ever overwritten. A real file where a name has to go stops the
//! rename. An install link is never removed before its replacement exists: the
//! replacement is made beside it under a hidden name and renamed over it, so
//! the install holds a working link at every instant.

use std::path::{Path, PathBuf};

use super::{Vault, VaultFile, RENAME_JOURNAL_PREFIX};
use crate::error::{ErrorCode, Result, VaultError};
use crate::store::{JournalEntry, JournalState, JournalStep, VaultFileRecord};
use crate::time_util::Timestamp;

/// What a rename did.
#[derive(Debug, Clone)]
pub struct Renamed {
    pub file: VaultFile,
    /// Install links Windows would not point at the new name. Each still loads
    /// the model through the old name, which stays beside the file.
    pub not_repointed: Vec<NotRepointed>,
}

/// An install link a rename could not point at the new name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRepointed {
    pub install_id: String,
    pub path: PathBuf,
    pub message: String,
}

/// The two names of a rename, read from its journal.
struct Move {
    sha: String,
    old: PathBuf,
    new: PathBuf,
    entry: JournalEntry,
}

/// Where the settle leaves the vault.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Towards {
    NewName,
    OldName,
}

fn undo_marker(id: &str) -> String {
    format!("renameUndo:{id}")
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// The hidden name a replacement link is made under, beside the link it
/// replaces. It has no model extension, so ComfyUI never lists it, and it
/// carries the rename's id, so a crash's leftover is known for what it is.
pub(crate) fn temp_link(link: &Path, id: &str) -> PathBuf {
    let short: String = id.trim_start_matches(RENAME_JOURNAL_PREFIX).chars().take(12).collect();
    link.with_file_name(format!(".{}.{short}.cvtmp", file_name(link)))
}

impl<'a> Vault<'a> {
    /// A new id for a rename, so a caller can record it before the rename
    /// starts.
    pub fn new_rename_id() -> String {
        format!("{RENAME_JOURNAL_PREFIX}{}", uuid::Uuid::new_v4().simple())
    }

    /// Makes one of a content's names the real file.
    pub fn set_canonical_name(&self, sha256: &str, name: &str) -> Result<VaultFile> {
        self.rename_file(sha256, name, &Self::new_rename_id()).map(|r| r.file)
    }

    /// Makes `name`, one of the content's second names, the real file, under
    /// the journal `id`.
    ///
    /// The previous real name becomes a link beside it, and every install link
    /// that names the old place is pointed at the new one, so nothing resolves
    /// through two links in a row. If the rename cannot finish, it is put back
    /// and the error returned. [`Vault::undo_rename`] puts it back later.
    pub fn rename_file(&self, sha256: &str, name: &str, id: &str) -> Result<Renamed> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(VaultError::invalid("That is not a file hash."));
        };
        if !id.starts_with(RENAME_JOURNAL_PREFIX) || !self.store.journal(id)?.is_empty() {
            return Err(VaultError::invalid("That rename id is not a new one."));
        }
        let record = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;
        if record.canonical_name == name {
            let file = self.file(&sha)?.ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;
            return Ok(Renamed { file, not_repointed: Vec::new() });
        }
        if !record.aliases.iter().any(|a| a == name) {
            return Err(VaultError::invalid(
                "That name does not belong to this model. Choose one of the names it already has.",
            ));
        }
        let old = self.inside(&PathBuf::from(&record.category).join(&record.canonical_name))?;
        let new = self.inside(&PathBuf::from(&record.category).join(name))?;
        if !is_real_file(&old) {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "The vault no longer holds that file, so its name cannot be changed.",
            )
            .with_path(&old));
        }
        if std::fs::symlink_metadata(&new).is_ok() && !self.platform.is_symlink(&new) {
            return Err(VaultError::conflict("A different file already has that name in the vault.").with_path(&new));
        }

        // What the rename is for, written before anything moves.
        let now = Timestamp::now();
        let entry = JournalEntry {
            apply_id: id.to_string(),
            seq: 0,
            group_id: sha.clone(),
            step: JournalStep::MoveToVault {
                from: old.clone(),
                to: new.clone(),
                copied: false,
                sha256: sha.clone(),
                size_bytes: record.size_bytes,
            },
            state: JournalState::Pending,
            started_at: now,
            finished_at: None,
            error: None,
        };
        self.store.append_journal(&entry)?;
        let mv = Move { sha: sha.clone(), old, new, entry };

        match self.settle_rename(&mv, id, Towards::NewName) {
            Ok(not_repointed) => {
                let record = self.store.vault_file(&sha)?.ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;
                Ok(Renamed { file: self.decorate(&record)?, not_repointed })
            }
            Err(e) => {
                // Put back. If even that fails, the journal stays pending and
                // the next vault open finishes the rename.
                if self.settle_rename(&mv, id, Towards::OldName).is_ok() {
                    self.mark(&mv.entry, JournalState::Undone)?;
                }
                Err(e)
            }
        }
    }

    /// Puts back the name a [`Vault::rename_file`] took away. Doing it twice
    /// changes nothing.
    ///
    /// Refused, with nothing changed, when a real file now has the old name in
    /// the vault, or when the file was renamed again later: that later rename
    /// has to be put back first.
    pub fn undo_rename(&self, id: &str) -> Result<()> {
        let Some(mv) = self.move_of(id)? else { return Ok(()) };
        if mv.entry.state == JournalState::Undone {
            return Ok(());
        }
        self.check_undo_rename(id)?;
        let record = self.record_of(&mv)?;
        let (old_name, new_name) = (file_name(&mv.old), file_name(&mv.new));
        if record.canonical_name != old_name && record.canonical_name != new_name {
            return Err(VaultError::conflict(
                "That model's name in the vault changed again later. Undo the later change first.",
            ));
        }
        // Written first, so an undo a crash cuts off is finished when the
        // vault next opens.
        self.store.put_meta_flag(&undo_marker(id), true)?;
        self.settle_rename(&mv, id, Towards::OldName)?;
        self.mark(&mv.entry, JournalState::Undone)?;
        self.store.put_meta_flag(&undo_marker(id), false)?;
        Ok(())
    }

    /// The checks [`Vault::undo_rename`] makes before it changes anything,
    /// without changing anything.
    pub fn check_undo_rename(&self, id: &str) -> Result<()> {
        let Some(mv) = self.move_of(id)? else { return Ok(()) };
        if mv.entry.state == JournalState::Undone {
            return Ok(());
        }
        self.record_of(&mv)?;
        if std::fs::symlink_metadata(&mv.old).is_ok() && !self.platform.is_symlink(&mv.old) && !is_real_file(&mv.new) {
            // The file is back under its old name already.
            return Ok(());
        }
        if std::fs::symlink_metadata(&mv.old).is_ok() && !self.platform.is_symlink(&mv.old) {
            return Err(VaultError::conflict(
                "A different file now has the model's old name in the vault, so the name was not put back. Nothing was changed.",
            )
            .with_path(&mv.old));
        }
        Ok(())
    }

    /// Finishes the renames a crash cut off, when a vault opens: an undo that
    /// had started is finished, and a rename that had started is completed.
    ///
    /// A rename that cannot be settled now, for example because a file of the
    /// person's own sits where a name goes, is left pending and tried again at
    /// the next open, rather than stop the vault from opening.
    pub fn finish_interrupted_renames(&self) -> Result<u64> {
        let mut finished = 0;
        for id in self.store.journal_ids()? {
            if !id.starts_with(RENAME_JOURNAL_PREFIX) {
                continue;
            }
            let Some(mv) = self.move_of(&id)? else { continue };
            let undoing = self.store.meta_flag(&undo_marker(&id))?;
            let done = if undoing {
                self.settle_rename(&mv, &id, Towards::OldName).is_ok()
                    && self.mark(&mv.entry, JournalState::Undone).is_ok()
                    && self.store.put_meta_flag(&undo_marker(&id), false).is_ok()
            } else if mv.entry.state == JournalState::Pending {
                self.settle_rename(&mv, &id, Towards::NewName).is_ok()
            } else {
                continue;
            };
            if done {
                finished += 1;
            }
        }
        Ok(finished)
    }

    fn move_of(&self, id: &str) -> Result<Option<Move>> {
        if !id.starts_with(RENAME_JOURNAL_PREFIX) {
            return Err(VaultError::not_found("That name change is not in this vault's history."));
        }
        let entries = self.store.journal(id)?;
        if entries.is_empty() {
            return Err(VaultError::not_found("That name change is not in this vault's history."));
        }
        let found = entries.into_iter().find_map(|e| match &e.step {
            JournalStep::MoveToVault { from, to, sha256, .. } if e.state != JournalState::Failed => {
                Some((from.clone(), to.clone(), sha256.clone(), e))
            }
            _ => None,
        });
        let Some((old, new, sha, entry)) = found else { return Ok(None) };
        let prove = |p: &Path| {
            crate::paths::resolve_new_path_within(self.store.vault_root(), p).map_err(|e| {
                VaultError::new(
                    ErrorCode::PathOutsideBoundary,
                    "That name change names a place outside the vault, so nothing was changed.",
                )
                .with_detail(e.message)
            })
        };
        Ok(Some(Move { sha, old: prove(&old)?, new: prove(&new)?, entry }))
    }

    fn record_of(&self, mv: &Move) -> Result<VaultFileRecord> {
        self.store.vault_file(&mv.sha)?.ok_or_else(|| {
            VaultError::conflict("That model is no longer in the vault, so its old name cannot come back.")
        })
    }

    fn mark(&self, entry: &JournalEntry, state: JournalState) -> Result<()> {
        let mut e = entry.clone();
        e.state = state;
        e.finished_at = Some(Timestamp::now());
        self.store.update_journal(&e)
    }

    /// Brings the vault to one of a rename's two names, from whatever state it
    /// is in, and answers the install links it could not point there.
    ///
    /// In order: the file takes the name, the record says so, the other name
    /// is a link beside the file, and each install link that names the other
    /// place is pointed at the file. Each part is skipped when it is done.
    fn settle_rename(&self, mv: &Move, id: &str, towards: Towards) -> Result<Vec<NotRepointed>> {
        let (to, from) = match towards {
            Towards::NewName => (&mv.new, &mv.old),
            Towards::OldName => (&mv.old, &mv.new),
        };
        let mut record = self.record_of(mv)?;
        let (to_name, from_name) = (file_name(to), file_name(from));
        let to_rel = PathBuf::from(&record.category).join(&to_name);
        let from_rel = PathBuf::from(&record.category).join(&from_name);

        // 1. The file.
        if !is_real_file(to) {
            if !is_real_file(from) {
                return Err(VaultError::new(
                    ErrorCode::NotFound,
                    "The vault no longer holds that file under either name, so its name cannot change.",
                )
                .with_path(from));
            }
            match std::fs::symlink_metadata(to) {
                Err(_) => {}
                Ok(m) if m.file_type().is_symlink() => self.platform.remove_symlink(to)?,
                Ok(_) => {
                    return Err(VaultError::conflict(
                        "A different file already has that name in the vault, so it was not changed.",
                    )
                    .with_path(to))
                }
            }
            self.platform
                .rename(from, to)
                .map_err(|e| e.into_vault_error(from, "changing the name the vault keeps"))?;
        }

        // 2. The record.
        if record.canonical_name != to_name || !record.aliases.contains(&from_name) || record.aliases.contains(&to_name) {
            record.canonical_name = to_name.clone();
            record.aliases.retain(|a| a != &to_name);
            if !record.aliases.contains(&from_name) {
                record.aliases.push(from_name.clone());
            }
            record.aliases.sort();
            self.store.put_vault_file(&record)?;
        }

        // 3. The other name, as a link beside the file, so every link that
        // still names it keeps loading the model.
        match std::fs::symlink_metadata(from) {
            Err(_) => self.platform.create_file_symlink(from, to)?,
            Ok(m) if m.file_type().is_symlink() => {
                if !self.points_straight_at(from, to) {
                    self.replace_link(from, to, id)?;
                }
            }
            Ok(_) => {
                return Err(VaultError::conflict("A real file sits where the vault's other name goes.").with_path(from))
            }
        }
        // Every other second name, too, goes straight to the file. One left
        // pointing at a name would load nothing once that name is removed.
        for alias in record.aliases.iter().filter(|a| **a != from_name) {
            let Ok(path) = self.inside(&PathBuf::from(&record.category).join(alias)) else { continue };
            if self.platform.is_symlink(&path) && !self.points_straight_at(&path, to) {
                self.replace_link(&path, to, id)?;
            }
        }

        // 4. The install links.
        let from_path = self.store.vault_root().join(&from_rel);
        let mut not_repointed = Vec::new();
        for mut link in self.store.links_for_hash(&mv.sha)? {
            if link.vault_rel_path != from_rel {
                continue;
            }
            let _ = self.remove_temp(&link.abs_path, id);
            if !self.platform.is_symlink(&link.abs_path) {
                // On a drive that is unplugged, or removed by hand. Its record
                // keeps naming the other place, which stays, so it works
                // again when the drive comes back.
                continue;
            }
            let points_at = self.platform.read_symlink(&link.abs_path).ok().map(|t| place(&t));
            let already = points_at.as_ref() == Some(&place(to));
            if !already {
                if points_at.as_ref() != Some(&place(&from_path)) {
                    // Someone pointed it somewhere else. It is theirs now.
                    continue;
                }
                if let Err(e) = self.replace_link(&link.abs_path, to, id) {
                    not_repointed.push(NotRepointed {
                        install_id: link.install_id.clone(),
                        path: link.abs_path.clone(),
                        message: e.message,
                    });
                    continue;
                }
            }
            link.vault_rel_path = to_rel.clone();
            self.store.put_link(&link)?;
        }

        if towards == Towards::NewName && mv.entry.state == JournalState::Pending {
            self.mark(&mv.entry, JournalState::Done)?;
        }
        Ok(not_repointed)
    }

    /// Points the link at `link` at `target` without a moment where the link
    /// is missing: the new link is made beside it under a hidden name, then
    /// renamed over it.
    fn replace_link(&self, link: &Path, target: &Path, id: &str) -> Result<()> {
        let temp = temp_link(link, id);
        self.remove_temp(link, id)?;
        self.platform.create_file_symlink(&temp, target)?;
        // Looked at again right before: a rename over the place replaces
        // whatever is there, and a real file that took the link's place is
        // the person's.
        if !self.platform.is_symlink(link) {
            let _ = self.platform.remove_symlink(&temp);
            return Err(VaultError::conflict(
                "Something that is not a link took this link's place, so it was left as it is.",
            )
            .with_path(link));
        }
        if let Err(e) = self.platform.rename(&temp, link) {
            let _ = self.platform.remove_symlink(&temp);
            return Err(e.into_vault_error(link, "pointing the link at the vault file's new name"));
        }
        Ok(())
    }

    /// The link at `link` names `target` itself, not a name that leads there.
    fn points_straight_at(&self, link: &Path, target: &Path) -> bool {
        self.platform.read_symlink(link).map(|t| place(&t) == place(target)).unwrap_or(false)
    }

    /// Removes a hidden replacement link a crash left beside `link`. Only a
    /// link under the hidden name this rename uses.
    fn remove_temp(&self, link: &Path, id: &str) -> Result<()> {
        let temp = temp_link(link, id);
        if self.platform.is_symlink(&temp) {
            self.platform.remove_symlink(&temp)?;
        }
        Ok(())
    }
}

/// Where a path is, with its folder followed through every link and its
/// last part kept as written, in the form two such places compare by.
pub(super) fn place(p: &Path) -> String {
    let parent = p.parent().map(|d| crate::paths::canonicalize_existing_prefix(d).unwrap_or_else(|_| d.to_path_buf()));
    let whole = match parent {
        Some(d) => d.join(p.file_name().unwrap_or_default()),
        None => p.to_path_buf(),
    };
    crate::paths::compare_key(&whole)
}

fn is_real_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false)
}
