//! The engine's front door.
//!
//! One object holds the open vault, the platform, and the rule that only one
//! long operation runs at a time. Everything above it, whether the desktop
//! command layer, a command line tool or an MCP server, calls these methods and
//! nothing else.
//!
//! # Long operations
//!
//! A scan, an apply and a revert run on their own thread and report through a
//! sink the caller supplies. The starting call returns an identifier straight
//! away, which is what the contract promises the interface. A second long
//! operation while one runs is refused with `vaultBusy`, so two of them can
//! never touch the same files at once.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::apply::{
    ApplyProgress, ApplyRequest, Applier, InterruptedApply, RevertPreview, RevertProgress,
};
use crate::error::{ErrorCode, Result, VaultError};
use crate::install::{detect, Install, InstallCandidate};
use crate::links::{CreateLinkRequest, Links, ModelDirNode};
use crate::metadata::{civitai::CivitaiClient, http::UreqTransport, MetadataService, ModelMetadata};
use crate::plan::{ConsolidationPlan, Planner};
use crate::platform::{LockState, NativePlatform, Platform, PlatformReport, RunningComfy};
use crate::progress::{CancelToken, ProgressSink};
use crate::scan::{ScanProgress, Scanner};
use crate::settings::{Settings, SettingsPatch};
use crate::store::{ApplyRecord, LinkRecord, LinkState, ScanEntryRecord, ScanRecord, Store};
use crate::usage::UsageReport;
use crate::vault::{NameGroup, Vault, VaultFile, VaultFilter, VaultHealth, VaultPage, VaultSort};

/// What the application remembers between runs.
///
/// Only the vault folder. Everything else lives inside the vault, so the vault
/// folder is self describing and moving the drive moves the whole record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub vault_root: Option<PathBuf>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| VaultError::from_io(&e, parent, "creating the settings folder"))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)
            .map_err(|e| VaultError::from_io(&e, path, "saving the settings"))
    }
}

/// The long operation currently running.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusyOp {
    pub kind: BusyKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BusyKind {
    Scan,
    Apply,
    Revert,
}

impl BusyKind {
    fn word(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Apply => "consolidation",
            Self::Revert => "undo",
        }
    }
}

/// What the interface asks for when it starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub vault_root: Option<String>,
    pub vault_initialized: bool,
    pub install_count: u64,
    pub platform: PlatformReport,
    pub settings: Settings,
    pub last_scan_id: Option<String>,
    pub last_plan_id: Option<String>,
    pub interrupted_applies: Vec<String>,
    pub busy: Option<BusyOp>,
}

/// Facts about an open vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub created_at: crate::time_util::Timestamp,
    /// Which drive the vault is on, as the operating system names it.
    ///
    /// On Windows this is the drive root and it carries a trailing separator:
    /// `C:\`, not `C:`. Do not compare it against the first characters of a
    /// path. Comparing `C:\` with `C:` said every install on the vault's own
    /// drive was on a different one, and the person was told their files would
    /// be copied when they would in fact be renamed.
    pub volume: String,
    /// Null together when the drive could not be read. Never zero: zero of
    /// zero reads as a completely full drive, which is a different statement.
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub file_count: u64,
    pub total_stored_bytes: u64,
    pub schema_version: u32,
}

/// The engine.
pub struct Engine {
    platform: Arc<dyn Platform>,
    store: RwLock<Option<Arc<Store>>>,
    busy: Mutex<Option<(BusyOp, CancelToken)>>,
    config_path: PathBuf,
}

impl Engine {
    /// Builds an engine against the real operating system.
    pub fn new(config_path: PathBuf) -> Arc<Self> {
        Self::with_platform(config_path, Arc::new(NativePlatform::new()))
    }

    pub fn with_platform(config_path: PathBuf, platform: Arc<dyn Platform>) -> Arc<Self> {
        Arc::new(Self {
            platform,
            store: RwLock::new(None),
            busy: Mutex::new(None),
            config_path,
        })
    }

