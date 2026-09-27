//! Giving a model one name in every install.
//!
//! A model that reached two installs under two names is one file in the vault
//! with two names in the installs. A saved workflow asks for a model by the
//! name its install gives it, so the two names are two different models to
//! the person. This module renames the links so every install uses one name.
//!
//! # What never happens
//!
//! * **An install never loses the model.** The new link is made before the old
//!   one is removed, so at every instant the install holds at least one link
//!   that loads the model.
//! * **Nothing is overwritten.** A place that already holds anything other
//!   than this model's own recorded link is left alone, and the link that
//!   would have gone there keeps its name.
//! * **A refusal from Windows stops the job where it is.** Removing the old
//!   link can fail, for example while a program holds it. The new link then
//!   stays, the job stops, and the model loads under both names. Running the
//!   job again finishes it, and its undo puts the old names back.
//!
//! Every step is journaled before it touches the disk, under an id that
//! starts with [`UNIFY_JOURNAL_PREFIX`]. A crash in the middle is finished
//! when the vault next opens.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apply::boundary::{self, Places};
use crate::error::{ErrorCode, Result, VaultError};
use crate::install::Install;
use crate::links::Links;
use crate::platform::Platform;
use crate::store::{
    JournalEntry, JournalState, JournalStep, LinkOrigin, LinkRecord, LinkState, Store, VaultFileRecord,
};
use crate::time_util::Timestamp;
use crate::usage::UsageResult;
use crate::vault::{name_key, Vault};

/// How the journal of a "use one name everywhere" job is named.
pub const UNIFY_JOURNAL_PREFIX: &str = "unify-";

/// What the job does to one link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnifyAction {
    /// The link already has the name.
    Keep,
    /// The link gets the name. The place for it is free.
    Rename,
    /// The same folder already has this model's link under the name, so this
    /// link goes.
    Remove,
    /// Something else already has the name in that folder. The link keeps
    /// its name.
    BlockedTaken,
    /// The link is recorded but not on the disk now, for example on a drive
    /// that is unplugged. It keeps its name, and the vault keeps the place it
    /// names, so it loads the model again when the drive is back.
    Unreachable,
}

/// One link of the model, and what the job does to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyLink {
    pub install_id: String,
    pub abs_path: PathBuf,
    pub link_name: String,
    pub action: UnifyAction,
    /// Where the link goes, for `rename`.
    pub new_abs_path: Option<PathBuf>,
    /// What already has the name, for `blockedTaken`.
    pub taken_by: Option<String>,
}

/// What giving the model one name would do. Reading it changes nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyPlan {
    pub sha256: String,
    /// The chosen name, spelled as the installs spell it.
    pub name: String,
    pub links: Vec<UnifyLink>,
    /// The installs whose links change and whose ComfyUI is running now. The
    /// job refuses to start while this is not empty.
    pub running: Vec<String>,
    /// The saved-workflow search for each name that goes away.
    pub workflows: Vec<UsageResult>,
}

/// A link that now carries the chosen name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedLink {
    pub install_id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// A link removed because the same folder already had the model under the
/// chosen name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedLink {
    pub install_id: String,
    pub path: PathBuf,
}

/// A link the job left as it was, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedLink {
    pub install_id: String,
    pub path: PathBuf,
    pub reason: String,
}

/// Where the job stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyStop {
    /// `None` when it stopped in the vault, after every link was done.
    pub install_id: Option<String>,
    pub path: PathBuf,
    pub message: String,
}

/// What the job did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyResult {
    /// What `undo_unify_name` takes.
    pub unify_id: String,
    pub name: String,
    pub renamed: Vec<RenamedLink>,
    pub removed: Vec<RemovedLink>,
    pub skipped: Vec<SkippedLink>,
    pub stopped: Option<UnifyStop>,
    /// The name the vault keeps the file under when the job ends.
    pub vault_name: String,
}

/// The reply to an undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifyUndone {
    pub undone: bool,
}

