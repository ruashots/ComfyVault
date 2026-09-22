//! Looking after what is already in the vault.
//!
//! Listing it, tidying the names, finding what nothing points at any more, and
//! checking that the links out in the installs still work.
//!
//! # Why one content can carry several names
//!
//! The vault keeps one real file per unique content. When the same weights
//! arrived under a second name, that name is kept as a link beside the real
//! file, so the vault shows every name the model was ever known by and a saved
//! workflow that used the other name keeps working.
//!
//! The person can change which name the vault keeps as the real file, and can
//! remove a name. Removing is a separate action on purpose: names are cheap and
//! a removed name can break a workflow nobody has opened for months.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apply::fsops;
use crate::error::{ErrorCode, Result, VaultError};
use crate::links::Links;
use crate::platform::Platform;
use crate::progress::CancelToken;
use crate::store::{LinkRecord, LinkState, Store, VaultFileRecord};

/// One content in the vault, with everything the interface shows about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFile {
    pub sha256: String,
    pub canonical_name: String,
    pub category: String,
    pub vault_rel_path: PathBuf,
    pub size_bytes: u64,
    pub added_at: crate::time_util::Timestamp,
    pub aliases: Vec<String>,
    pub link_count: u64,
    pub links: Vec<LinkRecord>,
    pub metadata: Option<crate::metadata::ModelMetadata>,
    /// The real file is on the disk.
    pub present: bool,
}

/// One name a content carries inside the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultName {
    pub name: String,
    pub is_canonical: bool,
    pub vault_rel_path: PathBuf,
    /// Install links that resolve through this name.
    pub used_by_links: u64,
    pub seen_in_installs: Vec<String>,
}

/// A content that carries more than one name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameGroup {
    pub sha256: String,
    pub size_bytes: u64,
    pub category: String,
    pub canonical_name: String,
    pub names: Vec<VaultName>,
}

/// What the health check found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultHealth {
    pub checked_links: u64,
    pub checked_files: u64,
    /// The link is there and it points at nothing. The most serious result.
    pub dangling_links: Vec<LinkRecord>,
    /// A real file sits where a link belonged.
    pub replaced_links: Vec<LinkRecord>,
    /// Recorded in the database, absent from the disk.
    pub missing_vault_files: Vec<VaultFile>,
    /// Files in the vault folder the database does not know about.
    pub foreign_files: Vec<String>,
    pub ok: bool,
}

/// How to narrow a listing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFilter {
    pub category: Option<String>,
    pub name_contains: Option<String>,
    pub min_size_bytes: Option<u64>,
    #[serde(default)]
    pub orphans_only: bool,
    #[serde(default)]
    pub with_aliases_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VaultSort {
    Name,
    Size,
    AddedAt,
    LinkCount,
}

/// The largest page a listing returns.
pub const MAX_PAGE: usize = 1000;

/// A page of vault files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultPage {
    pub total: u64,
    pub offset: u64,
    pub files: Vec<VaultFile>,
}

pub struct Vault<'a> {
    store: &'a Store,
    platform: &'a dyn Platform,
}

impl<'a> Vault<'a> {
    pub fn new(store: &'a Store, platform: &'a dyn Platform) -> Self {
        Self { store, platform }
    }

