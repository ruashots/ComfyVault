//! Putting a vault model into an install, and taking it out again.
//!
//! This is the manual half of the product: the person picks a model in the
//! vault, picks a folder in an install, and the engine leaves a link there.
//!
//! # The boundary
//!
//! A link is only useful where ComfyUI looks for models, and a link anywhere
//! else is a file the engine had no business writing. So every path here is
//! resolved and proved to land inside one of the install's model roots: its
//! `models` folder, a folder named in its `extra_model_paths.yaml`, or one of
//! the five model folders under `output/`. Anything else is refused.
//!
//! The check resolves the whole path first, so neither a `..` sequence nor a
//! symbolic link planted in the folder chain can reach past it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, Result, VaultError};
use crate::install::Install;
use crate::platform::Platform;
use crate::store::{JournalEntry, JournalState, JournalStep, LinkOrigin, LinkRecord, LinkState, Store};
use crate::time_util::Timestamp;

/// A folder in an install that can receive a link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDirNode {
    /// Relative to the install root where that makes sense, and absolute for a
    /// folder outside it that `extra_model_paths.yaml` named.
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub category: String,
    pub origin: crate::install::RootOrigin,
    pub file_count: u64,
    pub children: Vec<ModelDirNode>,
}

/// Creating, removing and listing the links inside installs.
pub struct Links<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
}

/// What the caller asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLinkRequest {
    pub install_id: String,
    pub sha256: String,
    /// Relative to the install root, for example `models/loras/style`.
    /// Give this or `dir`, not both.
    #[serde(default)]
    pub relative_dir: String,
    /// A full folder path: one of the folders ComfyUI reads for the model's
    /// kind in this install, or a folder inside one. It can be outside the
    /// install, for a folder its `extra_model_paths.yaml` adds.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Defaults to the vault file's own name.
    #[serde(default)]
    pub link_name: Option<String>,
    #[serde(default)]
    pub create_dir: bool,
}

/// How the journal of a link made by hand is named.
pub const LINK_JOURNAL_PREFIX: &str = "link-";

/// The journal of one link made by hand: its folder, if one is made, and the
/// link. The group is the install, so a cut-off step can be finished.
pub struct LinkJournal<'a> {
    store: &'a Store,
    id: String,
    install_id: String,
    entries: Vec<JournalEntry>,
}

impl<'a> LinkJournal<'a> {
    fn new(store: &'a Store, install_id: &str) -> Self {
        Self {
            store,
            id: format!("{LINK_JOURNAL_PREFIX}{}", uuid::Uuid::new_v4().simple()),
            install_id: install_id.to_string(),
            entries: Vec::new(),
        }
    }

    fn pending(&mut self, step: JournalStep) -> Result<usize> {
        let e = JournalEntry {
            apply_id: self.id.clone(),
            seq: self.entries.len() as u64,
            group_id: self.install_id.clone(),
            step,
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        };
        self.store.append_journal(&e)?;
        self.entries.push(e);
        Ok(self.entries.len() - 1)
    }

    fn mark(&mut self, i: usize, state: JournalState) -> Result<()> {
        let e = &mut self.entries[i];
        e.state = state;
        e.finished_at = Some(Timestamp::now());
        self.store.update_journal(e)
    }

    /// Journals the making of `folder`, when it is missing and may be made.
    fn folder(&mut self, folder: &Path, create: bool) -> Result<Option<usize>> {
        if !create || folder.is_dir() {
            return Ok(None);
        }
        Ok(Some(self.pending(JournalStep::CreateDir { path: folder.to_path_buf() })?))
    }

    fn folder_done(&mut self, step: Option<usize>, made: bool) -> Result<()> {
        match step {
            Some(i) => self.mark(i, if made { JournalState::Done } else { JournalState::Failed }),
            None => Ok(()),
        }
    }
}

/// One folder in the chooser for a new link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkFolder {
    pub path: PathBuf,
    pub name: String,
    pub origin: crate::install::RootOrigin,
    /// A root can be listed before it exists, such as `models\\loras` in a
    /// fresh install. It is made when a link goes in it.
    pub exists: bool,
    pub has_subfolders: bool,
}

/// The chooser's answer: the folders, and where a new link of this kind
/// goes in this install unless the person picks another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkFolderList {
    pub folders: Vec<LinkFolder>,
    /// The folder picked last time for this kind in this install, while
    /// ComfyUI still reads it, or else where ComfyUI saves new files of it.
    pub default_dir: Option<String>,
}

fn has_subfolders(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut it| it.any(|e| e.ok().and_then(|e| e.file_type().ok()).map(|t| t.is_dir()).unwrap_or(false)))
        .unwrap_or(false)
}

/// A recorded link, with what it looks like on disk right now.
///
/// `state` is not stored. It is a fact about the disk, so it is measured when
/// the list is read. The record itself stays the record. The fields of
/// [`LinkRecord`] sit directly alongside `state`, not nested under a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkWithState {
    #[serde(flatten)]
    pub link: LinkRecord,
    pub state: LinkState,
}

