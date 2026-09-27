//! The vault database.
//!
//! Everything the engine remembers lives in one file inside the vault:
//! `<vault>/.comfyvault/vault.redb`. Putting it in the vault, and not beside
//! the application, means the vault folder is self describing. Move the drive
//! to another computer and the record of what was taken and from where moves
//! with it.
//!
//! # Why redb
//!
//! The apply journal needs a real transaction. A journal entry has to reach the
//! disk **before** the filesystem step it describes, or a crash leaves work the
//! engine cannot undo. redb commits are durable, so `commit()` returning means
//! the entry survives a power cut.
//!
//! redb is also pure Rust, which matters here: the build cross compiles to
//! Windows from Linux with no C compiler available, so a bundled SQLite could
//! not be built at all.
//!
//! # Keys
//!
//! Composite keys are strings joined with a `\u{1}` separator, which cannot
//! appear in a path or an identifier. The sequence part is zero padded so the
//! lexical order redb iterates in is also the numeric order.

use std::path::{Path, PathBuf};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{ErrorCode, Result, VaultError};

mod records;
pub use records::*;

/// Bumped when a stored shape changes in a way old data cannot satisfy.
pub const SCHEMA_VERSION: u32 = 1;

/// The folder inside the vault that holds everything the engine writes.
pub const INTERNAL_DIR: &str = ".comfyvault";
const DB_FILE: &str = "vault.redb";
/// Where a cross-volume copy lands before it is put in place.
pub const TEMP_DIR: &str = "tmp";

const SEP: char = '\u{1}';

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const INSTALLS: TableDefinition<&str, &[u8]> = TableDefinition::new("installs");
const VAULT_FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("vault_files");
const LINKS: TableDefinition<&str, &[u8]> = TableDefinition::new("links");
const LINKS_BY_PATH: TableDefinition<&str, &str> = TableDefinition::new("links_by_path");
const HASH_CACHE: TableDefinition<&str, &[u8]> = TableDefinition::new("hash_cache");
const SCANS: TableDefinition<&str, &[u8]> = TableDefinition::new("scans");
const SCAN_ENTRIES: TableDefinition<&str, &[u8]> = TableDefinition::new("scan_entries");
const PLANS: TableDefinition<&str, &[u8]> = TableDefinition::new("plans");
const APPLIES: TableDefinition<&str, &[u8]> = TableDefinition::new("applies");
const JOURNAL: TableDefinition<&str, &[u8]> = TableDefinition::new("journal");
const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const DOWNLOADS: TableDefinition<&str, &[u8]> = TableDefinition::new("downloads");

/// Every table, created on open so a read never fails on a missing table.
const ALL_TABLES: [TableDefinition<&str, &[u8]>; 10] = [
    META, INSTALLS, VAULT_FILES, LINKS, HASH_CACHE, SCANS, SCAN_ENTRIES, PLANS, APPLIES, JOURNAL,
];

/// The vault database.
pub struct Store {
    db: Database,
    vault_root: PathBuf,
    /// A test's way to make download writes fail, as a full drive would.
    #[cfg(test)]
    download_writes_left: std::sync::Mutex<Option<u32>>,
    /// A test's way to make the next link record write fail, or stop the
    /// process there as a crash would.
    #[cfg(test)]
    link_write_fault: std::sync::Mutex<Option<LinkWriteFault>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub enum LinkWriteFault {
    Refuse,
    Crash,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("vault_root", &self.vault_root).finish()
    }
}

impl Store {
    /// Opens the database inside a vault, creating the vault if asked.
    /// Does this folder hold a ComfyVault vault: its own folder, with the
    /// database in it?
    pub fn is_vault(folder: &Path) -> bool {
        folder.join(INTERNAL_DIR).join(DB_FILE).is_file()
    }

    pub fn open(vault_root: &Path, create_if_missing: bool) -> Result<Self> {
        if !vault_root.exists() {
            if !create_if_missing {
                return Err(VaultError::new(
                    ErrorCode::NotFound,
                    "That vault folder does not exist. Choose another folder, or let the app create it.",
                )
                .with_path(vault_root));
            }
            std::fs::create_dir_all(vault_root)
                .map_err(|e| VaultError::from_io(&e, vault_root, "creating the vault folder"))?;
        }
        if !vault_root.is_dir() {
            return Err(VaultError::new(
                ErrorCode::InvalidArgument,
                "That is a file, not a folder. A vault has to be a folder.",
            )
            .with_path(vault_root));
        }

        let vault_root = crate::paths::canonicalize_clean(vault_root)
            .map_err(|e| VaultError::from_io(&e, vault_root, "opening the vault folder"))?;

        let internal = vault_root.join(INTERNAL_DIR);
        std::fs::create_dir_all(internal.join(TEMP_DIR))
            .map_err(|e| VaultError::from_io(&e, &internal, "creating the vault's own folder"))?;

        let db_path = internal.join(DB_FILE);
        let db = Database::create(&db_path).map_err(|e| {
            VaultError::new(
                ErrorCode::StoreError,
                "The vault database could not be opened. Another copy of the app may have it open.",
            )
            .with_detail(e.to_string())
            .with_path(&db_path)
        })?;

        let store = Self {
            db,
            vault_root,
            #[cfg(test)]
            download_writes_left: std::sync::Mutex::new(None),
            #[cfg(test)]
            link_write_fault: std::sync::Mutex::new(None),
        };
        store.initialize()?;
        store.scrub_download_addresses()?;
        Ok(store)
    }

    /// Rewrites every stored download address in the form the engine keeps.
    ///
    /// An earlier build kept the address as pasted, and Civitai's own
    /// instructions put the person's key in it. The database gives the
    /// pages an old row leaves back when it next writes and closes, which
    /// the test that runs a real crash checks.
    fn scrub_download_addresses(&self) -> Result<()> {
        for mut d in self.downloads()? {
            let kept = crate::download::address::stored_form(&d.address).unwrap_or_default();
            if kept != d.address {
                d.address = kept;
                self.put_download(&d)?;
            }
        }
        Ok(())
    }