    fn links(&self) -> Links<'a> {
        Links::new(self.store, self.platform)
    }

    /// Everything the interface shows about one content.
    pub fn file(&self, sha256: &str) -> Result<Option<VaultFile>> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(VaultError::invalid("That is not a file hash."));
        };
        let Some(record) = self.store.vault_file(&sha)? else {
            return Ok(None);
        };
        Ok(Some(self.decorate(&record)?))
    }

    fn decorate(&self, record: &VaultFileRecord) -> Result<VaultFile> {
        let links = self.live_links(&record.sha256)?;
        let path = self.store.vault_root().join(record.vault_rel_path());
        Ok(VaultFile {
            sha256: record.sha256.clone(),
            canonical_name: record.canonical_name.clone(),
            category: record.category.clone(),
            vault_rel_path: record.vault_rel_path(),
            size_bytes: record.size_bytes,
            added_at: record.added_at,
            aliases: record.aliases.clone(),
            link_count: links.len() as u64,
            links,
            metadata: self.store.metadata(&record.sha256)?,
            present: path.is_file(),
        })
    }

    /// The links for a content that are really on the disk right now.
    ///
    /// A recorded link the person deleted by hand no longer counts, or the
    /// orphan list would never show anything.
    fn live_links(&self, sha256: &str) -> Result<Vec<LinkRecord>> {
        let links = self.links();
        Ok(self
            .store
            .links_for_hash(sha256)?
            .into_iter()
            .filter(|l| {
                matches!(links.state_of(l), LinkState::Ok | LinkState::Dangling)
            })
            .collect())
    }

    /// A page of the vault's contents.
    pub fn list(
        &self,
        offset: u64,
        limit: u64,
        filter: &VaultFilter,
        sort: VaultSort,
        descending: bool,
    ) -> Result<VaultPage> {
        let mut files: Vec<VaultFile> = Vec::new();
        for record in self.store.vault_files()? {
            files.push(self.decorate(&record)?);
        }

        files.retain(|f| {
            if let Some(c) = &filter.category {
                if &f.category != c {
                    return false;
                }
            }
            if let Some(n) = &filter.name_contains {
                let needle = n.to_lowercase();
                let hit = f.canonical_name.to_lowercase().contains(&needle)
                    || f.aliases.iter().any(|a| a.to_lowercase().contains(&needle));
                if !hit {
                    return false;
                }
            }
            if let Some(m) = filter.min_size_bytes {
                if f.size_bytes < m {
                    return false;
                }
            }
            if filter.orphans_only && f.link_count > 0 {
                return false;
            }
            if filter.with_aliases_only && f.aliases.is_empty() {
                return false;
            }
            true
        });

        files.sort_by(|a, b| match sort {
            VaultSort::Name => a.canonical_name.to_lowercase().cmp(&b.canonical_name.to_lowercase()),
            VaultSort::Size => a.size_bytes.cmp(&b.size_bytes),
            VaultSort::AddedAt => a.added_at.cmp(&b.added_at),
            VaultSort::LinkCount => a.link_count.cmp(&b.link_count),
        });
        if descending {
            files.reverse();
        }

        let total = files.len() as u64;
        let limit = (limit as usize).min(MAX_PAGE);
        let page: Vec<VaultFile> = files.into_iter().skip(offset as usize).take(limit).collect();
        Ok(VaultPage { total, offset, files: page })
    }

    /// Contents that carry more than one name.
    pub fn name_groups(&self) -> Result<Vec<NameGroup>> {
        let mut out = Vec::new();
        for record in self.store.vault_files()? {
            if record.aliases.is_empty() {
                continue;
            }
            let links = self.live_links(&record.sha256)?;
            let names = record
                .all_names()
                .into_iter()
                .map(|name| {
                    let rel = PathBuf::from(&record.category).join(&name);
                    let through: Vec<&LinkRecord> = links
                        .iter()
                        .filter(|l| l.vault_rel_path == rel)
                        .collect();
                    VaultName {
                        is_canonical: name == record.canonical_name,
                        used_by_links: through.len() as u64,
                        seen_in_installs: {
                            let mut v: Vec<String> =
                                through.iter().map(|l| l.install_id.clone()).collect();
                            v.sort();
                            v.dedup();
                            v
                        },
                        vault_rel_path: rel,
                        name,
                    }
                })
                .collect();

            out.push(NameGroup {
                sha256: record.sha256.clone(),
                size_bytes: record.size_bytes,
                category: record.category.clone(),
                canonical_name: record.canonical_name.clone(),
                names,
            });
        }
        out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        Ok(out)
    }

    /// Makes one of a content's names the real file.
    ///
    /// The previous real name becomes a link beside it, and every install link
    /// is repointed at the new real path, so nothing ever resolves through two
    /// links in a row. Each step is journaled, so an interruption can be undone.
    pub fn set_canonical_name(&self, sha256: &str, name: &str) -> Result<VaultFile> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(VaultError::invalid("That is not a file hash."));
        };
        let mut record = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;

        if record.canonical_name == name {
            return self.file(&sha)?.ok_or_else(|| VaultError::not_found("That model is not in the vault."));
        }
        if !record.aliases.iter().any(|a| a == name) {
            return Err(VaultError::invalid(
                "That name does not belong to this model. Choose one of the names it already has.",
            ));
        }

        let root = self.store.vault_root();
        let old_path = root.join(&record.category).join(&record.canonical_name);
        let new_path = root.join(&record.category).join(name);

        if !old_path.is_file() {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "The vault no longer holds that file, so its name cannot be changed.",
            )
            .with_path(&old_path));
        }

        let apply_id = format!("rename-{}", uuid::Uuid::new_v4().simple());
        let mut seq = 0u64;
        let mut journal = |step: crate::store::JournalStep| -> Result<()> {
            let e = crate::store::JournalEntry {
                apply_id: apply_id.clone(),
                seq,
                group_id: sha.clone(),
                step,
                state: crate::store::JournalState::Done,
                started_at: crate::time_util::Timestamp::now(),
                finished_at: Some(crate::time_util::Timestamp::now()),
                error: None,
            };
            seq += 1;
            self.store.append_journal(&e)
        };

        // The chosen name is currently a link beside the real file. Remove it
        // so the real file can take that name.
        if self.platform.is_symlink(&new_path) {
            self.platform.remove_symlink(&new_path)?;
            journal(crate::store::JournalStep::RemoveLink {
                link: new_path.clone(),
                target: old_path.clone(),
            })?;
        } else if new_path.exists() {
            return Err(VaultError::conflict(
                "A different file already has that name in the vault.",
            )
            .with_path(&new_path));
        }

        std::fs::rename(&old_path, &new_path)
            .map_err(|e| VaultError::from_io(&e, &old_path, "changing the name the vault keeps"))?;
        journal(crate::store::JournalStep::MoveToVault {
            from: old_path.clone(),
            to: new_path.clone(),
            copied: false,
            sha256: sha.clone(),
            size_bytes: record.size_bytes,
        })?;

        // The old name stays, as a link, so a workflow that used it still works.
        self.platform.create_file_symlink(&old_path, &new_path)?;
        journal(crate::store::JournalStep::CreateLink {
            link: old_path.clone(),
            target: new_path.clone(),
        })?;

        // Repoint every install link, so none of them resolves through a link.
        let old_rel = PathBuf::from(&record.category).join(&record.canonical_name);
        let new_rel = PathBuf::from(&record.category).join(name);
        for mut link in self.store.links_for_hash(&sha)? {
            if link.vault_rel_path != old_rel {
                continue;
            }
            if self.platform.is_symlink(&link.abs_path) {
                self.platform.remove_symlink(&link.abs_path)?;
                journal(crate::store::JournalStep::RemoveLink {
                    link: link.abs_path.clone(),
                    target: old_path.clone(),
                })?;
                self.platform.create_file_symlink(&link.abs_path, &new_path)?;
                journal(crate::store::JournalStep::CreateLink {
                    link: link.abs_path.clone(),
                    target: new_path.clone(),
                })?;
            }
            link.vault_rel_path = new_rel.clone();
            self.store.put_link(&link)?;
        }

        let previous = record.canonical_name.clone();
        record.aliases.retain(|a| a != name);
        record.aliases.push(previous);
        record.aliases.sort();
        record.canonical_name = name.to_string();
        self.store.put_vault_file(&record)?;

        self.decorate(&record)
    }

    /// Removes one of a content's names from the vault.
    ///
    /// Refuses the real file's own name, and refuses a name an install link
    /// still resolves through.
    pub fn remove_alias(&self, sha256: &str, name: &str) -> Result<()> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(VaultError::invalid("That is not a file hash."));
        };
        let mut record = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;

        if record.canonical_name == name {
            return Err(VaultError::conflict(
                "That is the name the vault keeps. Choose another name to keep first, then remove this one.",
            ));
        }
        if !record.aliases.iter().any(|a| a == name) {
            return Err(VaultError::not_found("That name is not one this model has."));
        }

        let rel = PathBuf::from(&record.category).join(name);
        let through: Vec<LinkRecord> = self
            .live_links(&sha)?
            .into_iter()
            .filter(|l| l.vault_rel_path == rel)
            .collect();
        if !through.is_empty() {
            let where_ = through
                .iter()
                .map(|l| crate::paths::display_path(&l.abs_path))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(VaultError::conflict(
                "Some installs still use that name, so it was kept. Remove those links first.",
            )
            .with_detail(where_));
        }

        let path = self.store.vault_root().join(&rel);
        if self.platform.is_symlink(&path) {
            self.platform.remove_symlink(&path)?;
        } else if path.exists() {
            return Err(VaultError::conflict(
                "A real file has that name in the vault, not a link, so nothing was removed.",
            )
            .with_path(&path));
        }

        record.aliases.retain(|a| a != name);
        self.store.put_vault_file(&record)?;
        Ok(())
    }

    /// Vault files no install links to.
    pub fn orphans(&self) -> Result<Vec<VaultFile>> {
        let mut out: Vec<VaultFile> = Vec::new();
        for record in self.store.vault_files()? {
            let f = self.decorate(&record)?;
            if f.link_count == 0 {
                out.push(f);
            }
        }
        out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        Ok(out)
    }

    /// Deletes a vault file and every name it carries.
    ///
    /// **This cannot be undone.** The bytes are gone, so it is refused while
    /// any install still links to the file, and it needs the caller to repeat
    /// the hash back as a confirmation.
    pub fn delete_file(&self, sha256: &str, confirm: &str) -> Result<u64> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(VaultError::invalid("That is not a file hash."));
        };
        if crate::scan::hash::normalize_sha256(confirm).as_deref() != Some(sha.as_str()) {
            return Err(VaultError::invalid(
                "This delete was not confirmed, so nothing was removed.",
            ));
        }
        let record = self
            .store
            .vault_file(&sha)?
            .ok_or_else(|| VaultError::not_found("That model is not in the vault."))?;

        let links = self.live_links(&sha)?;
        if !links.is_empty() {
            return Err(VaultError::conflict(
                "Some installs still link to that model, so it was kept. Remove those links first.",
            )
            .with_detail(
                links
                    .iter()
                    .map(|l| crate::paths::display_path(&l.abs_path))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        let root = self.store.vault_root();
        // The names go first, so a failure part way leaves no name pointing at
        // a file that is about to disappear.
        for alias in &record.aliases {
            let p = root.join(&record.category).join(alias);
            if self.platform.is_symlink(&p) {
                self.platform.remove_symlink(&p)?;
            }
        }
        let path = root.join(record.vault_rel_path());
        let freed = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(record.size_bytes);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| VaultError::from_io(&e, &path, "deleting the file from the vault"))?;
        }
        self.store.delete_vault_file(&sha)?;
        Ok(freed)
    }

    /// Checks that the vault and the links out in the installs still agree.
    pub fn health(&self) -> Result<VaultHealth> {
        let links = self.links();
        let all_links = self.store.links()?;
        let mut dangling = Vec::new();
        let mut replaced = Vec::new();

        for l in &all_links {
            match links.state_of(l) {
                // The worst one: ComfyUI lists a dangling link in its model
                // menu and then fails to load it, and a custom node that
                // re-downloads the missing model writes through the link and
                // puts the file inside the vault.
                LinkState::Dangling => dangling.push(l.clone()),
                LinkState::Replaced => replaced.push(l.clone()),
                _ => {}
            }
        }

        let records = self.store.vault_files()?;
        let mut missing = Vec::new();
        let mut known: std::collections::HashSet<PathBuf> = Default::default();

        for r in &records {
            let f = self.decorate(r)?;
            known.insert(self.store.vault_root().join(r.vault_rel_path()));
            for a in &r.aliases {
                known.insert(self.store.vault_root().join(r.alias_rel_path(a)));
            }
            if !f.present {
                missing.push(f);
            }
        }

        let mut foreign = Vec::new();
        for e in walkdir::WalkDir::new(self.store.vault_root())
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.store.is_internal(e.path()))
            .flatten()
        {
            let p = e.path();
            if self.store.is_internal(p) {
                continue;
            }
            let is_file_or_link = e.file_type().is_file() || e.file_type().is_symlink();
            if is_file_or_link && !known.contains(p) {
                foreign.push(crate::paths::display_path(p));
            }
        }
        foreign.sort();

        Ok(VaultHealth {
            checked_links: all_links.len() as u64,
            checked_files: records.len() as u64,
            ok: dangling.is_empty() && replaced.is_empty() && missing.is_empty(),
            dangling_links: dangling,
            replaced_links: replaced,
            missing_vault_files: missing,
            foreign_files: foreign,
        })
    }

    /// Removes the links that point at nothing.
    ///
    /// A dangling link is worse than a missing file, so the interface offers
    /// this straight from the health screen.
    pub fn remove_dangling_links(&self) -> Result<u64> {
        let mut removed = 0;
        for l in self.health()?.dangling_links {
            if self.platform.is_symlink(&l.abs_path) {
                self.platform.remove_symlink(&l.abs_path)?;
            }
            self.store.delete_link(&l.id)?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Where a content's real file sits on the disk.
    pub fn path_of(&self, record: &VaultFileRecord) -> PathBuf {
        self.store.vault_root().join(record.vault_rel_path())
    }
}

/// Puts a content into the vault outside an apply run.
///
/// Used by the tests and available to a command line front end. The apply
/// engine has its own journaled path and does not use this.
pub fn place_file(
    store: &Store,
    platform: &dyn Platform,
    source: &Path,
    category: &str,
    name: &str,
    sha256: &str,
    cancel: &CancelToken,
) -> Result<VaultFileRecord> {
    crate::paths::validate_file_name(name)?;
    let dir = store.vault_root().join(category);
    fsops::ensure_dir(&dir)?;
    let dest = dir.join(name);
    let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
    fsops::move_file(platform, source, &dest, sha256, &store.temp_dir(), cancel)?;

    let record = VaultFileRecord {
        sha256: sha256.to_string(),
        canonical_name: name.to_string(),
        category: category.to_string(),
        size_bytes: size,
        added_at: crate::time_util::Timestamp::now(),
        aliases: Vec::new(),
    };
    store.put_vault_file(&record)?;
    Ok(record)
}

/// Groups vault records by the content they hold. Kept separate so the grouping
/// rule can be tested without a disk.
pub fn group_by_content(records: &[VaultFileRecord]) -> BTreeMap<String, Vec<&VaultFileRecord>> {
    let mut out: BTreeMap<String, Vec<&VaultFileRecord>> = BTreeMap::new();
    for r in records {
        out.entry(r.sha256.clone()).or_default().push(r);
    }
    out
}

#[cfg(test)]
mod tests;