    /// Opens the vault recorded from last time, if there is one.
    ///
    /// A vault that cannot be opened is reported and the application still
    /// starts, so the person can choose another folder rather than face a
    /// window that refuses to appear.
    pub fn restore_last_vault(&self) -> Option<VaultError> {
        let config = AppConfig::load(&self.config_path);
        let root = config.vault_root?;
        match self.select_vault(&root, false) {
            Ok(_) => None,
            Err(e) => Some(e),
        }
    }

    pub fn platform(&self) -> &dyn Platform {
        self.platform.as_ref()
    }

    /// Every drive on this computer, with its size and its free space.
    ///
    /// Answers with no vault open, because it is what the first screen shows
    /// while asking where the vault should go. It reads the drives and nothing
    /// in the vault, so there is nothing for it to need.
    pub fn drives(&self) -> Vec<crate::platform::DriveInfo> {
        self.platform.drives()
    }

    pub fn platform_report(&self) -> PlatformReport {
        PlatformReport {
            os: crate::platform::os_name().to_string(),
            symlinks: self.platform.symlink_capability(),
            long_paths_enabled: self.platform.long_paths_enabled(),
        }
    }

    fn store(&self) -> Result<Arc<Store>> {
        self.store
            .read()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(VaultError::not_initialized)
    }

    /// Takes the single long-operation slot, or reports what is already in it.
    ///
    /// The worker thread owns the slot from here and clears it when it ends,
    /// however it ends.
    fn take_slot(&self, kind: BusyKind, id: &str) -> Result<CancelToken> {
        let mut g = self
            .busy
            .lock()
            .map_err(|_| VaultError::new(ErrorCode::StoreError, "The app got into a bad state. Restart it."))?;
        if let Some((op, _)) = g.as_ref() {
            return Err(VaultError::busy(op.kind.word()));
        }
        let cancel = CancelToken::new();
        *g = Some((BusyOp { kind, id: id.to_string() }, cancel.clone()));
        Ok(cancel)
    }

    pub fn busy(&self) -> Option<BusyOp> {
        self.busy.lock().ok().and_then(|g| g.as_ref().map(|(op, _)| op.clone()))
    }

    /// Asks the running operation to stop.
    pub fn cancel(&self, id: &str) -> Result<()> {
        let g = self
            .busy
            .lock()
            .map_err(|_| VaultError::new(ErrorCode::StoreError, "The app got into a bad state. Restart it."))?;
        match g.as_ref() {
            Some((op, cancel)) if op.id == id => {
                cancel.cancel();
                Ok(())
            }
            Some(_) => Err(VaultError::not_found("That is not the operation running now.")),
            None => Err(VaultError::not_found("Nothing is running.")),
        }
    }

    // -- vault ------------------------------------------------------------

