//! Where a run may touch the disk.
//!
//! Every path an apply, an undo or a recovery acts on comes from the vault's
//! database: the journal, the stored plan, the stored installs. That file
//! travels with the vault, so it can come from another computer or from
//! someone else, and a row in it is a claim, not a fact. A row naming a file in
//! the person's documents made an undo delete that file, and a row naming the
//! Windows Startup folder made a recovery write a script there.
//!
//! So before any step runs, every path it names is proved to be one of the two
//! kinds of place a run can produce:
//!
//! * inside the vault, and never inside its own database folder;
//! * inside a folder the scan walks for an install that is registered in this
//!   vault and is a ComfyUI install on this disk now, or inside one of its
//!   category folders where that folder really is, and never inside that
//!   install's `custom_nodes`.
//!
//! A file a run moves, links, or puts back must also be a model file by its
//! name: a plain file name with one of the model extensions built into the
//! engine. Not the list in the vault's settings, which sit in the same
//! database a crafted vault writes: adding `.txt` there let `notes.txt`
//! through again. Nothing in the app changes that list, so nothing a real run
//! did is refused.
//!
//! A run that names any other place is refused whole, before a file is touched.

use std::path::{Path, PathBuf};

use crate::error::{ErrorCode, Result, VaultError};
use crate::install::Install;
use crate::plan::PlanGroup;
use crate::store::{JournalEntry, JournalStep, Store};

use super::fsops::STASH_SUFFIX;

/// The places a run may touch, read from the disk now.
pub struct Places {
    vault_root: PathBuf,
    internal: PathBuf,
    /// Every folder the scan walks, per proved install, with that install's
    /// `custom_nodes`, which is never touched.
    install_roots: Vec<(PathBuf, PathBuf)>,
}

impl Places {
    /// The vault, and every registered install that is still a ComfyUI
    /// install. An install that is not is left out, so a path inside it fails.
    pub fn read(store: &Store) -> Result<Self> {
        let mut install_roots = Vec::new();
        for stored in store.installs()? {
            let Ok(install) = stored.proved() else { continue };
            install_roots.extend(roots_of(&install));
            // A category folder that is a junction elsewhere is still where
            // the install keeps that category, and a scan offers its files.
            let custom_nodes = install.custom_nodes_dir();
            install_roots.extend(
                install
                    .category_folders(true, true, store.vault_root())
                    .into_iter()
                    .map(|(path, _)| (path, custom_nodes.clone())),
            );
        }
        Ok(Self {
            vault_root: store.vault_root().to_path_buf(),
            internal: store.internal_dir(),
            install_roots,
        })
    }

    /// Is `path` inside the vault, outside its database folder?
    ///
    /// The last part of the path is never followed: a place in an install is
    /// often one of the vault's own links, and following it would land in the
    /// vault.
    pub fn in_vault(&self, path: &Path) -> bool {
        match crate::paths::resolve_new_path_within(&self.vault_root, path) {
            Ok(real) => !crate::paths::resolve_new_path_within(&self.internal, &real).is_ok()
                && !same(&real, &self.internal),
            Err(_) => false,
        }
    }

    /// Is `path` inside a folder the scan walks for a proved install, and not
    /// inside that install's `custom_nodes`?
    ///
    /// Never inside the vault, even when an install's extra model folders
    /// reach into it: a place in an install is one a vault file's copy came
    /// from, and a vault file is not a copy of itself.
    pub fn in_an_install(&self, path: &Path) -> bool {
        crate::paths::resolve_new_path_within(&self.vault_root, path).is_err()
            && self.install_roots.iter().any(|(root, custom_nodes)| {
                crate::paths::resolve_new_path_within(root, path).is_ok()
                    && crate::paths::resolve_new_path_within(custom_nodes, path).is_err()
            })
    }