/// A job's own record, kept beside its journal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyJob {
    pub unify_id: String,
    pub sha256: String,
    pub name: String,
    pub started_at: Timestamp,
    /// The records of the links the job removed, written before each one is
    /// removed. The undo puts them back as they were.
    pub replaced: Vec<LinkRecord>,
    /// The journal of the vault file's rename, when the job renamed it.
    #[serde(default)]
    pub vault_rename: Option<String>,
    /// A name the job added to the vault's names so the file could take it.
    #[serde(default)]
    pub added_alias: Option<String>,
    /// The vault's second names the job removed because no link used them.
    #[serde(default)]
    pub removed_aliases: Vec<String>,
}

/// The model, its file in the vault, and its links that can be renamed.
struct Target {
    record: VaultFileRecord,
    vault_path: PathBuf,
    /// The chosen name, spelled as the first link that carries it.
    name: String,
    links: Vec<LinkRecord>,
    /// Recorded links of the model that are not on the disk now.
    unreachable: Vec<LinkRecord>,
}

pub struct Unify<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
}

impl<'a> Unify<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform) -> Self {
        Self { store, platform }
    }

    /// What giving the model the name `name` in every install would do.
    pub fn plan(&self, sha256: &str, name: &str) -> Result<UnifyPlan> {
        let t = self.target(sha256, name)?;
        let links = self.classify(&t)?;
        let installs = self.installs_of(&t)?;

        let mut going: Vec<String> = Vec::new();
        for l in links.iter().filter(|l| changes(l.action)) {
            if !going.iter().any(|g| name_key(g) == name_key(&l.link_name)) {
                going.push(l.link_name.clone());
            }
        }
        let workflows = if going.is_empty() {
            Vec::new()
        } else {
            crate::usage::check(&installs, &going)?.results
        };

        Ok(UnifyPlan {
            sha256: t.record.sha256.clone(),
            running: self.running(&links)?,
            name: t.name,
            links,
            workflows,
        })
    }

    /// Gives the model the name `name` in every install where that is
    /// possible, and then in the vault.
    pub fn unify(&self, sha256: &str, name: &str) -> Result<UnifyResult> {
        let t = self.target(sha256, name)?;
        let plan = self.classify(&t)?;
        let running = self.running(&plan)?;
        if !running.is_empty() {
            let labels: Vec<String> = running
                .iter()
                .map(|id| self.store.install(id).ok().flatten().map(|i| i.label).unwrap_or_else(|| id.clone()))
                .collect();
            return Err(VaultError::conflict(format!("Close {} first", labels.join(" and "))).with_detail(
                "ComfyUI is running for an install whose links would change. Nothing was changed.",
            ));
        }

        let mut job = UnifyJob {
            unify_id: format!("{UNIFY_JOURNAL_PREFIX}{}", uuid::Uuid::new_v4().simple()),
            sha256: t.record.sha256.clone(),
            name: t.name.clone(),
            started_at: Timestamp::now(),
            replaced: Vec::new(),
            vault_rename: None,
            added_alias: None,
            removed_aliases: Vec::new(),
        };
        self.store.put_unify_job(&job)?;
        let mut journal = Journal { store: self.store, id: job.unify_id.clone(), next: 0 };
        let mut out = UnifyResult {
            unify_id: job.unify_id.clone(),
            name: t.name.clone(),
            renamed: Vec::new(),
            removed: Vec::new(),
            skipped: Vec::new(),
            stopped: None,
            vault_name: t.record.canonical_name.clone(),
        };

        for planned in &plan {
            if planned.action == UnifyAction::Keep {
                continue;
            }
            if planned.action == UnifyAction::Unreachable {
                out.skipped.push(SkippedLink {
                    install_id: planned.install_id.clone(),
                    path: planned.abs_path.clone(),
                    reason: "This link is not on the disk now, for example on a drive that is unplugged. It keeps its name, and loads the model again when the drive is back.".into(),
                });
                continue;
            }
            let Some(record) = t.links.iter().find(|l| l.abs_path == planned.abs_path) else { continue };
            let skip = |out: &mut UnifyResult, reason: String| {
                out.skipped.push(SkippedLink {
                    install_id: record.install_id.clone(),
                    path: record.abs_path.clone(),
                    reason,
                })
            };
            if planned.action == UnifyAction::BlockedTaken {
                skip(&mut out, taken_reason());
                continue;
            }

            // The disk as it is now, not as the plan read it: the link must
            // still be this model's recorded link, and the place for the new
            // name is looked at again.
            if !self.still_ours(record, &t.vault_path)? {
                skip(&mut out, "This link changed since the names were read, so it was left as it is.".into());
                continue;
            }
            let new_path = record.abs_path.with_file_name(&t.name);
            let rename = match self.place_for(&new_path, &t)? {
                Place::Free => true,
                Place::OurLink => false,
                Place::Taken => {
                    skip(&mut out, taken_reason());
                    continue;
                }
            };

            if rename {
                let step = journal.pending(&record.install_id, JournalStep::CreateLink {
                    link: new_path.clone(),
                    target: t.vault_path.clone(),
                })?;
                if let Err(e) = self.platform.create_file_symlink(&new_path, &t.vault_path) {
                    journal.finish(step, JournalState::Failed)?;
                    skip(&mut out, e.message);
                    continue;
                }
                let mut made = self.record_for(&record.install_id, &new_path, &t.record, &job.unify_id);
                made.link_name = t.name.clone();
                if let Err(e) = self.store.put_link(&made) {
                    // No record, no link: one the vault does not know about is
                    // one nothing would ever tidy.
                    let _ = self.platform.remove_symlink(&new_path);
                    journal.finish(step, JournalState::Reverted)?;
                    out.stopped = Some(UnifyStop {
                        install_id: Some(record.install_id.clone()),
                        path: new_path,
                        message: e.message,
                    });
                    break;
                }
                journal.finish(step, JournalState::Done)?;
            }

            // Looked at again right before it goes: something may have taken
            // its place while the new link was made, and removing a link is
            // deleting whatever is at that path. The new link stays, so the
            // model then loads under both names.
            if !self.still_ours(record, &t.vault_path)? {
                skip(&mut out, "This link changed while its name was changing, so it was left as it is.".into());
                continue;
            }

            // Written before the link goes, so the undo can put the record
            // back exactly as it was, whatever happens next.
            job.replaced.push(record.clone());
            self.store.put_unify_job(&job)?;
            let step = journal.pending(&record.install_id, JournalStep::RemoveLink {
                link: record.abs_path.clone(),
                target: t.vault_path.clone(),
            })?;
            if let Err(e) = self.platform.remove_symlink(&record.abs_path) {
                journal.finish(step, JournalState::Failed)?;
                // The new link stays, so the model loads under both names.
                out.stopped = Some(UnifyStop {
                    install_id: Some(record.install_id.clone()),
                    path: record.abs_path.clone(),
                    message: e.message,
                });
                break;
            }
            self.store.delete_link(&record.id)?;
            journal.finish(step, JournalState::Done)?;

            if rename {
                out.renamed.push(RenamedLink {
                    install_id: record.install_id.clone(),
                    from: record.abs_path.clone(),
                    to: new_path,
                });
            } else {
                out.removed.push(RemovedLink { install_id: record.install_id.clone(), path: record.abs_path.clone() });
            }
        }

        if out.stopped.is_none() {
            match self.name_the_vault_file(&t, &mut job, &mut out) {
                Ok(name) => out.vault_name = name,
                Err(e) => {
                    out.stopped = Some(UnifyStop {
                        install_id: None,
                        path: e.path.clone().map(PathBuf::from).unwrap_or_else(|| t.vault_path.clone()),
                        message: e.message,
                    })
                }
            }
        }
        Ok(out)
    }

    /// Everything [`Unify::undo`] checks before it changes anything, with
    /// nothing changed. An undo of a consolidation runs it for every name
    /// change it will put back, before it puts back any of them.
    pub fn check_undo(&self, unify_id: &str) -> Result<()> {
        let job = self
            .store
            .unify_job(unify_id)?
            .ok_or_else(|| VaultError::not_found("That name change is not in this vault's history."))?;
        let record = self.store.vault_file(&job.sha256)?.ok_or_else(|| {
            VaultError::conflict(
                "That model is no longer in the vault, so its old names cannot come back. Nothing was changed.",
            )
        })?;
        let vault_path = self.vault_path(&record)?;
        let done: Vec<JournalEntry> = self
            .store
            .journal(unify_id)?
            .into_iter()
            .filter(|e| e.state == JournalState::Done)
            .collect();

        let touched: Vec<(String, PathBuf)> = done
            .iter()
            .filter_map(|e| match &e.step {
                JournalStep::CreateLink { link, .. } | JournalStep::RemoveLink { link, .. } => {
                    Some((e.group_id.clone(), link.clone()))
                }
                _ => None,
            })
            .collect();
        self.prove_places(&touched, &record.category)?;
        let mut taken: Vec<String> = Vec::new();
        for e in &done {
            if let JournalStep::RemoveLink { link, .. } = &e.step {
                let back_already = self.platform.is_symlink(link) && leads_to(link, &vault_path);
                if std::fs::symlink_metadata(link).is_ok() && !back_already {
                    taken.push(crate::paths::display_path(link));
                }
            }
        }
        if !taken.is_empty() {
            return Err(VaultError::conflict(
                "Something else now has an old name, so the names were not put back. Nothing was changed.",
            )
            .with_detail(taken.join(", ")));
        }
        if let Some(rename) = &job.vault_rename {
            Vault::new(self.store, self.platform).check_undo_rename(rename)?;
        }
        Ok(())
    }

    /// Puts back every old name a job removed, and removes the names it made.
    ///
    /// The vault keeps the name the job gave it: every link put back points at
    /// the file under that name, so each one loads the model.
    ///
    /// Everything is checked before anything changes. A place for an old name
    /// that now holds anything else refuses the whole undo, because putting a
    /// name back must never overwrite. A new link is removed only while it is
    /// still this job's own link to the model.
    pub fn undo(&self, unify_id: &str) -> Result<UnifyUndone> {
        self.check_undo(unify_id)?;
        let job = self
            .store
            .unify_job(unify_id)?
            .ok_or_else(|| VaultError::not_found("That name change is not in this vault's history."))?;
        let done: Vec<JournalEntry> = self
            .store
            .journal(unify_id)?
            .into_iter()
            .filter(|e| e.state == JournalState::Done)
            .collect();

        // The vault's name first, so every old link put back points straight
        // at the file under the name it had before the job.
        let vault = Vault::new(self.store, self.platform);
        if let Some(rename) = &job.vault_rename {
            vault.undo_rename(rename)?;
        }
        let record = self.store.vault_file(&job.sha256)?.ok_or_else(|| {
            VaultError::conflict("That model is no longer in the vault, so its old names cannot come back.")
        })?;
        let vault_path = self.vault_path(&record)?;


        for mut e in done.into_iter().rev() {
            match &e.step {
                JournalStep::RemoveLink { link, .. } => {
                    if std::fs::symlink_metadata(link).is_err() {
                        self.platform.create_file_symlink(link, &vault_path)?;
                    }
                    if self.store.link_at_path(link)?.is_none() {
                        let mut back = job
                            .replaced
                            .iter()
                            .find(|r| crate::paths::same_path_lexically(&r.abs_path, link))
                            .cloned()
                            .unwrap_or_else(|| self.record_for(&e.group_id, link, &record, unify_id));
                        back.vault_rel_path = record.vault_rel_path();
                        self.store.put_link(&back)?;
                    }
                }
                JournalStep::CreateLink { link, .. } => {
                    let mine = self.store.link_at_path(link)?;
                    let others = mine.as_ref().is_some_and(|r| r.apply_id.as_deref() != Some(unify_id));
                    if !others && self.platform.is_symlink(link) && leads_to(link, &vault_path) {
                        self.platform.remove_symlink(link)?;
                    }
                    if let Some(r) = mine.filter(|r| r.apply_id.as_deref() == Some(unify_id)) {
                        if !self.platform.is_symlink(link) {
                            self.store.delete_link(&r.id)?;
                        }
                    }
                }
                _ => {}
            }
            e.state = JournalState::Undone;
            e.finished_at = Some(Timestamp::now());
            self.store.update_journal(&e)?;
        }

        // The vault's second names as they were. A name whose place holds
        // anything now stays away, as nothing is ever overwritten.
        let mut record = record;
        for alias in &job.removed_aliases {
            if record.aliases.contains(alias) {
                continue;
            }
            let Ok(place) =
                crate::paths::resolve_new_path_within(self.store.vault_root(), &PathBuf::from(&record.category).join(alias))
            else {
                continue;
            };
            if std::fs::symlink_metadata(&place).is_ok() || self.platform.create_file_symlink(&place, &vault_path).is_err() {
                continue;
            }
            record.aliases.push(alias.clone());
            record.aliases.sort();
            self.store.put_vault_file(&record)?;
        }
        if let Some(added) = &job.added_alias {
            // Taken away again only when nothing uses it, which is the case
            // once the old links are back.
            let _ = vault.remove_alias(&job.sha256, added);
        }
        Ok(UnifyUndone { undone: true })
    }

    /// Finishes the steps a crash cut off. Run when a vault opens.
    ///
    /// A new link that is on the disk gets its record. An old link that is
    /// gone loses its record. A step that never reached the disk is marked
    /// failed, which leaves the model loading under both names, as a stopped
    /// job does.
    pub fn finish_interrupted(&self) -> Result<u64> {
        // A job's last step renames the vault file, which has its own journal.
        let mut finished = Vault::new(self.store, self.platform).finish_interrupted_renames()?;
        for id in self.store.journal_ids()? {
            if !id.starts_with(UNIFY_JOURNAL_PREFIX) {
                continue;
            }
            let job = self.store.unify_job(&id)?;
            for mut e in self.store.journal(&id)? {
                if e.state != JournalState::Pending {
                    continue;
                }
                let state = match (&e.step, &job) {
                    (JournalStep::CreateLink { link, target }, Some(job))
                        if self.platform.is_symlink(link) && leads_to(link, target) =>
                    {
                        if self.store.link_at_path(link)?.is_none() {
                            if let Some(record) = self.store.vault_file(&job.sha256)? {
                                self.store.put_link(&self.record_for(&e.group_id, link, &record, &id))?;
                            }
                        }
                        JournalState::Done
                    }
                    (JournalStep::RemoveLink { link, .. }, _) if std::fs::symlink_metadata(link).is_err() => {
                        if let Some(r) = self.store.link_at_path(link)? {
                            self.store.delete_link(&r.id)?;
                        }
                        JournalState::Done
                    }
                    _ => JournalState::Failed,
                };
                if state == JournalState::Done {
                    finished += 1;
                }
                e.state = state;
                e.finished_at = Some(Timestamp::now());
                self.store.update_journal(&e)?;
            }
        }
        Ok(finished)
    }

    // -- the parts ---------------------------------------------------------

    fn target(&self, sha256: &str, name: &str) -> Result<Target> {
        let sha = crate::scan::hash::normalize_sha256(sha256)
            .ok_or_else(|| VaultError::invalid("That is not a file hash."))?;
        crate::paths::validate_file_name(name)?;
        let record = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;
        let vault_path = self.vault_path(&record)?;

        let links = Links::new(self.store, self.platform);
        let mut live: Vec<LinkRecord> = Vec::new();
        let mut unreachable: Vec<LinkRecord> = Vec::new();
        for l in self.store.links_for_hash(&sha)? {
            if self.store.install(&l.install_id)?.is_none() {
                continue;
            }
            match links.state_of(&l) {
                LinkState::Ok => {}
                LinkState::Missing => {
                    unreachable.push(l);
                    continue;
                }
                _ => continue,
            }
            // A link someone pointed somewhere else by hand is not this
            // model's any more.
            if leads_to(&l.abs_path, &vault_path) {
                live.push(l);
            }
        }
        live.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));

        let spelled = live
            .iter()
            .find(|l| name_key(&l.link_name) == name_key(name))
            .map(|l| l.link_name.clone())
            .ok_or_else(|| {
                VaultError::invalid("That name is not one the installs use for this model. Nothing was changed.")
            })?;
        unreachable.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));
        Ok(Target { record, vault_path, name: spelled, links: live, unreachable })
    }

    /// The vault file's real place, which must hold the real file.
    fn vault_path(&self, record: &VaultFileRecord) -> Result<PathBuf> {
        let path = crate::paths::resolve_new_path_within(self.store.vault_root(), &record.vault_rel_path())
            .map_err(|e| {
                VaultError::new(
                    ErrorCode::PathOutsideBoundary,
                    "That model's recorded folder is outside the vault, so nothing was changed.",
                )
                .with_detail(e.message)
            })?;
        let real = std::fs::symlink_metadata(&path).map(|m| m.file_type().is_file()).unwrap_or(false);
        if !real {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "The vault no longer holds that file, so its names cannot change.",
            )
            .with_path(&path));
        }
        Ok(path)
    }

    /// What the job would do to each link, in path order.
    ///
    /// Every place a link would be made or removed is proved to be in an
    /// install first. A record that names anywhere else refuses the whole
    /// job, as it refuses an apply.
    fn classify(&self, t: &Target) -> Result<Vec<UnifyLink>> {
        let mut out = Vec::new();
        // New places an earlier link of this job will fill.
        let mut claimed: Vec<String> = Vec::new();
        // Each place the job would make or remove a link, with its install.
        let mut touched: Vec<(String, PathBuf)> = Vec::new();

        for l in &t.links {
            let mut row = UnifyLink {
                install_id: l.install_id.clone(),
                abs_path: l.abs_path.clone(),
                link_name: l.link_name.clone(),
                action: UnifyAction::Keep,
                new_abs_path: None,
                taken_by: None,
            };
            if name_key(&l.link_name) == name_key(&t.name) {
                out.push(row);
                continue;
            }
            let new_path = l.abs_path.with_file_name(&t.name);
            let key = crate::paths::compare_key(&new_path);
            row.action = if claimed.contains(&key) {
                UnifyAction::Remove
            } else {
                match self.place_for(&new_path, t)? {
                    Place::Free => {
                        claimed.push(key);
                        row.new_abs_path = Some(new_path.clone());
                        touched.push((l.install_id.clone(), new_path.clone()));
                        UnifyAction::Rename
                    }
                    Place::OurLink => UnifyAction::Remove,
                    Place::Taken => {
                        row.taken_by = Some(crate::paths::display_path(&new_path));
                        UnifyAction::BlockedTaken
                    }
                }
            };
            if changes(row.action) {
                touched.push((l.install_id.clone(), l.abs_path.clone()));
            }
            out.push(row);
        }
        self.prove_places(&touched, &t.record.category)?;
        out.extend(t.unreachable.iter().map(|l| UnifyLink {
            install_id: l.install_id.clone(),
            abs_path: l.abs_path.clone(),
            link_name: l.link_name.clone(),
            action: UnifyAction::Unreachable,
            new_abs_path: None,
            taken_by: None,
        }));
        out.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));
        Ok(out)
    }

    /// Proves every place the job would make or remove a link is a model
    /// file's place in that link's install, and refuses the whole job
    /// otherwise, as an apply is refused.
    ///
    /// A place passes when it is inside a folder the scan walks, or inside a
    /// folder ComfyUI reads for the model's category. The second covers a
    /// category folder that is a junction to another drive, which the first
    /// resolves to outside the install. Neither is ever inside the vault.
    fn prove_places(&self, touched: &[(String, PathBuf)], category: &str) -> Result<()> {
        let places = Places::read(self.store)?;
        let mut bad: Vec<PathBuf> = Vec::new();
        for (install_id, path) in touched {
            let install = self.store.install(install_id)?.and_then(|i| i.proved().ok());
            let in_a_root = || {
                let (Some(install), Some(dir)) = (install.as_ref(), path.parent()) else { return false };
                crate::download::folders::inside_roots(install, category, dir, self.store.vault_root()).is_ok()
            };
            let ok = install.is_some() && places.is_model_name(path) && (places.in_an_install(path) || in_a_root());
            if !ok {
                bad.push(path.clone());
            }
        }
        if bad.is_empty() {
            return Ok(());
        }
        bad.sort();
        bad.dedup();
        Err(boundary::refusal(&bad))
    }

    /// What is at the place a link would take its new name.
    fn place_for(&self, path: &Path, t: &Target) -> Result<Place> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Place::Free),
            Err(_) => Ok(Place::Taken),
            Ok(_) => {
                // Only a link the vault records for this same model. An
                // unrecorded one stays, or the install would be left with a
                // link the vault knows nothing about.
                let recorded = self.store.link_at_path(path)?.is_some_and(|r| r.sha256 == t.record.sha256);
                if recorded && self.platform.is_symlink(path) && leads_to(path, &t.vault_path) {
                    Ok(Place::OurLink)
                } else {
                    Ok(Place::Taken)
                }
            }
        }
    }

    /// The link is still on the disk as recorded, leading to the model.
    fn still_ours(&self, record: &LinkRecord, vault_path: &Path) -> Result<bool> {
        let same = self.store.link(&record.id)?.is_some_and(|r| r.abs_path == record.abs_path);
        Ok(same && self.platform.is_symlink(&record.abs_path) && leads_to(&record.abs_path, vault_path))
    }

    /// The installs among the links, proved on the disk, for the workflow search.
    fn installs_of(&self, t: &Target) -> Result<Vec<Install>> {
        let mut out: Vec<Install> = Vec::new();
        for l in &t.links {
            if out.iter().any(|i| i.id == l.install_id) {
                continue;
            }
            if let Some(i) = self.store.install(&l.install_id)?.and_then(|i| i.proved().ok()) {
                out.push(i);
            }
        }
        Ok(out)
    }

    /// The installs whose links change and whose ComfyUI is running.
    fn running(&self, links: &[UnifyLink]) -> Result<Vec<String>> {
        let affected: Vec<&str> =
            links.iter().filter(|l| changes(l.action)).map(|l| l.install_id.as_str()).collect();
        if affected.is_empty() {
            return Ok(Vec::new());
        }
        let installs: Vec<(String, PathBuf)> = self.store.installs()?.into_iter().map(|i| (i.id, i.root)).collect();
        let mut out: Vec<String> =
            crate::platform::match_processes_to_installs(&self.platform.list_processes(), &installs)
                .into_iter()
                .flat_map(|r| r.matched_install_ids)
                .filter(|id| affected.contains(&id.as_str()))
                .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// A record for a link this job made at `path`.
    fn record_for(&self, install_id: &str, path: &Path, file: &VaultFileRecord, unify_id: &str) -> LinkRecord {
        let rel = self
            .store
            .install(install_id)
            .ok()
            .flatten()
            .and_then(|i| path.strip_prefix(&i.root).ok().map(Path::to_path_buf))
            .unwrap_or_else(|| path.to_path_buf());
        LinkRecord {
            id: uuid::Uuid::new_v4().to_string(),
            install_id: install_id.to_string(),
            rel_path: rel,
            abs_path: path.to_path_buf(),
            link_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            sha256: file.sha256.clone(),
            vault_rel_path: file.vault_rel_path(),
            created_at: Timestamp::now(),
            created_by: LinkOrigin::Manual,
            apply_id: Some(unify_id.to_string()),
        }
    }

    /// Gives the vault file the chosen name, then removes each name the vault
    /// keeps that no link uses any more. Returns the name the vault keeps.
    ///
    /// The vault keeps its name when another model or another file already
    /// has the chosen one there. That is not a failure: every install already
    /// uses the chosen name, and the vault's name is only what the Library
    /// shows.
    fn name_the_vault_file(&self, t: &Target, job: &mut UnifyJob, out: &mut UnifyResult) -> Result<String> {
        let vault = Vault::new(self.store, self.platform);
        let sha = &t.record.sha256;
        let mut record = self
            .store
            .vault_file(sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;

        if name_key(&record.canonical_name) != name_key(&t.name) {
            let alias = match record.aliases.iter().find(|a| name_key(a) == name_key(&t.name)).cloned() {
                Some(alias) => Some(alias),
                None => {
                    let place = PathBuf::from(&record.category).join(&t.name);
                    let free = crate::paths::resolve_new_path_within(self.store.vault_root(), &place)
                        .map(|p| std::fs::symlink_metadata(p).is_err())
                        .unwrap_or(false);
                    let other = self.store.vault_name_taken(&record.category, &t.name)?.is_some_and(|s| &s != sha);
                    if free && !other {
                        // Written first, so the undo knows to take the name
                        // away again whatever happens next.
                        job.added_alias = Some(t.name.clone());
                        self.store.put_unify_job(job)?;
                        record.aliases.push(t.name.clone());
                        record.aliases.sort();
                        self.store.put_vault_file(&record)?;
                        Some(t.name.clone())
                    } else {
                        None
                    }
                }
            };
            if let Some(alias) = alias {
                // Recorded before the rename starts, so the undo finds it
                // whatever happens part way.
                let id = Vault::new_rename_id();
                job.vault_rename = Some(id.clone());
                self.store.put_unify_job(job)?;
                let renamed = vault.rename_file(sha, &alias, &id)?;
                for n in renamed.not_repointed {
                    out.skipped.push(SkippedLink {
                        install_id: n.install_id,
                        path: n.path,
                        reason: format!(
                            "{} This link still loads the model, through the vault's old name for it.",
                            n.message
                        ),
                    });
                }
            }
        }

        let record = self
            .store
            .vault_file(sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;
        for alias in &record.aliases {
            job.removed_aliases.push(alias.clone());
            self.store.put_unify_job(job)?;
            // Removing refuses a name any recorded link still names, one on a
            // drive that is unplugged included, and a name a real file now
            // holds. Such a name stays as it was: nothing is lost by that, and
            // the install links on the disk already carry the chosen name.
            if vault.remove_alias(sha, alias).is_err() {
                job.removed_aliases.pop();
                self.store.put_unify_job(job)?;
            }
        }
        Ok(self.store.vault_file(sha)?.map(|r| r.canonical_name).unwrap_or_default())
    }
}

enum Place {
    Free,
    /// This model's own recorded link.
    OurLink,
    Taken,
}

fn changes(action: UnifyAction) -> bool {
    matches!(action, UnifyAction::Rename | UnifyAction::Remove)
}

fn taken_reason() -> String {
    "Another file already has that name in this folder, so this link kept its name.".into()
}

fn leads_to(link: &Path, target: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(link), std::fs::canonicalize(target)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// The job's journal. Each step is written before it touches the disk.
struct Journal<'a> {
    store: &'a Store,
    id: String,
    next: u64,
}

impl Journal<'_> {
    fn pending(&mut self, install_id: &str, step: JournalStep) -> Result<JournalEntry> {
        let e = JournalEntry {
            apply_id: self.id.clone(),
            seq: self.next,
            group_id: install_id.to_string(),
            step,
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        };
        self.next += 1;
        self.store.append_journal(&e)?;
        Ok(e)
    }

    fn finish(&mut self, mut e: JournalEntry, state: JournalState) -> Result<()> {
        e.state = state;
        e.finished_at = Some(Timestamp::now());
        self.store.update_journal(&e)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recheck_tests;