    pub fn select_vault(&self, path: &Path, create_if_missing: bool) -> Result<VaultInfo> {
        if self.busy().is_some() {
            return Err(VaultError::busy("operation"));
        }
        // Installs known right now, before the vault is swapped. A folder
        // inside one of them cannot be the vault, whether that install is
        // listed in the vault being opened or in the one already open.
        let open = self.store.read().ok().and_then(|g| g.clone());
        let known_roots: Vec<PathBuf> = open
            .as_ref()
            .map(|s| s.installs().unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .map(|i| i.root)
            .collect();

        // Where the vault would really be, followed through every link that
        // exists, before anything is created. A refused folder used to be
        // created first, database and all, inside the person's install.
        let target = crate::paths::canonicalize_existing_prefix(&crate::paths::lexical_normalize(path))?;
        if let Some(root) = known_roots.iter().find(|r| target.starts_with(r)) {
            return Err(inside_an_install(&target).with_detail(crate::paths::display_path(root)));
        }

        // The vault that is already open. Opening it a second time failed as
        // if another copy of the app held it.
        if let Some(current) = &open {
            if crate::paths::canonicalize_clean(current.vault_root()).ok().as_deref() == Some(target.as_path()) {
                return self.vault_info_of(current);
            }
        }

        let store = Arc::new(Store::open(path, create_if_missing)?);

        // A vault inside an install would make the engine move files into
        // themselves and then scan what it just wrote.
        let mut roots = known_roots;
        roots.extend(store.installs()?.into_iter().map(|i| i.root));
        for root in roots {
            if store.vault_root().starts_with(&root) {
                return Err(inside_an_install(store.vault_root()));
            }
        }

        // A copy into the vault cut off by a crash leaves its half-written file
        // in the vault's own folder, and nothing else would ever remove it.
        crate::apply::fsops::remove_leftovers(&store.temp_dir());

        let info = self.vault_info_of(&store)?;
        *self.store.write().map_err(|_| poisoned())? = Some(store);

        AppConfig { vault_root: Some(PathBuf::from(&info.root)) }.save(&self.config_path)?;
        Ok(info)
    }

    /// The folders inside one folder, for the folder picker. An empty path
    /// lists the drives.
    ///
    /// Folders Windows marks hidden or system, such as `$RECYCLE.BIN` and
    /// `System Volume Information`, are left out, and so are names starting
    /// with a dot.
    pub fn list_directory(&self, raw: &str) -> Result<crate::reply::DirectoryListing> {
        use crate::reply::{DirEntryInfo, DirectoryListing};
        if raw.trim().is_empty() {
            return Ok(DirectoryListing {
                path: String::new(),
                parent: None,
                entries: self
                    .platform
                    .drive_roots()
                    .into_iter()
                    .map(|p| DirEntryInfo {
                        name: crate::paths::display_path(&p),
                        path: crate::paths::display_path(&p),
                        is_directory: true,
                        is_symlink: false,
                    })
                    .collect(),
            });
        }
        let path = PathBuf::from(raw);
        let read = std::fs::read_dir(&path).map_err(|e| VaultError::from_io(&e, &path, "opening the folder"))?;

        let mut entries: Vec<DirEntryInfo> = Vec::new();
        for e in read.flatten() {
            let Ok(file_type) = e.file_type() else { continue };
            let p = e.path();
            let is_symlink = file_type.is_symlink();
            let is_directory = file_type.is_dir() || (is_symlink && p.is_dir());
            if !is_directory {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || hidden_by_windows(&e) {
                continue;
            }
            entries.push(DirEntryInfo {
                name,
                path: crate::paths::display_path(&p),
                is_directory,
                is_symlink,
            });
        }
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // A drive root's parent is the drive list, which is the empty path.
        // Without this, the picker cannot go back up and pick another drive.
        let parent = match path.parent() {
            Some(p) => Some(crate::paths::display_path(p)),
            None => Some(String::new()),
        };
        Ok(DirectoryListing { parent, path: crate::paths::display_path(&path), entries })
    }

    /// Facts about the open vault, without reopening it.
    ///
    /// The drive's free space is the most-read number in the application, and
    /// asking for it should not mean opening the vault again.
    pub fn vault_info(&self) -> Result<VaultInfo> {
        let store = self.store()?;
        self.vault_info_of(&store)
    }

    fn vault_info_of(&self, store: &Store) -> Result<VaultInfo> {
        let files = store.vault_files()?;
        let space = self.platform.disk_space(store.vault_root()).ok();
        Ok(VaultInfo {
            root: crate::paths::display_path(store.vault_root()),
            created_at: store.created_at()?,
            volume: self
                .platform
                .volume_id(store.vault_root())
                .map(|v| v.0)
                .unwrap_or_default(),
            free_bytes: space.map(|s| s.free_bytes),
            total_bytes: space.map(|s| s.total_bytes),
            file_count: files.len() as u64,
            total_stored_bytes: files.iter().map(|f| f.size_bytes).sum(),
            schema_version: crate::store::SCHEMA_VERSION,
        })
    }

    /// Closes the vault, so another vault or another copy of the app can open
    /// it. The files stay exactly as they are.
    pub fn close_vault(&self) -> Result<()> {
        if let Some(op) = self.busy() {
            return Err(VaultError::busy(op.kind.word()));
        }
        if let Ok(mut g) = self.store.write() {
            *g = None;
        }
        Ok(())
    }

    pub fn app_state(&self) -> Result<AppState> {
        let store = self.store.read().ok().and_then(|g| g.clone());
        let (installs, settings, last_scan, last_plan, interrupted) = match &store {
            Some(s) => (
                s.installs()?.len() as u64,
                s.settings()?,
                s.last_scan_id()?,
                s.last_plan_id()?,
                Applier::new(s, self.platform.as_ref())
                    .interrupted()?
                    .into_iter()
                    .map(|i| i.apply_id)
                    .collect(),
            ),
            None => (0, Settings::default(), None, None, Vec::new()),
        };

        Ok(AppState {
            vault_root: store.as_ref().map(|s| crate::paths::display_path(s.vault_root())),
            vault_initialized: store.is_some(),
            install_count: installs,
            platform: self.platform_report(),
            settings,
            last_scan_id: last_scan,
            last_plan_id: last_plan,
            interrupted_applies: interrupted,
            busy: self.busy(),
        })
    }

    // -- settings ---------------------------------------------------------

    pub fn settings(&self) -> Result<Settings> {
        self.store()?.settings()
    }

    pub fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings> {
        let store = self.store()?;
        let mut s = store.settings()?;
        s.apply_patch(patch);
        store.put_settings(&s)?;
        Ok(s)
    }

    // -- installs ---------------------------------------------------------

    pub fn validate_install_path(&self, path: &Path) -> Result<InstallCandidate> {
        detect::inspect(path)
    }

    pub fn register_install(&self, path: &Path, label: Option<String>) -> Result<Install> {
        let store = self.store()?;
        let candidate = detect::require_valid(path)?;
        let root = candidate.root.clone().expect("a valid candidate has a root");

        if store.install_at_root(&root)?.is_some() {
            return Err(VaultError::new(
                ErrorCode::AlreadyRegistered,
                "That install is already in the list.",
            )
            .with_path(&root));
        }
        if store.vault_root().starts_with(&root) {
            return Err(VaultError::conflict(
                "The vault folder is inside that install, so the app cannot manage it. Move the vault outside it first.",
            )
            .with_path(store.vault_root()));
        }

        let install = Install::from_candidate(
            uuid::Uuid::new_v4().to_string(),
            label.unwrap_or_else(|| Install::default_label(&root)),
            path.to_path_buf(),
            &candidate,
        )?;
        store.put_install(&install)?;
        Ok(install)
    }

    pub fn installs(&self) -> Result<Vec<Install>> {
        self.store()?.installs()
    }

    pub fn install(&self, id: &str) -> Result<Install> {
        self.store()?
            .install(id)?
            .ok_or_else(|| VaultError::not_found("That install is not registered any more."))
    }

    /// Re-reads the version and the extra model paths from the disk.
    pub fn refresh_install(&self, id: &str) -> Result<Install> {
        let store = self.store()?;
        let existing = self.install(id)?;
        let candidate = detect::require_valid(&existing.root)?;
        let mut updated = Install::from_candidate(
            existing.id.clone(),
            existing.label.clone(),
            existing.registered_path.clone(),
            &candidate,
        )?;
        updated.added_at = existing.added_at;
        updated.last_scan_at = existing.last_scan_at;
        store.put_install(&updated)?;
        Ok(updated)
    }

    pub fn rename_install(&self, id: &str, label: &str) -> Result<Install> {
        let store = self.store()?;
        let mut i = self.install(id)?;
        if label.trim().is_empty() {
            return Err(VaultError::invalid("A name cannot be empty."));
        }
        i.label = label.trim().to_string();
        store.put_install(&i)?;
        Ok(i)
    }

    /// Forgets an install. Never deletes a file and never removes a link.
    pub fn unregister_install(&self, id: &str) -> Result<u64> {
        let store = self.store()?;
        let links = store.links_for_install(id)?.len() as u64;
        if !store.delete_install(id)? {
            return Err(VaultError::not_found("That install is not registered any more."));
        }
        Ok(links)
    }

    pub fn install_model_dirs(&self, id: &str) -> Result<Vec<ModelDirNode>> {
        let store = self.store()?;
        Links::new(&store, self.platform.as_ref()).model_dirs(id)
    }

    // -- scan -------------------------------------------------------------

    /// Starts a scan on its own thread and returns straight away.
    pub fn start_scan(
        self: &Arc<Self>,
        install_ids: Option<Vec<String>>,
        sink: Arc<dyn ProgressSink<ScanProgress>>,
        on_done: Arc<dyn Fn(Result<ScanRecord>) + Send + Sync>,
    ) -> Result<String> {
        let store = self.store()?;
        let installs = self.installs_for(&store, install_ids)?;
        let scan_id = uuid::Uuid::new_v4().to_string();
        let cancel = self.take_slot(BusyKind::Scan, &scan_id)?;

        let engine = Arc::clone(self);
        let id = scan_id.clone();
        std::thread::Builder::new()
            .name("comfyvault-scan".into())
            .spawn(move || {
                let result = engine.run_scan(&store, &id, &installs, &cancel, sink.as_ref());
                engine.clear_slot();
                on_done(result);
            })
            .map_err(|e| {
                self.clear_slot();
                VaultError::new(ErrorCode::IoError, "The app could not start the scan.")
                    .with_detail(e.to_string())
            })?;
        Ok(scan_id)
    }

    /// Runs a scan on this thread. Used by a command line front end and by the
    /// tests.
    pub fn run_scan(
        &self,
        store: &Store,
        scan_id: &str,
        installs: &[Install],
        cancel: &CancelToken,
        sink: &dyn ProgressSink<ScanProgress>,
    ) -> Result<ScanRecord> {
        let settings = store.settings()?;
        let outcome = Scanner::new(store, self.platform.as_ref(), settings)
            .scan(scan_id, installs, cancel, sink)?;
        store.put_scan(&outcome.record)?;
        store.put_scan_entries(scan_id, &outcome.entries)?;

        for i in installs {
            if let Some(mut install) = store.install(&i.id)? {
                install.last_scan_at = Some(crate::time_util::Timestamp::now());
                install.last_scan_totals = outcome
                    .record
                    .per_install
                    .iter()
                    .find(|p| p.install_id == install.id)
                    .cloned();
                store.put_install(&install)?;
            }
        }
        Ok(outcome.record)
    }

    /// The installs to scan. Each is inspected on the disk again, and an
    /// install whose folder is not a ComfyUI install now is left out, so a row
    /// in a database from elsewhere cannot make the scan read a folder of its
    /// choosing.
    fn installs_for(&self, store: &Store, ids: Option<Vec<String>>) -> Result<Vec<Install>> {
        let all: Vec<Install> = store.installs()?.iter().filter_map(|i| i.proved().ok()).collect();
        match ids {
            None => Ok(all),
            Some(ids) => ids
                .iter()
                .map(|id| {
                    all.iter()
                        .find(|i| &i.id == id)
                        .cloned()
                        .ok_or_else(|| VaultError::not_found("One of those installs is not registered any more."))
                })
                .collect(),
        }
    }

    fn clear_slot(&self) {
        if let Ok(mut g) = self.busy.lock() {
            *g = None;
        }
    }

    pub fn last_scan(&self) -> Result<Option<ScanRecord>> {
        let store = self.store()?;
        match store.last_scan_id()? {
            Some(id) => store.scan(&id),
            None => Ok(None),
        }
    }

    /// A page of the files a scan found.
    pub fn scan_entries(
        &self,
        scan_id: &str,
        offset: u64,
        limit: u64,
        filter: &ScanEntryFilter,
    ) -> Result<ScanEntryPage> {
        let store = self.store()?;
        let entries = store.scan_entries(scan_id)?;
        if entries.is_empty() && store.scan(scan_id)?.is_none() {
            return Err(VaultError::not_found("That scan is not in this vault's history."));
        }
        // Counted over the whole scan, before the filter. "How many paths
        // hold these bytes" is a fact about what was found, not about what
        // the person is looking at right now.
        let mut per_hash: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
        for e in &entries {
            if let Some(h) = e.sha256.as_deref() {
                *per_hash.entry(h).or_insert(0) += 1;
            }
        }
        let counts: Vec<u64> = entries
            .iter()
            .map(|e| e.sha256.as_deref().and_then(|h| per_hash.get(h)).copied().unwrap_or(1))
            .collect();

        let kept: Vec<ScanEntryWithCount> = entries
            .into_iter()
            .zip(counts)
            .filter(|(e, _)| filter.matches(e))
            .map(|(entry, occurrence_count)| ScanEntryWithCount { entry, occurrence_count })
            .collect();

        let total = kept.len() as u64;
        let limit = (limit as usize).min(crate::vault::MAX_PAGE);
        Ok(ScanEntryPage {
            total,
            offset,
            entries: kept.into_iter().skip(offset as usize).take(limit).collect(),
        })
    }

    // -- plan -------------------------------------------------------------

    pub fn build_plan(&self, scan_id: &str) -> Result<ConsolidationPlan> {
        let store = self.store()?;
        if store.scan(scan_id)?.is_none() {
            return Err(VaultError::not_found("That scan is not in this vault's history."));
        }
        let entries = store.scan_entries(scan_id)?;
        let installs: Vec<Install> = store.installs()?.iter().filter_map(|i| i.proved().ok()).collect();
        let plan = Planner::new(&store, self.platform.as_ref()).build(
            &uuid::Uuid::new_v4().to_string(),
            scan_id,
            &entries,
            &installs,
        )?;
        store.put_plan(&plan)?;
        Ok(plan)
    }

    pub fn plan(&self, plan_id: &str) -> Result<ConsolidationPlan> {
        self.store()?
            .plan(plan_id)?
            .ok_or_else(|| VaultError::not_found("That plan is not in this vault's history."))
    }

    // -- apply ------------------------------------------------------------

    /// Starts an apply on its own thread and returns straight away.
    pub fn start_apply(
        self: &Arc<Self>,
        req: ApplyRequest,
        sink: Arc<dyn ProgressSink<ApplyProgress>>,
        on_done: Arc<dyn Fn(Result<ApplyRecord>) + Send + Sync>,
    ) -> Result<String> {
        let store = self.store()?;
        let plan = self.plan(&req.plan_id)?;
        let apply_id = uuid::Uuid::new_v4().to_string();
        let cancel = self.take_slot(BusyKind::Apply, &apply_id)?;

        let engine = Arc::clone(self);
        let id = apply_id.clone();
        std::thread::Builder::new()
            .name("comfyvault-apply".into())
            .spawn(move || {
                let result = Applier::new(&store, engine.platform.as_ref())
                    .apply(&id, &plan, &req, &cancel, sink.as_ref());
                engine.clear_slot();
                on_done(result);
            })
            .map_err(|e| {
                self.clear_slot();
                VaultError::new(ErrorCode::IoError, "The app could not start the consolidation.")
                    .with_detail(e.to_string())
            })?;
        Ok(apply_id)
    }

    /// What undoing a run would cost. Reads only.
    pub fn preview_revert(&self, apply_id: &str) -> Result<RevertPreview> {
        let store = self.store()?;
        Applier::new(&store, self.platform.as_ref()).preview_revert(apply_id)
    }

    /// Undoes a run, on its own thread.
    pub fn start_revert(
        self: &Arc<Self>,
        apply_id: String,
        sink: Arc<dyn ProgressSink<RevertProgress>>,
        on_done: Arc<dyn Fn(Result<ApplyRecord>) + Send + Sync>,
    ) -> Result<String> {
        let store = self.store()?;
        let cancel = self.take_slot(BusyKind::Revert, &apply_id)?;

        let engine = Arc::clone(self);
        let id = apply_id.clone();
        std::thread::Builder::new()
            .name("comfyvault-revert".into())
            .spawn(move || {
                let result = Applier::new(&store, engine.platform.as_ref())
                    .revert(&id, &cancel, sink.as_ref());
                engine.clear_slot();
                on_done(result);
            })
            .map_err(|e| {
                self.clear_slot();
                VaultError::new(ErrorCode::IoError, "The app could not start the undo.")
                    .with_detail(e.to_string())
            })?;
        Ok(apply_id)
    }

    /// Finishes a run that stopped in the middle, on its own thread.
    pub fn start_resume(
        self: &Arc<Self>,
        apply_id: String,
        sink: Arc<dyn ProgressSink<ApplyProgress>>,
        on_done: Arc<dyn Fn(Result<ApplyRecord>) + Send + Sync>,
    ) -> Result<String> {
        let store = self.store()?;
        let cancel = self.take_slot(BusyKind::Apply, &apply_id)?;

        let engine = Arc::clone(self);
        let id = apply_id.clone();
        std::thread::Builder::new()
            .name("comfyvault-resume".into())
            .spawn(move || {
                let result = Applier::new(&store, engine.platform.as_ref())
                    .resume(&id, &cancel, sink.as_ref());
                engine.clear_slot();
                on_done(result);
            })
            .map_err(|e| {
                self.clear_slot();
                VaultError::new(ErrorCode::IoError, "The app could not start the run.")
                    .with_detail(e.to_string())
            })?;
        Ok(apply_id)
    }

    pub fn apply_record(&self, apply_id: &str) -> Result<ApplyRecord> {
        self.store()?
            .apply(apply_id)?
            .ok_or_else(|| VaultError::not_found("That run is not in this vault's history."))
    }

    pub fn applies(&self) -> Result<Vec<ApplyRecord>> {
        self.store()?.applies()
    }

    pub fn interrupted_applies(&self) -> Result<Vec<InterruptedApply>> {
        let store = self.store()?;
        Applier::new(&store, self.platform.as_ref()).interrupted()
    }

    // -- links ------------------------------------------------------------

    pub fn create_link(&self, req: &CreateLinkRequest) -> Result<LinkRecord> {
        let store = self.store()?;
        Links::new(&store, self.platform.as_ref()).create(req)
    }

    pub fn remove_link(&self, link_id: &str) -> Result<()> {
        let store = self.store()?;
        Links::new(&store, self.platform.as_ref()).remove(link_id)
    }

    pub fn create_model_folder(&self, install_id: &str, relative_dir: &str) -> Result<(PathBuf, bool)> {
        let store = self.store()?;
        Links::new(&store, self.platform.as_ref()).create_folder(install_id, relative_dir)
    }

    pub fn links(
        &self,
        install_id: Option<&str>,
        sha256: Option<&str>,
        state: Option<LinkState>,
    ) -> Result<Vec<crate::links::LinkWithState>> {
        let store = self.store()?;
        Links::new(&store, self.platform.as_ref()).list(install_id, sha256, state)
    }

    // -- vault contents ---------------------------------------------------

    pub fn vault_files(
        &self,
        offset: u64,
        limit: u64,
        filter: &VaultFilter,
        sort: VaultSort,
        descending: bool,
    ) -> Result<VaultPage> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).list(offset, limit, filter, sort, descending)
    }