    /// Is the last part of `path` a model file's name: a plain name, with one
    /// of the engine's own model extensions?
    pub fn is_model_name(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            return false;
        };
        let lower = name.to_lowercase();
        crate::paths::validate_file_name(&name).is_ok()
            && crate::settings::DEFAULT_EXTENSIONS.iter().any(|e| lower.ends_with(e))
    }

    /// A model file inside a proved install.
    fn model_in_an_install(&self, path: &Path) -> bool {
        self.is_model_name(path) && self.in_an_install(path)
    }

    /// A model file inside the vault.
    fn model_in_vault(&self, path: &Path) -> bool {
        self.is_model_name(path) && self.in_vault(path)
    }

    /// The paths of one journal step that are not places a run may touch.
    pub fn refused_in_step(&self, step: &JournalStep) -> Vec<PathBuf> {
        let mut bad = Vec::new();
        let mut want = |ok: bool, p: &Path| {
            if !ok {
                bad.push(p.to_path_buf());
            }
        };
        match step {
            JournalStep::CreateDir { path } => {
                want(self.in_vault(path) || self.in_an_install(path), path);
            }
            JournalStep::MoveToVault { from, to, .. } => {
                want(self.model_in_an_install(from), from);
                want(self.model_in_vault(to), to);
            }
            JournalStep::StashOriginal { path, stash } => {
                want(self.model_in_an_install(path), path);
                if !stash.as_os_str().is_empty() {
                    want(is_stash_of(path, stash), stash);
                }
            }
            JournalStep::CreateLink { link, target } | JournalStep::RemoveLink { link, target } => {
                // A link in an install, or a second name beside a vault file.
                want(self.model_in_an_install(link) || self.model_in_vault(link), link);
                want(self.model_in_vault(target), target);
            }
            JournalStep::DeleteStash { stash, original, vault_path, .. } => {
                want(self.model_in_an_install(original), original);
                want(is_stash_of(original, stash), stash);
                want(self.model_in_vault(vault_path), vault_path);
            }
            JournalStep::DeleteVaultFile { path, .. } => {
                want(self.model_in_vault(path), path);
            }
        }
        bad
    }

    /// The paths of a set of journal steps that are not places a run may touch.
    pub fn refused_in_steps<'e>(&self, entries: impl IntoIterator<Item = &'e JournalEntry>) -> Vec<PathBuf> {
        let mut bad: Vec<PathBuf> = entries.into_iter().flat_map(|e| self.refused_in_step(&e.step)).collect();
        bad.sort();
        bad.dedup();
        bad
    }

    /// The paths of a stored plan group that are not places a run may touch.
    pub fn refused_in_group(&self, group: &PlanGroup) -> Vec<PathBuf> {
        let mut bad: Vec<PathBuf> = std::iter::once(&group.source.abs_path)
            .chain(group.links.iter().map(|l| &l.abs_path))
            .filter(|p| !self.model_in_an_install(p))
            .cloned()
            .collect();
        bad.sort();
        bad.dedup();
        bad
    }
}

/// The refusal for a run that names places outside the vault and the installs.
pub fn refusal(bad: &[PathBuf]) -> VaultError {
    VaultError::new(
        ErrorCode::PathOutsideBoundary,
        "This run names files outside the vault and the registered installs, so nothing was touched.",
    )
    .with_detail(bad.iter().map(|p| crate::paths::display_path(p)).collect::<Vec<_>>().join(", "))
    .with_path(&bad[0])
}

fn roots_of(install: &Install) -> Vec<(PathBuf, PathBuf)> {
    let custom_nodes = install.custom_nodes_dir();
    install
        .scan_roots(true, true)
        .into_iter()
        .map(|r| (r.path, custom_nodes.clone()))
        .collect()
}

/// A set-aside name is the original's name with the stash suffix, in the same
/// folder. Nothing else can be one.
pub fn is_stash_of(original: &Path, stash: &Path) -> bool {
    let (Some(name), Some(stash_name)) = (original.file_name(), stash.file_name()) else {
        return false;
    };
    let prefix = format!("{}{STASH_SUFFIX}-", name.to_string_lossy());
    if !stash_name.to_string_lossy().starts_with(&prefix) {
        return false;
    }
    match (original.parent(), stash.parent()) {
        (Some(a), Some(b)) => match (crate::paths::canonicalize_clean(a), crate::paths::canonicalize_clean(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
        _ => false,
    }
}

fn same(a: &Path, b: &Path) -> bool {
    crate::paths::canonicalize_clean(a).ok() == crate::paths::canonicalize_clean(b).ok()
}