impl<'a> Links<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform) -> Self {
        Self { store, platform }
    }

    /// Creates one link inside an install, pointing at a vault file.
    pub fn create(&self, req: &CreateLinkRequest) -> Result<LinkRecord> {
        let install = self
            .store
            .install(&req.install_id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))?
            // Its folders come from the disk, not from the database row.
            .proved()?;
        let mut journal = LinkJournal::new(self.store, &install.id);
        let Some(dir) = &req.dir else {
            let folder = self.resolve_dir_path(&install, &req.relative_dir)?;
            let made = journal.folder(&folder, req.create_dir)?;
            let target_dir = self.resolve_dir(&install, &req.relative_dir, req.create_dir);
            journal.folder_done(made, target_dir.is_ok())?;
            return self.make(&install, &req.sha256, target_dir?, req.link_name.clone(), LinkOrigin::Manual, None, Some(&mut journal));
        };
        if !req.relative_dir.is_empty() {
            return Err(VaultError::invalid("Give the folder one way, not both. Nothing was created."));
        }
        let sha = crate::scan::hash::normalize_sha256(&req.sha256)
            .ok_or_else(|| VaultError::invalid("That is not a file hash."))?;
        let category = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?
            .category;
        crate::download::folders::inside_roots(&install, &category, dir)?;
        if !dir.is_dir() && !req.create_dir {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "That folder does not exist. Choose another one, or let the app create it.",
            )
            .with_path(dir));
        }
        let made = journal.folder(dir, req.create_dir)?;
        let target_dir = self.prove_dir(&install, dir.clone(), req.create_dir);
        journal.folder_done(made, target_dir.is_ok())?;
        let record = self.make(&install, &sha, target_dir?, req.link_name.clone(), LinkOrigin::Manual, None, Some(&mut journal))?;
        self.store.put_link_dir(&install.id, &category, dir)?;
        Ok(record)
    }

    /// The folders a new link for `category` can go in, for the chooser:
    /// the roots when `dir` is `None`, or the folders directly inside `dir`,
    /// which must be a root or inside one.
    pub fn link_folders(&self, install_id: &str, category: &str, dir: Option<&Path>) -> Result<LinkFolderList> {
        let (folders, install) = self.link_folders_of(install_id, category, dir)?;
        let default = crate::download::default_dir(self.store, &install, category)?;
        Ok(LinkFolderList { folders, default_dir: Some(crate::paths::display_path(&default)) })
    }

    fn link_folders_of(&self, install_id: &str, category: &str, dir: Option<&Path>) -> Result<(Vec<LinkFolder>, Install)> {
        crate::paths::validate_file_name(category)?;
        let install = self
            .store
            .install(install_id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))?
            .proved()?;
        let roots = crate::download::folders::roots(&install, category);
        let Some(dir) = dir else {
            let folders = roots
                .into_iter()
                .map(|r| LinkFolder {
                    name: r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    exists: r.path.is_dir(),
                    has_subfolders: has_subfolders(&r.path),
                    path: r.path,
                    origin: r.origin,
                })
                .collect();
            return Ok((folders, install));
        };
        crate::download::folders::inside_roots(&install, category, dir)?;
        let origin = roots
            .iter()
            .find(|r| dir.starts_with(&r.path))
            .map(|r| r.origin)
            .unwrap_or(crate::install::RootOrigin::ModelsDir);
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                // Only real folders. A linked folder can lead anywhere, and
                // the check below would refuse it when chosen anyway.
                if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let path = e.path();
                out.push(LinkFolder {
                    name: e.file_name().to_string_lossy().to_string(),
                    has_subfolders: has_subfolders(&path),
                    path,
                    origin,
                    exists: true,
                });
            }
        }
        out.sort_by_key(|f| f.name.to_lowercase());
        Ok((out, install))
    }

    /// Makes one folder inside a root for `category`, for the chooser. Its
    /// parent must already be there.
    pub fn make_link_folder(&self, install_id: &str, category: &str, dir: &Path) -> Result<(PathBuf, bool)> {
        crate::paths::validate_file_name(category)?;
        let install = self
            .store
            .install(install_id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))?
            .proved()?;
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        crate::paths::validate_file_name(&name)?;
        crate::download::folders::inside_roots(&install, category, dir)?;
        if dir.is_dir() {
            return Ok((dir.to_path_buf(), false));
        }
        let parent = dir.parent().filter(|p| p.is_dir()).ok_or_else(|| {
            VaultError::new(ErrorCode::NotFound, "The folder it goes in does not exist. Choose another place.").with_path(dir)
        })?;
        crate::download::folders::inside_roots(&install, category, parent)?;
        std::fs::create_dir(dir).map_err(|e| VaultError::from_io(&e, dir, "making the folder"))?;
        // Checked again where it really landed, and removed if that is out.
        if let Err(e) = crate::download::folders::inside_roots(&install, category, dir) {
            let _ = std::fs::remove_dir(dir);
            return Err(e);
        }
        Ok((dir.to_path_buf(), true))
    }

    /// Creates one link in a folder given as a full path, which must be one
    /// of the install's model folders, like every other link.
    ///
    /// A download uses this, because the folder ComfyUI saves a category into
    /// can be a shared folder outside the install.
    pub fn create_in(
        &self,
        install: &Install,
        folder: &Path,
        sha256: &str,
        name: &str,
        origin: LinkOrigin,
        apply_id: Option<String>,
    ) -> Result<LinkRecord> {
        let target_dir = self.prove_dir(install, folder.to_path_buf(), true)?;
        // A download journals its own steps.
        self.make(install, sha256, target_dir, Some(name.to_string()), origin, apply_id, None)
    }

    fn make(
        &self,
        install: &Install,
        sha256: &str,
        target_dir: PathBuf,
        link_name: Option<String>,
        origin: LinkOrigin,
        apply_id: Option<String>,
        mut journal: Option<&mut LinkJournal<'_>>,
    ) -> Result<LinkRecord> {
        let sha = crate::scan::hash::normalize_sha256(sha256)
            .ok_or_else(|| VaultError::invalid("That is not a file hash."))?;
        let vault_file = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;

        let vault_path = self.store.vault_root().join(vault_file.vault_rel_path());
        if !vault_path.is_file() {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "The vault no longer holds that file, so a link to it would point at nothing.",
            )
            .with_path(&vault_path));
        }

        let name = match link_name {
            Some(n) => n,
            None => vault_file.canonical_name.clone(),
        };
        crate::paths::validate_file_name(&name)?;

        // A link this engine creates is always a model file. Without this, a
        // caller can name one `__init__.py`, and ComfyUI imports that at
        // startup, so the engine would be putting a file of someone's choosing
        // on the import path. Nothing legitimate is lost by requiring it.
        let settings = self.store.settings()?;
        if !settings.matches_extension(&name) {
            return Err(VaultError::invalid(format!(
                "A model file has to end in one of {}. Nothing was created.",
                settings.scan_extensions.join(", ")
            )));
        }

        let link_path = target_dir.join(&name);

        // Never overwrite. Something already here is the person's file, and
        // replacing it would destroy whatever it is.
        if std::fs::symlink_metadata(&link_path).is_ok() {
            let what = if self.platform.is_symlink(&link_path) {
                "a link"
            } else {
                "a file"
            };
            return Err(VaultError::new(
                ErrorCode::Conflict,
                format!("There is already {what} with that name in that folder. Nothing was changed."),
            )
            .with_path(&link_path));
        }

        // Written before the link, and marked done only once its record is
        // saved. A crash in between leaves the step pending, and the next
        // time the vault opens, the record is written for the link it made.
        let step = match journal.as_deref_mut() {
            Some(j) => Some(j.pending(JournalStep::CreateLink { link: link_path.clone(), target: vault_path.clone() })?),
            None => None,
        };
        if let Err(e) = self.platform.create_file_symlink(&link_path, &vault_path) {
            if let (Some(j), Some(i)) = (journal.as_deref_mut(), step) {
                let _ = j.mark(i, JournalState::Failed);
            }
            return Err(e);
        }

        let record = LinkRecord {
            id: uuid::Uuid::new_v4().to_string(),
            install_id: install.id.clone(),
            rel_path: link_path
                .strip_prefix(&install.root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| link_path.clone()),
            abs_path: link_path,
            link_name: name,
            sha256: sha,
            vault_rel_path: vault_file.vault_rel_path(),
            created_at: Timestamp::now(),
            created_by: origin,
            apply_id,
        };
        if let Err(e) = self.store.put_link(&record) {
            // No record, no link: a link the vault does not know about is
            // one nothing would ever tidy.
            let _ = self.platform.remove_symlink(&record.abs_path);
            if let (Some(j), Some(i)) = (journal.as_deref_mut(), step) {
                let _ = j.mark(i, JournalState::Reverted);
            }
            return Err(e);
        }
        if let (Some(j), Some(i)) = (journal, step) {
            j.mark(i, JournalState::Done)?;
        }
        Ok(record)
    }

    /// Finishes the links a crash cut off between the link and its record.
    ///
    /// Run when a vault opens. A pending step whose link is on the disk,
    /// leading to the vault file it names, gets its record. One whose link
    /// never appeared is marked failed. Anything else at that path is not
    /// this engine's, and is left alone.
    pub fn finish_interrupted(&self) -> Result<u64> {
        let mut finished = 0;
        for id in self.store.journal_ids()? {
            if !id.starts_with(LINK_JOURNAL_PREFIX) {
                continue;
            }
            for mut e in self.store.journal(&id)? {
                if e.state != JournalState::Pending {
                    continue;
                }
                let state = match &e.step {
                    JournalStep::CreateLink { link, target } => self.finish_one(&e.group_id, &id, link, target)?,
                    // A folder is a folder whether or not the link followed.
                    JournalStep::CreateDir { path } if path.is_dir() => JournalState::Done,
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

    fn finish_one(&self, install_id: &str, journal: &str, link: &Path, target: &Path) -> Result<JournalState> {
        let ours = self.platform.is_symlink(link)
            && matches!((std::fs::canonicalize(link), std::fs::canonicalize(target)), (Ok(a), Ok(b)) if a == b);
        if !ours {
            return Ok(JournalState::Failed);
        }
        if self.store.link_at_path(link)?.is_some() {
            return Ok(JournalState::Done);
        }
        let root = self.store.vault_root();
        let Some(file) = self
            .store
            .vault_files()?
            .into_iter()
            .find(|f| crate::paths::same_path_lexically(&root.join(f.vault_rel_path()), target))
        else {
            return Ok(JournalState::Failed);
        };
        let rel = self
            .store
            .install(install_id)?
            .and_then(|i| link.strip_prefix(&i.root).ok().map(Path::to_path_buf))
            .unwrap_or_else(|| link.to_path_buf());
        self.store.put_link(&LinkRecord {
            id: uuid::Uuid::new_v4().to_string(),
            install_id: install_id.to_string(),
            rel_path: rel,
            abs_path: link.to_path_buf(),
            link_name: link.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            sha256: file.sha256.clone(),
            vault_rel_path: file.vault_rel_path(),
            created_at: Timestamp::now(),
            created_by: LinkOrigin::Manual,
            apply_id: Some(journal.to_string()),
        })?;
        Ok(JournalState::Done)
    }

    /// Removes one link. Never touches the vault file it points at.
    pub fn remove(&self, link_id: &str) -> Result<()> {
        let record = self
            .store
            .link(link_id)?
            .ok_or_else(|| VaultError::not_found("That link is not in this vault's records."))?;

        match std::fs::symlink_metadata(&record.abs_path) {
            Ok(m) if m.file_type().is_symlink() => {
                self.platform.remove_symlink(&record.abs_path)?;
            }
            Ok(_) => {
                // A real file took its place. Deleting it would destroy
                // something the engine did not create.
                return Err(VaultError::conflict(
                    "A real file is in that place now, not a link, so nothing was removed.",
                )
                .with_path(&record.abs_path));
            }
            // Already gone. Forgetting the record is the right outcome.
            Err(_) => {}
        }
        self.store.delete_link(link_id)?;
        Ok(())
    }

    /// Creates a folder inside an install's model roots.
    pub fn create_folder(&self, install_id: &str, relative_dir: &str) -> Result<(PathBuf, bool)> {
        let install = self
            .store
            .install(install_id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))?
            // Its folders come from the disk, not from the database row.
            .proved()?;
        let existed = self.resolve_dir_path(&install, relative_dir)?.is_dir();
        let path = self.resolve_dir(&install, relative_dir, true)?;
        Ok((path, !existed))
    }

    /// The links this vault knows about, with what each one looks like on disk.
    pub fn list(
        &self,
        install_id: Option<&str>,
        sha256: Option<&str>,
        state: Option<LinkState>,
    ) -> Result<Vec<LinkWithState>> {
        let mut out = self.store.links()?;
        if let Some(id) = install_id {
            out.retain(|l| l.install_id == id);
        }
        if let Some(sha) = sha256.and_then(crate::scan::hash::normalize_sha256) {
            out.retain(|l| l.sha256 == sha);
        }
        if let Some(want) = state {
            out.retain(|l| self.state_of(l) == want);
        }
        out.sort_by(|a, b| a.abs_path.cmp(&b.abs_path));
        Ok(out
            .into_iter()
            .map(|l| LinkWithState { state: self.state_of(&l), link: l })
            .collect())
    }

    /// What a recorded link looks like on disk right now.
    pub fn state_of(&self, link: &LinkRecord) -> LinkState {
        match std::fs::symlink_metadata(&link.abs_path) {
            Err(_) => LinkState::Missing,
            Ok(m) if !m.file_type().is_symlink() => LinkState::Replaced,
            // A link that resolves to nothing. ComfyUI lists it in the model
            // menu and then fails to load it, so this is the worst state.
            Ok(_) if !link.abs_path.exists() => LinkState::Dangling,
            Ok(_) => LinkState::Ok,
        }
    }

    /// The folders an install offers as a place to put a link.
    pub fn model_dirs(&self, install_id: &str) -> Result<Vec<ModelDirNode>> {
        let install = self
            .store
            .install(install_id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))?
            // Its folders come from the disk, not from the database row.
            .proved()?;

        // Only the folders a link may actually be written to. Offering a
        // folder the engine would then refuse is a worse experience than not
        // offering it, and offering one it would accept but should not is
        // worse still.
        let boundaries = install.link_boundaries();
        let mut out = Vec::new();
        for root in install.scan_roots(true, true) {
            if !root.path.is_dir() || !boundaries.iter().any(|b| root.path.starts_with(b)) {
                continue;
            }
            let rel = root
                .path
                .strip_prefix(&install.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| crate::paths::display_path(&root.path));

            out.push(build_node(
                &root.path,
                rel,
                root.category.clone().unwrap_or_default(),
                root.origin,
                0,
            ));
        }
        Ok(out)
    }

    // -- the boundary -----------------------------------------------------

    /// Resolves a folder and proves it lands inside one of the install's model
    /// roots.
    fn resolve_dir(&self, install: &Install, relative_dir: &str, create: bool) -> Result<PathBuf> {
        let candidate = self.resolve_dir_path(install, relative_dir)?;
        self.prove_dir(install, candidate, create)
    }

    /// Proves a folder lands inside one of the install's model roots, and
    /// creates it if asked.
    fn prove_dir(&self, install: &Install, candidate: PathBuf, create: bool) -> Result<PathBuf> {
        let boundaries = install.link_boundaries();

        // Checked before anything is created. A refused request must leave no
        // trace, so the engine does not make a folder and then decide it should
        // not have.
        if !boundaries.iter().any(|b| candidate.starts_with(b)) {
            return Err(outside_boundary(&candidate));
        }

        if !candidate.is_dir() {
            if !create {
                return Err(VaultError::new(
                    ErrorCode::NotFound,
                    "That folder does not exist. Choose another one, or let the app create it.",
                )
                .with_path(&candidate));
            }
            std::fs::create_dir_all(&candidate)
                .map_err(|e| VaultError::from_io(&e, &candidate, "creating the folder"))?;
        }

        // Checked again against the real, fully resolved location. The first
        // check compared text; this one follows every link in the chain. If it
        // fails, whatever was just created is removed again.
        let resolved = crate::paths::canonicalize_clean(&candidate)
            .map_err(|e| VaultError::from_io(&e, &candidate, "opening the folder"))?;
        if !boundaries.iter().any(|b| crate::paths::is_within(b, &resolved)) {
            let _ = crate::apply::fsops::remove_dir_if_empty(&candidate);
            return Err(outside_boundary(&resolved));
        }
        Ok(resolved)
    }

    /// Joins the relative folder onto the install root, refusing traversal.
    fn resolve_dir_path(&self, install: &Install, relative_dir: &str) -> Result<PathBuf> {
        let rel = crate::paths::validate_relative_dir(relative_dir)?;
        if rel.as_os_str().is_empty() {
            return Err(VaultError::invalid("Choose a folder inside the install."));
        }
        // Resolves the existing part of the path fully, so a link planted in
        // the chain cannot reach outside the install.
        crate::paths::resolve_within(&install.root, &rel)
    }
}

fn outside_boundary(path: &Path) -> VaultError {
    VaultError::new(
        ErrorCode::PathOutsideBoundary,
        "That folder is outside the places ComfyUI looks for models, so a link there would do nothing.",
    )
    .with_path(path)
}

fn build_node(
    path: &Path,
    rel: String,
    category: String,
    origin: crate::install::RootOrigin,
    depth: usize,
) -> ModelDirNode {
    let mut file_count = 0u64;
    let mut children = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for e in entries.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                Ok(_) => file_count += 1,
                Err(_) => {}
            }
        }
        // Three levels is deeper than any real model tree, and it keeps the
        // folder picker from walking a terabyte.
        if depth < 3 {
            dirs.sort();
            for d in dirs {
                let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                children.push(build_node(
                    &d,
                    format!("{rel}/{name}"),
                    category.clone(),
                    origin,
                    depth + 1,
                ));
            }
        }
    }

    ModelDirNode {
        rel_path: rel,
        abs_path: path.to_path_buf(),
        category,
        origin,
        file_count,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{weights, weights_hash, TestWorld};

    /// Puts one content in the vault, the way an apply would.
    fn vault_a_file(w: &TestWorld, tag: &str, category: &str, name: &str) -> String {
        let sha = weights_hash(tag);
        let dir = w.vault_root.join(category);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), weights(tag)).unwrap();
        w.store
            .put_vault_file(&crate::store::VaultFileRecord {
                sha256: sha.clone(),
                canonical_name: name.to_string(),
                category: category.to_string(),
                size_bytes: weights(tag).len() as u64,
                added_at: Timestamp::now(),
                aliases: vec![],
            })
            .unwrap();
        sha
    }

    fn links<'a>(w: &'a TestWorld) -> Links<'a> {
        Links::new(&w.store, &w.platform)
    }

    #[test]
    fn a_link_is_created_and_reads_the_vault_file() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        std::fs::create_dir_all(i.root.join("models/loras")).unwrap();

        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id.clone(),
                sha256: sha.clone(),
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: false,
            })
            .unwrap();

        assert!(w.is_link(&record.abs_path));
        assert_eq!(w.read(&record.abs_path), weights("lora1"));
        assert_eq!(record.link_name, "lora1.safetensors");
        assert_eq!(record.sha256, sha);
        assert_eq!(record.created_by, LinkOrigin::Manual);
        assert_eq!(record.rel_path, PathBuf::from("models/loras/lora1.safetensors"));
    }

    #[test]
    fn a_link_can_be_given_a_different_name() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        std::fs::create_dir_all(i.root.join("models/loras")).unwrap();

        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: Some("my-own-name.safetensors".into()),
                create_dir: false,
            })
            .unwrap();

        assert_eq!(record.link_name, "my-own-name.safetensors");
        assert_eq!(w.read(&record.abs_path), weights("lora1"));
    }

    #[test]
    fn a_missing_folder_is_refused_unless_the_caller_asks_for_it() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        let req = CreateLinkRequest {
            install_id: i.id.clone(),
            sha256: sha.clone(),
            relative_dir: "models/loras/brand-new".into(), dir: None,
            link_name: None,
            create_dir: false,
        };
        assert_eq!(links(&w).create(&req).unwrap_err().code, ErrorCode::NotFound);

        let record = links(&w)
            .create(&CreateLinkRequest { create_dir: true, ..req })
            .unwrap();
        assert!(record.abs_path.parent().unwrap().is_dir());
        assert!(w.is_link(&record.abs_path));
    }

    #[test]
    fn a_link_is_never_put_on_top_of_an_existing_file() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let occupied = w.write_model(&i, "models/loras/lora1.safetensors", b"the person's own file");

        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: false,
            })
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&occupied).unwrap(), b"the person's own file");
    }

    #[test]
    fn a_folder_outside_the_installs_model_roots_is_refused() {
        // The security boundary. A link anywhere else is a file the engine had
        // no business writing.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        for bad in ["custom_nodes", "comfy", "output/images", ""] {
            let err = links(&w)
                .create(&CreateLinkRequest {
                    install_id: i.id.clone(),
                    sha256: sha.clone(),
                    relative_dir: bad.into(), dir: None,
                    link_name: None,
                    create_dir: true,
                })
                .unwrap_err();
            assert!(
                matches!(err.code, ErrorCode::PathOutsideBoundary | ErrorCode::InvalidArgument),
                "{bad} was allowed, with {:?}",
                err.code
            );
        }
    }

    #[test]
    fn a_folder_path_that_climbs_out_is_refused() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/../../escape".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        assert!(!w.path().join("escape").exists(), "a folder was created outside the install");
    }

    #[test]
    fn a_link_reaching_out_through_a_planted_folder_link_is_refused() {
        // A folder link inside models that points outside the install. A check
        // that only looked at the text would let this through.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        let outside = w.path().join("somewhere-else");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(i.root.join("models")).unwrap();
        let planted = i.root.join("models/escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &planted).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &planted).unwrap();

        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/escape".into(), dir: None,
                link_name: None,
                create_dir: false,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "something was written outside");
    }

    #[test]
    fn a_folder_named_in_extra_model_paths_is_inside_the_boundary() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let shared = w.path().join("shared-loras");
        let i = w.add_extra_model_path(&i, "loras", &shared);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        // Reached through the install root, which is where the contract says
        // the relative folder starts.
        let inside = i.root.join("models/loras");
        std::fs::create_dir_all(&inside).unwrap();
        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id.clone(),
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: false,
            })
            .unwrap();
        assert!(w.is_link(&record.abs_path));
        assert!(i.link_boundaries().contains(&shared), "the shared folder is a boundary too");
    }

    #[test]
    fn a_link_to_a_model_that_is_not_in_the_vault_is_refused() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: "A".repeat(64),
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_link_to_a_vault_file_that_vanished_is_refused_rather_than_left_dangling() {
        // A dangling link is worse than no link: ComfyUI shows the model and
        // then fails to load it.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        std::fs::remove_file(w.vault_root.join("loras/lora1.safetensors")).unwrap();

        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.contains("point at nothing"));
    }

    #[test]
    fn a_name_windows_cannot_store_is_refused() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        for bad in ["CON.safetensors", "a/b.safetensors", "", "trailing.safetensors "] {
            let err = links(&w)
                .create(&CreateLinkRequest {
                    install_id: i.id.clone(),
                    sha256: sha.clone(),
                    relative_dir: "models/loras".into(), dir: None,
                    link_name: Some(bad.into()),
                    create_dir: true,
                })
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidArgument, "{bad} was allowed");
        }
    }

    #[test]
    fn removing_a_link_leaves_the_vault_file_alone() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let vault_file = w.vault_root.join("loras/lora1.safetensors");

        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap();

        links(&w).remove(&record.id).unwrap();
        assert!(!record.abs_path.exists());
        assert!(vault_file.is_file(), "the vault file must survive");
        assert_eq!(std::fs::read(&vault_file).unwrap(), weights("lora1"));
        assert!(w.store.link(&record.id).unwrap().is_none());
    }

    #[test]
    fn removing_a_link_that_a_real_file_replaced_is_refused() {
        // Deleting it would destroy something the engine did not create.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap();

        std::fs::remove_file(&record.abs_path).unwrap();
        std::fs::write(&record.abs_path, b"somebody put a real file here").unwrap();

        let err = links(&w).remove(&record.id).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&record.abs_path).unwrap(), b"somebody put a real file here");
    }

    #[test]
    fn removing_a_link_that_is_already_gone_just_forgets_the_record() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap();
        std::fs::remove_file(&record.abs_path).unwrap();

        links(&w).remove(&record.id).unwrap();
        assert!(w.store.link(&record.id).unwrap().is_none());
    }

    #[test]
    fn the_state_of_a_link_is_read_from_the_disk() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let record = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap();

        assert_eq!(links(&w).state_of(&record), LinkState::Ok);

        // The worst state, and the one the interface has to show first.
        std::fs::remove_file(w.vault_root.join("loras/lora1.safetensors")).unwrap();
        assert_eq!(links(&w).state_of(&record), LinkState::Dangling);

        std::fs::remove_file(&record.abs_path).unwrap();
        assert_eq!(links(&w).state_of(&record), LinkState::Missing);

        std::fs::write(&record.abs_path, b"real file").unwrap();
        assert_eq!(links(&w).state_of(&record), LinkState::Replaced);
    }

    #[test]
    fn links_can_be_listed_by_install_by_model_and_by_state() {
        let w = TestWorld::new();
        let a = w.add_install("A");
        let b = w.add_install("B");
        let sha1 = vault_a_file(&w, "one", "loras", "one.safetensors");
        let sha2 = vault_a_file(&w, "two", "loras", "two.safetensors");

        for (install, sha) in [(&a, &sha1), (&a, &sha2), (&b, &sha1)] {
            links(&w)
                .create(&CreateLinkRequest {
                    install_id: install.id.clone(),
                    sha256: sha.clone(),
                    relative_dir: "models/loras".into(), dir: None,
                    link_name: None,
                    create_dir: true,
                })
                .unwrap();
        }

        assert_eq!(links(&w).list(None, None, None).unwrap().len(), 3);
        assert_eq!(links(&w).list(Some(&a.id), None, None).unwrap().len(), 2);
        assert_eq!(links(&w).list(None, Some(&sha1), None).unwrap().len(), 2);
        assert_eq!(links(&w).list(None, None, Some(LinkState::Ok)).unwrap().len(), 3);
        assert_eq!(links(&w).list(None, None, Some(LinkState::Dangling)).unwrap().len(), 0);
    }

    #[test]
    fn creating_a_folder_reports_whether_it_was_new() {
        let w = TestWorld::new();
        let i = w.add_install("A");

        let (path, created) = links(&w).create_folder(&i.id, "models/loras/new-style").unwrap();
        assert!(created);
        assert!(path.is_dir());

        let (_, created_again) = links(&w).create_folder(&i.id, "models/loras/new-style").unwrap();
        assert!(!created_again);
    }

    #[test]
    fn creating_a_folder_outside_the_boundary_is_refused_and_nothing_is_made() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let err = links(&w).create_folder(&i.id, "comfy/sneaky").unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
        // The folder is created before the boundary is proved, so that the
        // proof runs against the real resolved location. It must not survive.
        assert!(!i.root.join("comfy/sneaky").exists(), "a refused folder was left behind");
    }

    #[test]
    fn the_folder_picker_lists_the_model_roots_with_their_contents() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        w.write_model(&i, "models/loras/a.safetensors", &weights("a"));
        w.write_model(&i, "models/loras/style/b.safetensors", &weights("b"));
        w.write_model(&i, "models/checkpoints/c.safetensors", &weights("c"));

        let dirs = links(&w).model_dirs(&i.id).unwrap();
        let models = dirs.iter().find(|d| d.rel_path == "models").expect("the models root");

        let loras = models.children.iter().find(|c| c.rel_path.ends_with("loras")).unwrap();
        assert_eq!(loras.file_count, 1);
        assert_eq!(loras.children.len(), 1, "the nested folder is offered too");
        assert_eq!(loras.children[0].file_count, 1);

        let ckpt = models.children.iter().find(|c| c.rel_path.ends_with("checkpoints")).unwrap();
        assert_eq!(ckpt.file_count, 1);
    }

    #[test]
    fn an_extra_model_path_cannot_widen_the_boundary_to_the_whole_install() {
        // A line like `base_path: C:\ComfyUI` with `loras: .` makes the whole
        // install a declared model folder. custom_nodes is then inside the
        // boundary, and ComfyUI imports custom_nodes/<pack>/__init__.py at
        // startup, so a link written there is on the import path.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let i = w.add_extra_model_path(&i, "loras", &i.root.clone());
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        assert!(
            !i.link_boundaries().contains(&i.root),
            "the install root is not a model folder: {:?}",
            i.link_boundaries()
        );

        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id.clone(),
                sha256: sha,
                relative_dir: "custom_nodes/EvilPack".into(), dir: None,
                link_name: Some("lora1.safetensors".into()),
                create_dir: true,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
        assert!(!i.root.join("custom_nodes/EvilPack/lora1.safetensors").exists());
    }

    #[test]
    fn a_link_name_has_to_be_a_model_file() {
        // The second layer, independent of the boundary. A link this engine
        // creates is always a model, so a name ComfyUI would import instead is
        // refused whatever folder it was aimed at.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");

        for bad in ["__init__.py", "evil.dll", "run.bat", "notes.txt"] {
            let err = links(&w)
                .create(&CreateLinkRequest {
                    install_id: i.id.clone(),
                    sha256: sha.clone(),
                    relative_dir: "models/loras".into(), dir: None,
                    link_name: Some(bad.into()),
                    create_dir: true,
                })
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidArgument, "{bad} was allowed");
            assert!(!i.root.join("models/loras").join(bad).exists());
        }

        // The control, so this cannot pass by refusing everything.
        assert!(links(&w)
            .create(&CreateLinkRequest {
                install_id: i.id,
                sha256: sha,
                relative_dir: "models/loras".into(), dir: None,
                link_name: Some("renamed.safetensors".into()),
                create_dir: true,
            })
            .is_ok());
    }

    #[test]
    fn the_folder_picker_never_offers_a_folder_a_link_cannot_go_in() {
        // The person picks a folder the product showed them. Offering one the
        // engine would refuse is bad; offering one it would accept but should
        // not is worse.
        let w = TestWorld::new();
        let i = w.add_install("A");
        std::fs::create_dir_all(i.root.join("custom_nodes/SomePack")).unwrap();
        let i = w.add_extra_model_path(&i, "loras", &i.root.clone());

        let offered = links(&w).model_dirs(&i.id).unwrap();
        fn every_path(nodes: &[ModelDirNode], out: &mut Vec<PathBuf>) {
            for n in nodes {
                out.push(n.abs_path.clone());
                every_path(&n.children, out);
            }
        }
        let mut paths = Vec::new();
        every_path(&offered, &mut paths);

        assert!(!paths.is_empty(), "the picker offered nothing at all");
        let boundaries = i.link_boundaries();
        for p in &paths {
            assert!(
                boundaries.iter().any(|b| p.starts_with(b)),
                "the picker offered {p:?}, which is outside every boundary"
            );
        }
        assert!(
            !paths.iter().any(|p| p.starts_with(i.root.join("custom_nodes"))),
            "the picker offered custom_nodes"
        );
    }

    #[test]
    fn a_link_for_an_install_that_is_gone_is_refused() {
        let w = TestWorld::new();
        let err = links(&w)
            .create(&CreateLinkRequest {
                install_id: "never-registered".into(),
                sha256: "A".repeat(64),
                relative_dir: "models/loras".into(), dir: None,
                link_name: None,
                create_dir: true,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    // --- a folder chosen by its full path -----------------------------------

    fn yaml_install(w: &TestWorld) -> (Install, PathBuf) {
        let i = w.add_install("A");
        let shared = w.path().join("other-drive");
        let i = w.add_extra_model_path(&i, "loras", &shared);
        (i, shared)
    }

    fn by_dir(i: &Install, sha: &str, dir: PathBuf, create: bool) -> CreateLinkRequest {
        CreateLinkRequest {
            install_id: i.id.clone(),
            sha256: sha.to_string(),
            relative_dir: String::new(),
            dir: Some(dir),
            link_name: None,
            create_dir: create,
        }
    }

    #[test]
    fn a_link_can_go_in_a_yaml_folder_outside_the_install_and_the_choice_is_kept() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let dir = shared.join("portraits");
        let record = links(&w).create(&by_dir(&i, &sha, dir.clone(), true)).unwrap();
        assert!(w.is_link(&dir.join("lora1.safetensors")));
        assert_eq!(w.read(&record.abs_path), weights("lora1"));
        assert_eq!(w.store.link_dir(&i.id, "loras").unwrap(), Some(dir.clone()));
        // The chooser opened from the Library starts there next time.
        let list = links(&w).link_folders(&i.id, "loras", None).unwrap();
        assert_eq!(list.default_dir, Some(crate::paths::display_path(&dir)));
    }

    #[test]
    fn a_full_path_folder_for_another_kind_or_outside_is_refused_and_nothing_is_made() {
        let w = TestWorld::new();
        let (i, _) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        for bad in [
            i.root.join("models/checkpoints/x"),
            i.root.join("custom_nodes/pack"),
            i.root.join("models/loras/../../custom_nodes/pack"),
            w.path().join("elsewhere"),
        ] {
            let err = links(&w).create(&by_dir(&i, &sha, bad.clone(), true)).unwrap_err();
            assert_eq!(err.code, ErrorCode::PathOutsideBoundary, "{bad:?}: {err:?}");
            assert!(!bad.exists(), "{bad:?} was made");
        }
        let both = CreateLinkRequest { relative_dir: "models/loras".into(), ..by_dir(&i, &sha, i.root.join("models/loras"), true) };
        assert_eq!(links(&w).create(&both).unwrap_err().code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_chooser_lists_the_roots_and_their_folders_and_nothing_else() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        std::fs::create_dir_all(i.root.join("models/loras/portraits/deeper")).unwrap();
        std::fs::write(i.root.join("models/loras/a.safetensors"), b"x").unwrap();

        let list = links(&w).link_folders(&i.id, "loras", None).unwrap();
        assert_eq!(
            list.default_dir.map(|d| crate::paths::compare_key(Path::new(&d))),
            Some(crate::paths::compare_key(&i.root.join("models").join("loras")))
        );
        let roots = list.folders;
        let paths: Vec<&PathBuf> = roots.iter().map(|r| &r.path).collect();
        assert!(paths.contains(&&i.root.join("models/loras")));
        assert!(paths.contains(&&shared));
        let models = roots.iter().find(|r| r.path == i.root.join("models/loras")).unwrap();
        assert!(models.has_subfolders);

        let inside = links(&w).link_folders(&i.id, "loras", Some(&i.root.join("models/loras"))).unwrap().folders;
        assert_eq!(inside.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["portraits"], "folders only");
        assert!(inside[0].has_subfolders);

        for bad in [i.root.clone(), i.root.join("custom_nodes"), i.root.join("models/checkpoints"), w.path().to_path_buf()] {
            let err = links(&w).link_folders(&i.id, "loras", Some(&bad)).unwrap_err();
            assert_eq!(err.code, ErrorCode::PathOutsideBoundary, "{bad:?}");
        }
    }

    #[test]
    fn the_chooser_makes_one_folder_inside_a_root_and_nowhere_else() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let (path, made) = links(&w).make_link_folder(&i.id, "loras", &shared.join("new")).unwrap();
        assert!(made && path.is_dir());
        assert!(!links(&w).make_link_folder(&i.id, "loras", &shared.join("new")).unwrap().1, "already there");
        for bad in [
            i.root.join("custom_nodes/evil"),
            shared.join("a/b"),
            w.path().join("elsewhere"),
            shared.join("CON"),
        ] {
            assert!(links(&w).make_link_folder(&i.id, "loras", &bad).is_err(), "{bad:?}");
            assert!(!bad.exists(), "{bad:?} was made");
        }
    }

    // --- a link made by hand is journaled -------------------------------------

    fn journal_of(w: &TestWorld) -> Vec<crate::store::JournalEntry> {
        let id = w.store.journal_ids().unwrap().into_iter().find(|i| i.starts_with(LINK_JOURNAL_PREFIX)).expect("a journal");
        w.store.journal(&id).unwrap()
    }

    #[test]
    fn a_link_made_by_hand_journals_its_folder_and_itself() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        links(&w).create(&by_dir(&i, &sha, shared.join("portraits"), true)).unwrap();
        let steps: Vec<(&str, JournalState)> = journal_of(&w)
            .iter()
            .map(|e| match e.step {
                JournalStep::CreateDir { .. } => ("folder", e.state),
                JournalStep::CreateLink { .. } => ("link", e.state),
                _ => ("other", e.state),
            })
            .collect();
        assert_eq!(steps, vec![("folder", JournalState::Done), ("link", JournalState::Done)]);
    }

    #[test]
    fn a_crash_between_the_link_and_its_record_is_finished_when_the_vault_opens() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let dir = shared.join("portraits");
        w.store.fault_next_link_write(crate::store::LinkWriteFault::Crash);
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            links(&w).create(&by_dir(&i, &sha, dir.clone(), true))
        }));
        assert!(crashed.is_err(), "the crash did not happen");
        let link = dir.join("lora1.safetensors");
        assert!(w.is_link(&link), "the link was made");
        assert!(w.store.link_at_path(&link).unwrap().is_none(), "and its record was not");

        assert_eq!(links(&w).finish_interrupted().unwrap(), 1);
        let rec = w.store.link_at_path(&link).unwrap().expect("the record is written for the link");
        assert_eq!(rec.sha256, sha);
        assert_eq!(rec.install_id, i.id);
        assert!(journal_of(&w).iter().all(|e| e.state == JournalState::Done));
        assert_eq!(links(&w).finish_interrupted().unwrap(), 0, "a second pass finds nothing to do");
    }

    #[test]
    fn a_record_the_database_refuses_takes_its_link_away_too() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let dir = shared.join("portraits");
        w.store.fault_next_link_write(crate::store::LinkWriteFault::Refuse);
        assert_eq!(links(&w).create(&by_dir(&i, &sha, dir.clone(), true)).unwrap_err().code, ErrorCode::StoreError);
        assert!(std::fs::symlink_metadata(dir.join("lora1.safetensors")).is_err(), "a link with no record was left");
    }

    #[test]
    fn a_pending_step_whose_path_holds_something_else_is_left_alone() {
        let w = TestWorld::new();
        let (i, shared) = yaml_install(&w);
        let sha = vault_a_file(&w, "lora1", "loras", "lora1.safetensors");
        let dir = shared.join("portraits");
        w.store.fault_next_link_write(crate::store::LinkWriteFault::Crash);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| links(&w).create(&by_dir(&i, &sha, dir.clone(), true))));
        // The person replaced the half-made link with a file of their own.
        let link = dir.join("lora1.safetensors");
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, b"theirs").unwrap();
        assert_eq!(links(&w).finish_interrupted().unwrap(), 0);
        assert!(w.store.link_at_path(&link).unwrap().is_none());
        assert_eq!(std::fs::read(&link).unwrap(), b"theirs");
    }
}