    /// One row per unique content, across the vault and the installs.
    pub fn contents(
        &self,
        offset: u64,
        limit: u64,
        filter: &VaultFilter,
        sort: VaultSort,
        descending: bool,
    ) -> Result<crate::vault::ContentPage> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).list_contents(offset, limit, filter, sort, descending)
    }

    pub fn name_groups(&self) -> Result<Vec<NameGroup>> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).name_groups()
    }

    pub fn set_canonical_name(&self, sha256: &str, name: &str) -> Result<VaultFile> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).set_canonical_name(sha256, name)
    }

    pub fn remove_alias(&self, sha256: &str, name: &str) -> Result<()> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).remove_alias(sha256, name)
    }

    pub fn orphans(&self) -> Result<Vec<VaultFile>> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).orphans()
    }

    pub fn delete_vault_file(&self, sha256: &str, confirm: &str) -> Result<u64> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).delete_file(sha256, confirm)
    }

    pub fn vault_health(&self) -> Result<VaultHealth> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).health()
    }

    pub fn remove_dangling_links(&self) -> Result<u64> {
        let store = self.store()?;
        Vault::new(&store, self.platform.as_ref()).remove_dangling_links()
    }

    // -- usage ------------------------------------------------------------

    pub fn check_usage(&self, names: &[String], install_ids: Option<Vec<String>>) -> Result<UsageReport> {
        let store = self.store()?;
        let installs = self.installs_for(&store, install_ids)?;
        crate::usage::check(&installs, names)
    }

    // -- metadata ---------------------------------------------------------

    pub fn metadata(&self, sha256: &str, refresh: bool) -> Result<Option<ModelMetadata>> {
        let store = self.store()?;
        let settings = store.settings()?;
        let transport = UreqTransport::new();
        let client = CivitaiClient::new(&transport);
        MetadataService::new(&store, &client, settings.metadata_lookups_enabled).get(sha256, refresh)
    }

    pub fn metadata_batch(&self, hashes: &[String], refresh: bool) -> Result<Vec<ModelMetadata>> {
        let store = self.store()?;
        let settings = store.settings()?;
        let transport = UreqTransport::new();
        let client = CivitaiClient::new(&transport);
        MetadataService::new(&store, &client, settings.metadata_lookups_enabled)
            .get_many(hashes, refresh)
    }

    pub fn clear_metadata_cache(&self) -> Result<u64> {
        Ok(self.store()?.clear_metadata()? as u64)
    }

    // -- running programs -------------------------------------------------

    pub fn running_comfy(&self) -> Result<Vec<RunningComfy>> {
        let store = self.store()?;
        let installs: Vec<(String, PathBuf)> = store
            .installs()?
            .into_iter()
            .map(|i| (i.id, i.root))
            .collect();
        Ok(crate::platform::match_processes_to_installs(
            &self.platform.list_processes(),
            &installs,
        ))
    }

    pub fn locked_files(&self, paths: &[PathBuf]) -> Vec<LockState> {
        paths.iter().map(|p| self.platform.lock_state(p)).collect()
    }
}