    fn initialize(&self) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            for t in ALL_TABLES {
                tx.open_table(t)?;
            }
            tx.open_table(LINKS_BY_PATH)?;
            tx.open_table(METADATA)?;
            tx.open_table(DOWNLOADS)?;
        }
        tx.commit()?;

        self.scrub_stored_credentials()?;

        match self.meta_u32("schemaVersion")? {
            Some(v) if v > SCHEMA_VERSION => {
                return Err(VaultError::new(
                    ErrorCode::StoreError,
                    "This vault was made by a newer version of the app. Update the app to open it.",
                )
                .with_detail(format!("vault schema {v}, this build understands {SCHEMA_VERSION}")));
            }
            Some(_) => {}
            None => {
                self.put_meta("schemaVersion", &SCHEMA_VERSION)?;
                self.put_meta("createdAt", &crate::time_util::Timestamp::now())?;
            }
        }
        Ok(())
    }

    /// Removes a credential an older build stored, on open.
    ///
    /// Dropping a field from the type stops it being read. It does not remove
    /// the bytes: they sit in the database until someone happens to save a
    /// setting, which on an upgrade may be never. This vault is a folder the
    /// person is told to carry on a portable drive, so "unreadable by this
    /// build" is not the same as gone.
    ///
    /// The rule is by shape rather than by name, so it also catches a
    /// credential a future field introduces and a later build removes.
    ///
    /// Rewriting the row is what removes the bytes: the database reuses the
    /// page the old value sat in. That is verified by a test on a fresh vault
    /// and on one with four thousand rows in it, rather than assumed. What it
    /// cannot reach is a copy outside the database: a filesystem snapshot, a
    /// backup, or a block the drive has already relocated.
    fn scrub_stored_credentials(&self) -> Result<()> {
        let Some(raw): Option<serde_json::Value> = self.get(META, "settings")? else {
            return Ok(());
        };
        let Some(object) = raw.as_object() else { return Ok(()) };

        let carries_credential = object
            .iter()
            .any(|(k, v)| looks_like_a_credential(k) && !v.is_null());
        if !carries_credential {
            return Ok(());
        }

        // Rewrite the row from the settings this build understands, which by
        // construction carry no credential, then overwrite the old bytes.
        let cleaned: crate::settings::Settings =
            serde_json::from_value(raw).unwrap_or_default();
        self.put_meta("settings", &cleaned)?;
        Ok(())
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }

    pub fn internal_dir(&self) -> PathBuf {
        self.vault_root.join(INTERNAL_DIR)
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.internal_dir().join(TEMP_DIR)
    }

    /// Is this path the engine's own folder, which a scan must never treat as a
    /// model folder?
    pub fn is_internal(&self, path: &Path) -> bool {
        path.starts_with(self.internal_dir())
    }

    // -- generic helpers --------------------------------------------------

    fn put<T: Serialize>(&self, table: TableDefinition<&str, &[u8]>, key: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(table)?;
            t.insert(key, bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get<T: DeserializeOwned>(&self, table: TableDefinition<&str, &[u8]>, key: &str) -> Result<Option<T>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        match t.get(key)? {
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
            None => Ok(None),
        }
    }

    fn delete(&self, table: TableDefinition<&str, &[u8]>, key: &str) -> Result<bool> {
        let tx = self.db.begin_write()?;
        let existed;
        {
            let mut t = tx.open_table(table)?;
            let removed = t.remove(key)?;
            existed = removed.is_some();
        }
        tx.commit()?;
        Ok(existed)
    }

    fn list<T: DeserializeOwned>(&self, table: TableDefinition<&str, &[u8]>) -> Result<Vec<T>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        let mut out = Vec::new();
        for row in t.iter()? {
            let (_, v) = row?;
            out.push(serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }

    /// Every value whose key starts with `prefix`, in key order.
    fn list_prefix<T: DeserializeOwned>(
        &self,
        table: TableDefinition<&str, &[u8]>,
        prefix: &str,
    ) -> Result<Vec<T>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        let mut out = Vec::new();
        for row in t.range(prefix..)? {
            let (k, v) = row?;
            if !k.value().starts_with(prefix) {
                break;
            }
            out.push(serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }

    fn put_meta<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.put(META, key, value)
    }

    /// How a run was asked to check its files and to handle a failure, kept
    /// apart from its record so a recovery finishes it the same way.
    pub fn put_apply_choices<T: Serialize>(&self, apply_id: &str, choices: &T) -> Result<()> {
        self.put_meta(&format!("applyChoices:{apply_id}"), choices)
    }

    pub fn apply_choices<T: DeserializeOwned>(&self, apply_id: &str) -> Result<Option<T>> {
        self.get(META, &format!("applyChoices:{apply_id}"))
    }

    fn meta_u32(&self, key: &str) -> Result<Option<u32>> {
        self.get(META, key)
    }

    pub fn created_at(&self) -> Result<crate::time_util::Timestamp> {
        Ok(self
            .get(META, "createdAt")?
            .unwrap_or_else(crate::time_util::Timestamp::now))
    }

    // -- settings ---------------------------------------------------------

    pub fn settings(&self) -> Result<crate::settings::Settings> {
        Ok(self.get(META, "settings")?.unwrap_or_default())
    }

    pub fn put_settings(&self, s: &crate::settings::Settings) -> Result<()> {
        self.put_meta("settings", s)
    }

    // -- installs ---------------------------------------------------------

    pub fn put_install(&self, i: &crate::install::Install) -> Result<()> {
        self.put(INSTALLS, &i.id, i)
    }

    pub fn install(&self, id: &str) -> Result<Option<crate::install::Install>> {
        self.get(INSTALLS, id)
    }

    pub fn installs(&self) -> Result<Vec<crate::install::Install>> {
        let mut v: Vec<crate::install::Install> = self.list(INSTALLS)?;
        v.sort_by(|a, b| a.added_at.cmp(&b.added_at));
        Ok(v)
    }

    pub fn delete_install(&self, id: &str) -> Result<bool> {
        self.delete(INSTALLS, id)
    }

    /// The install already registered at this root, if there is one.
    pub fn install_at_root(&self, root: &Path) -> Result<Option<crate::install::Install>> {
        Ok(self
            .installs()?
            .into_iter()
            .find(|i| crate::paths::same_path_lexically(&i.root, root)))
    }

    // -- vault files ------------------------------------------------------

    pub fn put_vault_file(&self, f: &VaultFileRecord) -> Result<()> {
        self.put(VAULT_FILES, &f.sha256, f)
    }

    pub fn vault_file(&self, sha256: &str) -> Result<Option<VaultFileRecord>> {
        self.get(VAULT_FILES, sha256)
    }

    pub fn vault_files(&self) -> Result<Vec<VaultFileRecord>> {
        self.list(VAULT_FILES)
    }

    pub fn delete_vault_file(&self, sha256: &str) -> Result<bool> {
        self.delete(VAULT_FILES, sha256)
    }

    /// Is a vault file already using this name in this category?
    ///
    /// Used to settle a name clash, where two different contents carry one file
    /// name and the second must take an adjusted name.
    pub fn vault_name_taken(&self, category: &str, name: &str) -> Result<Option<String>> {
        for f in self.vault_files()? {
            if f.category != category {
                continue;
            }
            if f.canonical_name.eq_ignore_ascii_case(name)
                || f.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
            {
                return Ok(Some(f.sha256));
            }
        }
        Ok(None)
    }

    // -- links ------------------------------------------------------------

    #[cfg(test)]
    pub fn fault_next_link_write(&self, fault: LinkWriteFault) {
        *self.link_write_fault.lock().unwrap() = Some(fault);
    }

    pub fn put_link(&self, l: &LinkRecord) -> Result<()> {
        #[cfg(test)]
        {
            let fault = self.link_write_fault.lock().unwrap().take();
            match fault {
                Some(LinkWriteFault::Refuse) => {
                    return Err(VaultError::new(ErrorCode::StoreError, "The vault database refused the write."))
                }
                Some(LinkWriteFault::Crash) => panic!("the computer stopped"),
                None => {}
            }
        }
        let bytes = serde_json::to_vec(l)?;
        let key = path_key(&l.abs_path);
        let tx = self.db.begin_write()?;
        {
            let mut links = tx.open_table(LINKS)?;
            links.insert(l.id.as_str(), bytes.as_slice())?;
            let mut by_path = tx.open_table(LINKS_BY_PATH)?;
            by_path.insert(key.as_str(), l.id.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn link(&self, id: &str) -> Result<Option<LinkRecord>> {
        self.get(LINKS, id)
    }

    pub fn link_at_path(&self, path: &Path) -> Result<Option<LinkRecord>> {
        let key = path_key(path);
        let tx = self.db.begin_read()?;
        let by_path = tx.open_table(LINKS_BY_PATH)?;
        let Some(id) = by_path.get(key.as_str())? else {
            return Ok(None);
        };
        let links = tx.open_table(LINKS)?;
        match links.get(id.value())? {
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
            None => Ok(None),
        }
    }

    pub fn links(&self) -> Result<Vec<LinkRecord>> {
        self.list(LINKS)
    }

    pub fn links_for_hash(&self, sha256: &str) -> Result<Vec<LinkRecord>> {
        Ok(self.links()?.into_iter().filter(|l| l.sha256 == sha256).collect())
    }

    pub fn links_for_install(&self, install_id: &str) -> Result<Vec<LinkRecord>> {
        Ok(self
            .links()?
            .into_iter()
            .filter(|l| l.install_id == install_id)
            .collect())
    }

    pub fn delete_link(&self, id: &str) -> Result<bool> {
        let tx = self.db.begin_write()?;
        let record: Option<LinkRecord>;
        {
            let mut links = tx.open_table(LINKS)?;
            let removed = links.remove(id)?;
            record = match &removed {
                Some(v) => Some(serde_json::from_slice(v.value())?),
                None => None,
            };
        }
        let existed = record.is_some();
        if let Some(r) = record {
            let mut by_path = tx.open_table(LINKS_BY_PATH)?;
            by_path.remove(path_key(&r.abs_path).as_str())?;
        }
        tx.commit()?;
        Ok(existed)
    }

    // -- hash cache -------------------------------------------------------

    pub fn cached_hash(&self, path: &Path) -> Result<Option<HashCacheRecord>> {
        self.get(HASH_CACHE, &path_key(path))
    }

    pub fn put_cached_hash(&self, r: &HashCacheRecord) -> Result<()> {
        self.put(HASH_CACHE, &path_key(&r.path), r)
    }

    /// Writes many cache rows in one transaction.
    ///
    /// A scan produces thousands of these. One transaction each would make the
    /// scan disk bound on commits rather than on reading the files.
    pub fn put_cached_hashes(&self, rows: &[HashCacheRecord]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(HASH_CACHE)?;
            for r in rows {
                let bytes = serde_json::to_vec(r)?;
                t.insert(path_key(&r.path).as_str(), bytes.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear_hash_cache(&self) -> Result<usize> {
        self.clear_table(HASH_CACHE)
    }

    fn clear_table(&self, table: TableDefinition<&str, &[u8]>) -> Result<usize> {
        let tx = self.db.begin_write()?;
        let n = {
            let mut t = tx.open_table(table)?;
            let keys: Vec<String> = t
                .iter()?
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            let n = keys.len();
            for k in keys {
                t.remove(k.as_str())?;
            }
            n
        };
        tx.commit()?;
        Ok(n)
    }

    // -- scans ------------------------------------------------------------

    /// Stores a scan's record and moves both pointers in one transaction: the
    /// latest scan, and the last scan that finished.
    pub fn put_scan(&self, s: &ScanRecord) -> Result<()> {
        let record = serde_json::to_vec(s)?;
        let id = serde_json::to_vec(&s.scan_id)?;
        let tx = self.db.begin_write()?;
        {
            tx.open_table(SCANS)?.insert(s.scan_id.as_str(), record.as_slice())?;
            let mut meta = tx.open_table(META)?;
            meta.insert("lastScanId", id.as_slice())?;
            if !s.cancelled {
                meta.insert("lastFinishedScanId", id.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn scan(&self, id: &str) -> Result<Option<ScanRecord>> {
        self.get(SCANS, id)
    }

    /// The most recent scan, finished or cancelled.
    pub fn latest_scan_id(&self) -> Result<Option<String>> {
        self.get(META, "lastScanId")
    }

    /// The scan whose results count: the last one that finished.
    ///
    /// A cancelled scan read only part of the installs, so it never replaces
    /// a finished one. Its totals would say that a scanned computer holds
    /// nothing. Only when no scan ever finished is the latest, cancelled one
    /// the answer, so that the person still sees that a scan ran.
    pub fn last_scan_id(&self) -> Result<Option<String>> {
        if let Some(id) = self.get::<String>(META, "lastFinishedScanId")? {
            return Ok(Some(id));
        }
        // A vault written before this pointer existed, or one where no scan
        // has finished yet: find the last finished scan among the records.
        let finished = self
            .list::<ScanRecord>(SCANS)?
            .into_iter()
            .filter(|s| !s.cancelled)
            .max_by_key(|s| s.started_at)
            .map(|s| s.scan_id);
        match finished {
            Some(id) => Ok(Some(id)),
            None => self.latest_scan_id(),
        }
    }

    /// Makes the vault look like one written before the finished-scan pointer
    /// existed.
    #[cfg(test)]
    pub fn forget_finished_scan_pointer_for_tests(&self) {
        self.delete(META, "lastFinishedScanId").unwrap();
    }

    /// Stores the individual files a scan found, in one transaction.
    ///
    /// Anything already stored under this identifier is removed first. Writing
    /// a shorter result over a longer one would otherwise leave the tail of the
    /// old one behind, and the reader would see files that are no longer there.
    pub fn put_scan_entries(&self, scan_id: &str, entries: &[ScanEntryRecord]) -> Result<()> {
        let prefix = format!("{scan_id}{SEP}");
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(SCAN_ENTRIES)?;
            let stale: Vec<String> = t
                .range(prefix.as_str()..)?
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .take_while(|k| k.starts_with(&prefix))
                .collect();
            for k in stale {
                t.remove(k.as_str())?;
            }
            for (i, e) in entries.iter().enumerate() {
                let bytes = serde_json::to_vec(e)?;
                t.insert(seq_key(scan_id, i as u64).as_str(), bytes.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn scan_entries(&self, scan_id: &str) -> Result<Vec<ScanEntryRecord>> {
        self.list_prefix(SCAN_ENTRIES, &format!("{scan_id}{SEP}"))
    }

    // -- plans ------------------------------------------------------------

    pub fn put_plan(&self, p: &crate::plan::ConsolidationPlan) -> Result<()> {
        self.put(PLANS, &p.plan_id, p)?;
        self.put_meta("lastPlanId", &p.plan_id)
    }

    pub fn plan(&self, id: &str) -> Result<Option<crate::plan::ConsolidationPlan>> {
        self.get(PLANS, id)
    }

    pub fn last_plan_id(&self) -> Result<Option<String>> {
        self.get(META, "lastPlanId")
    }

    // -- applies and the journal -------------------------------------------

    pub fn put_apply(&self, a: &ApplyRecord) -> Result<()> {
        self.put(APPLIES, &a.apply_id, a)
    }

    pub fn apply(&self, id: &str) -> Result<Option<ApplyRecord>> {
        self.get(APPLIES, id)
    }

    pub fn applies(&self) -> Result<Vec<ApplyRecord>> {
        let mut v: Vec<ApplyRecord> = self.list(APPLIES)?;
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(v)
    }

    /// Writes one journal entry and waits for it to reach the disk.
    ///
    /// The caller writes the entry **before** it performs the step. A crash
    /// between the two leaves a `Pending` entry, which recovery reads and either
    /// finishes or undoes. A crash the other way round would leave a change on
    /// disk that nothing records, and that change could not be undone.
    pub fn append_journal(&self, e: &JournalEntry) -> Result<()> {
        self.put(JOURNAL, &seq_key(&e.apply_id, e.seq), e)
    }

    pub fn update_journal(&self, e: &JournalEntry) -> Result<()> {
        self.append_journal(e)
    }

    /// Every run that has journal entries, including the `rename-*` runs the
    /// vault screen writes. A revert needs this to find out whether anything
    /// later depends on the run being undone.
    pub fn journal_ids(&self) -> Result<Vec<String>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(JOURNAL)?;
        let mut out: Vec<String> = Vec::new();
        for row in t.iter()? {
            let (k, _) = row?;
            if let Some((id, _)) = k.value().split_once(SEP) {
                if out.last().map(|l| l != id).unwrap_or(true) {
                    out.push(id.to_string());
                }
            }
        }
        out.dedup();
        Ok(out)
    }

    pub fn journal(&self, apply_id: &str) -> Result<Vec<JournalEntry>> {
        self.list_prefix(JOURNAL, &format!("{apply_id}{SEP}"))
    }

    // -- metadata cache ---------------------------------------------------

    /// A cached answer, checked again as it is read: its addresses are held
    /// to the same rules as a fresh answer's, whoever wrote the row.
    pub fn metadata(&self, sha256: &str) -> Result<Option<crate::metadata::ModelMetadata>> {
        Ok(self.get(METADATA, sha256)?.map(crate::metadata::civitai::checked))
    }

    pub fn put_metadata(&self, m: &crate::metadata::ModelMetadata) -> Result<()> {
        self.put(METADATA, &m.sha256, m)
    }

    // -- downloads --------------------------------------------------------

    pub fn put_download(&self, d: &crate::download::DownloadRecord) -> Result<()> {
        #[cfg(test)]
        if let Some(left) = self.download_writes_left.lock().unwrap().as_mut() {
            if *left == 0 {
                return Err(VaultError::new(ErrorCode::StoreError, "The vault database refused the write.")
                    .with_detail("a test made download writes fail"));
            }
            *left -= 1;
        }
        self.put(DOWNLOADS, &d.download.download_id, d)
    }

    #[cfg(test)]
    pub fn download_writes_left_for_tests(&self) -> std::sync::MutexGuard<'_, Option<u32>> {
        self.download_writes_left.lock().unwrap()
    }

    /// Lets this many more download writes through, then refuses every one.
    #[cfg(test)]
    pub fn fail_download_writes_after(&self, n: u32) {
        *self.download_writes_left.lock().unwrap() = Some(n);
    }

    pub fn download(&self, id: &str) -> Result<Option<crate::download::DownloadRecord>> {
        self.get(DOWNLOADS, id)
    }

    /// Every download, in the order they were started.
    pub fn downloads(&self) -> Result<Vec<crate::download::DownloadRecord>> {
        let mut out: Vec<crate::download::DownloadRecord> = self.list(DOWNLOADS)?;
        out.sort_by_key(|d| d.seq);
        Ok(out)
    }

    pub fn delete_download(&self, id: &str) -> Result<bool> {
        self.delete(DOWNLOADS, id)
    }

    /// The installs the person ticked for the last download.
    pub fn last_download_installs(&self) -> Result<Option<Vec<String>>> {
        self.get(META, "lastDownloadInstalls")
    }

    pub fn put_last_download_installs(&self, ids: &[String]) -> Result<()> {
        self.put_meta("lastDownloadInstalls", &ids)
    }

    /// The folder the person last chose for a new link of this category in
    /// this install.
    pub fn link_dir(&self, install_id: &str, category: &str) -> Result<Option<PathBuf>> {
        let all: std::collections::BTreeMap<String, std::collections::BTreeMap<String, PathBuf>> =
            self.get(META, "linkDirs")?.unwrap_or_default();
        Ok(all.get(install_id).and_then(|c| c.get(category)).cloned())
    }

    pub fn put_link_dir(&self, install_id: &str, category: &str, dir: &Path) -> Result<()> {
        let mut all: std::collections::BTreeMap<String, std::collections::BTreeMap<String, PathBuf>> =
            self.get(META, "linkDirs")?.unwrap_or_default();
        all.entry(install_id.to_string()).or_default().insert(category.to_string(), dir.to_path_buf());
        self.put_meta("linkDirs", &all)
    }

    /// Where a download keeps its part while it runs. Inside the engine's own
    /// folder, so a scan and the health check never take it for a model, and
    /// on the vault's drive, so moving it into place is a rename.
    pub fn downloads_dir(&self) -> PathBuf {
        self.internal_dir().join("downloads")
    }

    pub fn clear_metadata(&self) -> Result<usize> {
        self.clear_table(METADATA)
    }
}

/// Does this field name look like it holds a secret?
///
/// By shape rather than by name, so it catches a credential a future field
/// introduces and a later build removes, not only the one that prompted it.
fn looks_like_a_credential(field: &str) -> bool {
    let f = field.to_lowercase();
    ["key", "token", "secret", "password", "credential", "bearer"]
        .iter()
        .any(|w| f.contains(w))
}

/// A path used as a database key.
///
/// Two spellings of one file must land on one key, or a link the engine
/// created cannot be found again and a cached hash is recomputed for nothing.
///
/// Windows treats `/` and `\` as the same separator and ignores case, so both
/// are normalized here. A path that arrives from the interface as
/// `C:/models/loras` and one the engine walked as `C:\models\loras` are the
/// same folder, and they have to key the same.
use crate::paths::compare_key as path_key;

/// A key that sorts by sequence number, because redb iterates in key order.
fn seq_key(id: &str, seq: u64) -> String {
    format!("{id}{SEP}{seq:012}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_util::Timestamp;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("vault"), true).unwrap();
        (d, s)
    }

    fn link_record(id: &str, path: &str, sha: &str) -> LinkRecord {
        LinkRecord {
            id: id.into(),
            install_id: "inst".into(),
            abs_path: PathBuf::from(path),
            rel_path: PathBuf::from("models/loras/x.safetensors"),
            link_name: "x.safetensors".into(),
            sha256: sha.into(),
            vault_rel_path: PathBuf::from("loras/x.safetensors"),
            created_at: Timestamp::now(),
            created_by: LinkOrigin::Apply,
            apply_id: None,
        }
    }

    #[test]
    fn opening_a_missing_vault_creates_it_when_asked() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("NewVault");
        assert!(!root.exists());
        let s = Store::open(&root, true).unwrap();
        assert!(root.is_dir());
        assert!(s.internal_dir().is_dir());
        assert!(s.temp_dir().is_dir());
    }

    #[test]
    fn opening_a_missing_vault_refuses_when_not_asked() {
        let d = tempfile::tempdir().unwrap();
        let err = Store::open(&d.path().join("Nope"), false).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn opening_a_file_as_a_vault_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("file.txt");
        std::fs::write(&f, b"x").unwrap();
        let err = Store::open(&f, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_vault_reopens_with_its_data_intact() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        {
            let s = Store::open(&root, true).unwrap();
            s.put_vault_file(&VaultFileRecord {
                sha256: "AA".into(),
                canonical_name: "a.safetensors".into(),
                category: "loras".into(),
                size_bytes: 10,
                added_at: Timestamp::now(),
                aliases: vec!["b.safetensors".into()],
            })
            .unwrap();
        }
        let s = Store::open(&root, true).unwrap();
        let f = s.vault_file("AA").unwrap().unwrap();
        assert_eq!(f.canonical_name, "a.safetensors");
        assert_eq!(f.aliases, vec!["b.safetensors"]);
    }

    #[test]
    fn a_vault_from_a_newer_app_is_refused_rather_than_misread() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        {
            let s = Store::open(&root, true).unwrap();
            s.put_meta("schemaVersion", &(SCHEMA_VERSION + 1)).unwrap();
        }
        let err = Store::open(&root, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreError);
        assert!(err.message.contains("newer version"));
    }

    #[test]
    fn settings_default_until_they_are_written() {
        let (_d, s) = store();
        assert_eq!(s.settings().unwrap(), crate::settings::Settings::default());

        let mut custom = crate::settings::Settings::default();
        custom.metadata_lookups_enabled = false;
        s.put_settings(&custom).unwrap();
        assert!(!s.settings().unwrap().metadata_lookups_enabled);
    }

    #[test]
    fn one_path_spelled_two_ways_finds_the_same_link() {
        // The interface sends paths as text. On Windows a forward slash and a
        // backslash name the same folder, and so do two capitalizations. A key
        // that told them apart would lose the link the engine had just made.
        let (_d, s) = store();
        s.put_link(&link_record("l1", r"C:\Install\models\loras\x.safetensors", "AA"))
            .unwrap();

        if cfg!(windows) {
            assert!(
                s.link_at_path(Path::new("C:/Install/models/loras/x.safetensors"))
                    .unwrap()
                    .is_some(),
                "a forward-slash spelling must find it"
            );
            assert!(
                s.link_at_path(Path::new(r"c:\install\MODELS\loras\X.safetensors"))
                    .unwrap()
                    .is_some(),
                "a differently cased spelling must find it"
            );
        }
        assert!(s
            .link_at_path(Path::new(r"C:\Install\models\loras\x.safetensors"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn links_are_found_by_id_by_path_by_hash_and_by_install() {
        let (_d, s) = store();
        s.put_link(&link_record("l1", "/install/a/x.safetensors", "AA")).unwrap();
        s.put_link(&link_record("l2", "/install/b/y.safetensors", "AA")).unwrap();
        s.put_link(&link_record("l3", "/install/c/z.safetensors", "BB")).unwrap();

        assert!(s.link("l1").unwrap().is_some());
        assert_eq!(
            s.link_at_path(Path::new("/install/b/y.safetensors")).unwrap().unwrap().id,
            "l2"
        );
        assert_eq!(s.links_for_hash("AA").unwrap().len(), 2);
        assert_eq!(s.links_for_install("inst").unwrap().len(), 3);
        assert!(s.link_at_path(Path::new("/nowhere")).unwrap().is_none());
    }

    #[test]
    fn deleting_a_link_clears_the_path_lookup_too() {
        // A stale path entry would make the engine think a link exists where
        // there is now a real file, and refuse to consolidate it forever.
        let (_d, s) = store();
        s.put_link(&link_record("l1", "/install/a/x.safetensors", "AA")).unwrap();
        assert!(s.delete_link("l1").unwrap());
        assert!(s.link("l1").unwrap().is_none());
        assert!(
            s.link_at_path(Path::new("/install/a/x.safetensors")).unwrap().is_none(),
            "the path lookup kept a dead entry"
        );
        assert!(!s.delete_link("l1").unwrap(), "deleting twice reports not found");
    }

    #[test]
    fn a_link_record_survives_being_replaced_at_the_same_path() {
        let (_d, s) = store();
        s.put_link(&link_record("l1", "/install/a/x.safetensors", "AA")).unwrap();
        s.put_link(&link_record("l2", "/install/a/x.safetensors", "BB")).unwrap();
        let found = s.link_at_path(Path::new("/install/a/x.safetensors")).unwrap().unwrap();
        assert_eq!(found.id, "l2", "the newer record must win the path lookup");
    }

    #[test]
    fn the_hash_cache_stores_and_returns_a_row() {
        let (_d, s) = store();
        let r = HashCacheRecord {
            path: PathBuf::from("/m/a.safetensors"),
            size_bytes: 123,
            mtime_nanos: 456,
            sha256: "CAFE".into(),
        };
        s.put_cached_hash(&r).unwrap();
        let got = s.cached_hash(Path::new("/m/a.safetensors")).unwrap().unwrap();
        assert_eq!(got.sha256, "CAFE");
        assert_eq!(got.size_bytes, 123);
        assert_eq!(got.mtime_nanos, 456);
    }

    #[test]
    fn clearing_the_hash_cache_reports_how_many_rows_went() {
        let (_d, s) = store();
        for i in 0..5 {
            s.put_cached_hash(&HashCacheRecord {
                path: PathBuf::from(format!("/m/{i}.safetensors")),
                size_bytes: 1,
                mtime_nanos: 1,
                sha256: "X".into(),
            })
            .unwrap();
        }
        assert_eq!(s.clear_hash_cache().unwrap(), 5);
        assert!(s.cached_hash(Path::new("/m/0.safetensors")).unwrap().is_none());
    }

    #[test]
    fn a_batch_of_cache_rows_all_land() {
        let (_d, s) = store();
        let rows: Vec<HashCacheRecord> = (0..500)
            .map(|i| HashCacheRecord {
                path: PathBuf::from(format!("/m/{i}.safetensors")),
                size_bytes: i,
                mtime_nanos: i as i128,
                sha256: format!("{i:064X}"),
            })
            .collect();
        s.put_cached_hashes(&rows).unwrap();
        assert_eq!(s.cached_hash(Path::new("/m/499.safetensors")).unwrap().unwrap().size_bytes, 499);
        assert_eq!(s.clear_hash_cache().unwrap(), 500);
    }

    #[test]
    fn journal_entries_come_back_in_sequence_order() {
        // Revert walks the journal backwards. If the order were lexical on an
        // unpadded number, step 10 would sort before step 2 and the undo would
        // run in the wrong order.
        let (_d, s) = store();
        for seq in [0u64, 1, 2, 9, 10, 11, 100, 101] {
            s.append_journal(&JournalEntry {
                apply_id: "ap1".into(),
                seq,
                group_id: "g".into(),
                step: JournalStep::CreateLink {
                    link: PathBuf::from(format!("/l/{seq}")),
                    target: PathBuf::from("/v/x"),
                },
                state: JournalState::Pending,
                started_at: Timestamp::now(),
                finished_at: None,
                error: None,
            })
            .unwrap();
        }
        let got: Vec<u64> = s.journal("ap1").unwrap().iter().map(|e| e.seq).collect();
        assert_eq!(got, vec![0, 1, 2, 9, 10, 11, 100, 101]);
    }

    #[test]
    fn one_applys_journal_never_leaks_into_another() {
        let (_d, s) = store();
        for (apply, seq) in [("ap1", 0u64), ("ap1", 1), ("ap2", 0)] {
            s.append_journal(&JournalEntry {
                apply_id: apply.into(),
                seq,
                group_id: "g".into(),
                step: JournalStep::CreateDir { path: PathBuf::from("/d") },
                state: JournalState::Done,
                started_at: Timestamp::now(),
                finished_at: None,
                error: None,
            })
            .unwrap();
        }
        assert_eq!(s.journal("ap1").unwrap().len(), 2);
        assert_eq!(s.journal("ap2").unwrap().len(), 1);
    }

    #[test]
    fn updating_a_journal_entry_replaces_it_in_place() {
        let (_d, s) = store();
        let mut e = JournalEntry {
            apply_id: "ap1".into(),
            seq: 0,
            group_id: "g".into(),
            step: JournalStep::CreateDir { path: PathBuf::from("/d") },
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        };
        s.append_journal(&e).unwrap();
        e.state = JournalState::Done;
        s.update_journal(&e).unwrap();

        let all = s.journal("ap1").unwrap();
        assert_eq!(all.len(), 1, "an update must not add a second row");
        assert_eq!(all[0].state, JournalState::Done);
    }

    #[test]
    fn scan_entries_come_back_in_the_order_they_went_in() {
        let (_d, s) = store();
        let entries: Vec<ScanEntryRecord> = (0..25)
            .map(|i| ScanEntryRecord {
                abs_path: PathBuf::from(format!("/m/{i:03}.safetensors")),
                rel_path: PathBuf::from(format!("{i:03}.safetensors")),
                install_id: "inst".into(),
                category: "loras".into(),
                size_bytes: i,
                sha256: Some(format!("{i:064X}")),
                mtime_nanos: 0,
                classification: Classification::Movable,
                link_target: None,
            })
            .collect();
        s.put_scan_entries("scan1", &entries).unwrap();
        let back = s.scan_entries("scan1").unwrap();
        assert_eq!(back.len(), 25);
        assert_eq!(back[0].size_bytes, 0);
        assert_eq!(back[24].size_bytes, 24);
    }

    #[test]
    fn rewriting_a_scans_entries_leaves_none_of_the_old_ones() {
        // A shorter result written over a longer one must not leave the tail of
        // the old one behind, or the reader sees files that are no longer
        // there and counts them.
        let (_d, s) = store();
        let entry = |n: u64| ScanEntryRecord {
            abs_path: PathBuf::from(format!("/m/{n}.safetensors")),
            rel_path: PathBuf::from(format!("{n}.safetensors")),
            install_id: "inst".into(),
            category: "loras".into(),
            size_bytes: n,
            sha256: Some(format!("{n:064X}")),
            mtime_nanos: 0,
            classification: Classification::Movable,
            link_target: None,
        };

        let many: Vec<ScanEntryRecord> = (0..10).map(entry).collect();
        s.put_scan_entries("scan1", &many).unwrap();
        assert_eq!(s.scan_entries("scan1").unwrap().len(), 10);

        s.put_scan_entries("scan1", &[entry(0)]).unwrap();
        assert_eq!(s.scan_entries("scan1").unwrap().len(), 1, "the old tail survived");

        s.put_scan_entries("scan1", &[]).unwrap();
        assert!(s.scan_entries("scan1").unwrap().is_empty());
    }

    #[test]
    fn rewriting_one_scan_leaves_another_alone() {
        let (_d, s) = store();
        let entry = ScanEntryRecord {
            abs_path: PathBuf::from("/m/a.safetensors"),
            rel_path: PathBuf::from("a.safetensors"),
            install_id: "inst".into(),
            category: "loras".into(),
            size_bytes: 1,
            sha256: None,
            mtime_nanos: 0,
            classification: Classification::Movable,
            link_target: None,
        };
        s.put_scan_entries("scan1", &[entry.clone(), entry.clone()]).unwrap();
        s.put_scan_entries("scan2", &[entry.clone()]).unwrap();

        s.put_scan_entries("scan1", &[]).unwrap();
        assert!(s.scan_entries("scan1").unwrap().is_empty());
        assert_eq!(s.scan_entries("scan2").unwrap().len(), 1, "a different scan must be untouched");
    }

    #[test]
    fn installs_are_listed_oldest_first() {
        let (_d, s) = store();
        for (i, ms) in [("b", 2000i64), ("a", 1000), ("c", 3000)] {
            let d = tempfile::tempdir().unwrap();
            let root = d.path().join("ComfyUI");
            crate::install::detect::fixtures::make_install(&root);
            let c = crate::install::detect::inspect(&root).unwrap();
            let mut inst =
                crate::install::Install::from_candidate(i.into(), i.into(), root.clone(), &c).unwrap();
            inst.added_at = Timestamp::from_millis(ms);
            s.put_install(&inst).unwrap();
            std::mem::forget(d); // keep the fabricated tree alive for this test
        }
        let ids: Vec<String> = s.installs().unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn a_name_clash_is_found_within_a_category_and_not_across_categories() {
        let (_d, s) = store();
        s.put_vault_file(&VaultFileRecord {
            sha256: "AAA".into(),
            canonical_name: "lora1.safetensors".into(),
            category: "loras".into(),
            size_bytes: 10,
            added_at: Timestamp::now(),
            aliases: vec!["alt.safetensors".into()],
        })
        .unwrap();

        assert_eq!(s.vault_name_taken("loras", "lora1.safetensors").unwrap().as_deref(), Some("AAA"));
        // An alias also holds a name.
        assert_eq!(s.vault_name_taken("loras", "alt.safetensors").unwrap().as_deref(), Some("AAA"));
        // Windows ignores case, so a differently cased name is the same name.
        assert_eq!(s.vault_name_taken("loras", "LORA1.SAFETENSORS").unwrap().as_deref(), Some("AAA"));
        // A different category is a different folder, so the name is free.
        assert_eq!(s.vault_name_taken("checkpoints", "lora1.safetensors").unwrap(), None);
        assert_eq!(s.vault_name_taken("loras", "other.safetensors").unwrap(), None);
    }

    #[test]
    fn the_engines_own_folder_is_recognized_so_a_scan_skips_it() {
        let (_d, s) = store();
        assert!(s.is_internal(&s.internal_dir().join("vault.redb")));
        assert!(s.is_internal(&s.temp_dir().join("partial.bin")));
        assert!(!s.is_internal(&s.vault_root().join("loras/x.safetensors")));
    }

    #[test]
    fn a_missing_row_answers_none_rather_than_failing() {
        let (_d, s) = store();
        assert!(s.install("nope").unwrap().is_none());
        assert!(s.vault_file("nope").unwrap().is_none());
        assert!(s.scan("nope").unwrap().is_none());
        assert!(s.apply("nope").unwrap().is_none());
        assert!(s.metadata("nope").unwrap().is_none());
        assert!(s.journal("nope").unwrap().is_empty());
        assert!(s.scan_entries("nope").unwrap().is_empty());
    }
}

#[cfg(test)]
mod scrub_tests {
    use super::*;

    /// Writes the settings blob an older build left behind, credential and all.
    fn plant_old_settings(root: &Path, key: &str) {
        let s = Store::open(root, true).unwrap();
        let old = serde_json::json!({
            "metadataLookupsEnabled": true,
            "civitaiApiKey": key,
            "hashCacheEnabled": true,
            "scanExtensions": [".safetensors"],
            "minFileSizeBytes": 1048576,
            "followExtraModelPaths": true,
            "scanOutputModelDirs": true
        });
        s.put_meta("settings", &old).unwrap();
        drop(s);
    }

    fn key_readable_in(root: &Path, key: &str) -> bool {
        let db = root.join(INTERNAL_DIR).join(DB_FILE);
        let bytes = std::fs::read(&db).unwrap_or_default();
        bytes
            .windows(key.len())
            .any(|w| w == key.as_bytes())
    }

    #[test]
    fn opening_a_vault_removes_a_credential_an_older_build_stored() {
        // Dropping the field stops it being read. It does not remove the
        // bytes, and on an upgrade nobody may ever save a setting. This vault
        // is a folder the person is told to carry on a portable drive.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        const KEY: &str = "cv-SECRET-abc123";

        plant_old_settings(&root, KEY);
        assert!(
            key_readable_in(&root, KEY),
            "the test did not manage to plant the old value, so it proves nothing"
        );

        // Just opening it, with no setting saved and nothing else done.
        let s = Store::open(&root, true).unwrap();
        drop(s);

        assert!(
            !key_readable_in(&root, KEY),
            "the credential is still readable in the vault database"
        );
    }

    #[test]
    fn the_credential_goes_even_when_the_database_has_grown() {
        // A small database reuses its free pages almost at once, so a single
        // write can remove the old bytes by itself. A vault that has been used
        // has more pages to choose from, and that is where a write alone may
        // leave the old value sitting in one of them.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        const KEY: &str = "cv-SECRET-abc123";

        {
            let s = Store::open(&root, true).unwrap();
            let rows: Vec<HashCacheRecord> = (0..4000)
                .map(|i| HashCacheRecord {
                    path: PathBuf::from(format!("/m/{i}.safetensors")),
                    size_bytes: i,
                    mtime_nanos: i as i128,
                    sha256: format!("{i:064X}"),
                })
                .collect();
            s.put_cached_hashes(&rows).unwrap();

            let old = serde_json::json!({
                "metadataLookupsEnabled": true,
                "civitaiApiKey": KEY,
                "hashCacheEnabled": true,
                "scanExtensions": [".safetensors"],
                "minFileSizeBytes": 1048576,
                "followExtraModelPaths": true,
                "scanOutputModelDirs": true
            });
            s.put_meta("settings", &old).unwrap();
        }
        assert!(key_readable_in(&root, KEY), "the old value was not planted");

        let s = Store::open(&root, true).unwrap();
        drop(s);

        assert!(
            !key_readable_in(&root, KEY),
            "the credential survived in a used vault's database"
        );
    }

    #[test]
    fn the_settings_survive_the_scrub() {
        // The control. Removing the credential must not take the person's
        // other choices with it.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        plant_old_settings(&root, "cv-SECRET-abc123");

        let s = Store::open(&root, true).unwrap();
        let settings = s.settings().unwrap();
        assert_eq!(settings.scan_extensions, vec![".safetensors"]);
        assert_eq!(settings.min_file_size_bytes, 1048576);
        assert!(settings.metadata_lookups_enabled);
    }

    #[test]
    fn a_vault_with_nothing_to_scrub_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        {
            let s = Store::open(&root, true).unwrap();
            let mut custom = crate::settings::Settings::default();
            custom.min_file_size_bytes = 4096;
            s.put_settings(&custom).unwrap();
        }
        let s = Store::open(&root, true).unwrap();
        assert_eq!(s.settings().unwrap().min_file_size_bytes, 4096);
    }

    #[test]
    fn the_rule_is_the_shape_of_the_name_not_the_one_field_that_prompted_it() {
        for yes in ["civitaiApiKey", "apiKey", "authToken", "clientSecret", "password", "bearer"] {
            assert!(looks_like_a_credential(yes), "{yes} should be scrubbed");
        }
        for no in ["metadataLookupsEnabled", "scanExtensions", "minFileSizeBytes", "vaultRoot"] {
            assert!(!looks_like_a_credential(no), "{no} is not a credential");
        }
    }
}