fn poisoned() -> VaultError {
    VaultError::new(ErrorCode::StoreError, "The app got into a bad state. Restart it.")
}

/// How to narrow a page of scan entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntryFilter {
    pub install_id: Option<String>,
    pub classification: Option<crate::store::Classification>,
    pub category: Option<String>,
    pub min_size_bytes: Option<u64>,
    pub name_contains: Option<String>,
    #[serde(default)]
    pub duplicates_only: bool,
}

impl ScanEntryFilter {
    fn matches(&self, e: &ScanEntryRecord) -> bool {
        if let Some(id) = &self.install_id {
            if &e.install_id != id {
                return false;
            }
        }
        if let Some(c) = self.classification {
            if e.classification != c {
                return false;
            }
        }
        if let Some(c) = &self.category {
            if &e.category != c {
                return false;
            }
        }
        if let Some(m) = self.min_size_bytes {
            if e.size_bytes < m {
                return false;
            }
        }
        if let Some(n) = &self.name_contains {
            let name = e.abs_path.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !name.contains(&n.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

/// A page of the files a scan found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntryPage {
    pub total: u64,
    pub offset: u64,
    pub entries: Vec<ScanEntryWithCount>,
}

/// One file a scan found, with how many paths in that scan hold its bytes.
///
/// The count is not stored on the record. It is a fact about the whole scan,
/// so keeping a copy on every row would be a second version of the same truth
/// that a later scan could leave behind. The fields of [`ScanEntryRecord`] sit
/// directly alongside `occurrenceCount`, not nested under a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntryWithCount {
    #[serde(flatten)]
    pub entry: ScanEntryRecord,
    pub occurrence_count: u64,
}

fn inside_an_install(path: &Path) -> VaultError {
    VaultError::conflict(
        "That folder is inside a ComfyUI install, so it cannot be the vault. Choose a folder outside every install.",
    )
    .with_path(path)
}

/// Does Windows mark this entry hidden or system?
#[cfg(windows)]
fn hidden_by_windows(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    e.metadata().map(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0).unwrap_or(false)
}

#[cfg(not(windows))]
fn hidden_by_windows(_e: &std::fs::DirEntry) -> bool {
    false
}

#[cfg(test)]
mod tests;
