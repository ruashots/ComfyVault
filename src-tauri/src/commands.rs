//! One function per command in `docs/IPC-CONTRACT.md`.
//!
//! Every command follows the same shape:
//!
//! * It takes one argument object called `args`, with camel case fields.
//! * It runs the engine call on a blocking thread, so the window keeps
//!   painting while the disk is busy.
//! * It returns the engine's own error, which already carries a sentence for
//!   the person and a code for the interface.
//!
//! There are no rules in this file. Anything that decides something belongs in
//! `comfyvault-core`.

use std::path::PathBuf;
use std::sync::Arc;

use comfyvault_core::apply::{
    ApplyProgress, ApplyRequest, InterruptedApply, RevertPreview, RevertProgress,
};
use comfyvault_core::engine::{AppState, Engine, ScanEntryFilter, ScanEntryPage, VaultInfo};
use comfyvault_core::install::{Install, InstallCandidate};
use comfyvault_core::links::{CreateLinkRequest, LinkWithState, ModelDirNode};
use comfyvault_core::metadata::ModelMetadata;
use comfyvault_core::plan::ConsolidationPlan;
use comfyvault_core::platform::{DriveInfo, LockState, PlatformReport, RunningComfy};
use comfyvault_core::scan::ScanProgress;
use comfyvault_core::settings::{Settings, SettingsPatch};
use comfyvault_core::store::{ApplyRecord, LinkRecord, LinkState, ScanRecord};
use comfyvault_core::usage::UsageResult;
use comfyvault_core::vault::{
    ContentPage, NameGroup, VaultFile, VaultFilter, VaultHealth, VaultPage, VaultSort,
};
use comfyvault_core::{ErrorCode, VaultError};
use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::events;
use crate::AppEngine;

/// The small reply shapes live in the engine crate, so that their contract
/// samples come from the same serialiser the product uses.
pub use comfyvault_core::reply::{
    Cancelled, Cleared, CreatedDirectory, CreatedFolder, Deleted,
    DirectoryListing, Removed, RemovedLinks, StartedApply, StartedScan, UnregisterResult,
};

type Reply<T> = Result<T, VaultError>;

/// Runs engine work on a blocking thread.
///
/// Without this, a scan of a terabyte would hold the runtime thread that also
/// serves the window, and the interface would stop painting.
async fn blocking<T, F>(f: F) -> Reply<T>
where
    F: FnOnce() -> Reply<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| {
        VaultError::new(ErrorCode::IoError, "The app could not finish that just now.")
            .with_detail(e.to_string())
    })?
}

fn engine(state: &State<'_, AppEngine>) -> Arc<Engine> {
    Arc::clone(&state.0)
}

// ---------------------------------------------------------------------------
// Platform and application state
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_platform_report(state: State<'_, AppEngine>) -> Reply<PlatformReport> {
    let e = engine(&state);
    blocking(move || Ok(e.platform_report())).await
}

/// Every drive on this computer, with its size and its free space.
///
/// Answers before a vault exists.
#[tauri::command]
pub async fn list_drives(state: State<'_, AppEngine>) -> Reply<Vec<DriveInfo>> {
    let e = engine(&state);
    blocking(move || Ok(e.drives())).await
}

#[tauri::command]
pub async fn get_app_state(state: State<'_, AppEngine>) -> Reply<AppState> {
    let e = engine(&state);
    blocking(move || e.app_state()).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectVaultArgs {
    pub path: String,
    #[serde(default)]
    pub create_if_missing: bool,
}

#[tauri::command]
pub async fn select_vault(state: State<'_, AppEngine>, args: SelectVaultArgs) -> Reply<VaultInfo> {
    let e = engine(&state);
    blocking(move || e.select_vault(&PathBuf::from(&args.path), args.create_if_missing)).await
}

#[tauri::command]
pub async fn close_vault(state: State<'_, AppEngine>) -> Reply<()> {
    let e = engine(&state);
    blocking(move || e.close_vault()).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppEngine>) -> Reply<Settings> {
    let e = engine(&state);
    blocking(move || e.settings()).await
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppEngine>, args: SettingsPatch) -> Reply<Settings> {
    let e = engine(&state);
    blocking(move || e.update_settings(&args)).await
}

// ---------------------------------------------------------------------------
// Installs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PathArgs {
    pub path: String,
}

#[tauri::command]
pub async fn validate_install_path(
    state: State<'_, AppEngine>,
    args: PathArgs,
) -> Reply<InstallCandidate> {
    let e = engine(&state);
    blocking(move || e.validate_install_path(&PathBuf::from(&args.path))).await
}

#[derive(Deserialize)]
pub struct RegisterInstallArgs {
    pub path: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[tauri::command]
pub async fn register_install(
    state: State<'_, AppEngine>,
    args: RegisterInstallArgs,
) -> Reply<Install> {
    let e = engine(&state);
    blocking(move || e.register_install(&PathBuf::from(&args.path), args.label)).await
}

#[tauri::command]
pub async fn list_installs(state: State<'_, AppEngine>) -> Reply<Vec<Install>> {
    let e = engine(&state);
    blocking(move || e.installs()).await
}

#[derive(Deserialize)]
pub struct IdArgs {
    pub id: String,
}

#[tauri::command]
pub async fn refresh_install(state: State<'_, AppEngine>, args: IdArgs) -> Reply<Install> {
    let e = engine(&state);
    blocking(move || e.refresh_install(&args.id)).await
}

#[derive(Deserialize)]
pub struct UpdateInstallArgs {
    pub id: String,
    pub label: String,
}

#[tauri::command]
pub async fn update_install(
    state: State<'_, AppEngine>,
    args: UpdateInstallArgs,
) -> Reply<Install> {
    let e = engine(&state);
    blocking(move || e.rename_install(&args.id, &args.label)).await
}

#[tauri::command]
pub async fn unregister_install(
    state: State<'_, AppEngine>,
    args: IdArgs,
) -> Reply<UnregisterResult> {
    let e = engine(&state);
    blocking(move || {
        Ok(UnregisterResult {
            links_left_in_place: e.unregister_install(&args.id)?,
            removed: true,
        })
    })
    .await
}

#[tauri::command]
pub async fn list_install_model_dirs(
    state: State<'_, AppEngine>,
    args: IdArgs,
) -> Reply<Vec<ModelDirNode>> {
    let e = engine(&state);
    blocking(move || e.install_model_dirs(&args.id)).await
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartScanArgs {
    #[serde(default)]
    pub install_ids: Option<Vec<String>>,
}

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppEngine>,
    args: StartScanArgs,
) -> Reply<StartedScan> {
    let e = engine(&state);
    let sink = Arc::new(events::EventSink::<ScanProgress>::new(
        app.clone(),
        events::SCAN_PROGRESS,
    ));
    let done_app = app.clone();
    let scan_id = e.start_scan(
        args.install_ids,
        sink,
        Arc::new(move |result| {
            events::emit_result(&done_app, events::SCAN_DONE, events::SCAN_ERROR, result);
        }),
    )?;
    Ok(StartedScan { scan_id })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntriesArgs {
    pub scan_id: String,
    pub offset: u64,
    pub limit: u64,
    #[serde(default)]
    pub filter: Option<ScanEntryFilter>,
}

#[tauri::command]
pub async fn get_scan_entries(
    state: State<'_, AppEngine>,
    args: ScanEntriesArgs,
) -> Reply<ScanEntryPage> {
    let e = engine(&state);
    blocking(move || {
        e.scan_entries(
            &args.scan_id,
            args.offset,
            args.limit,
            &args.filter.unwrap_or_default(),
        )
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIdArgs {
    pub scan_id: String,
}

#[tauri::command]
pub async fn cancel_scan(state: State<'_, AppEngine>, args: ScanIdArgs) -> Reply<Cancelled> {
    let e = engine(&state);
    blocking(move || {
        e.cancel(&args.scan_id)?;
        Ok(Cancelled { cancelled: true })
    })
    .await
}

#[tauri::command]
pub async fn get_last_scan(state: State<'_, AppEngine>) -> Reply<Option<ScanRecord>> {
    let e = engine(&state);
    blocking(move || e.last_scan()).await
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn build_plan(
    state: State<'_, AppEngine>,
    args: ScanIdArgs,
) -> Reply<ConsolidationPlan> {
    let e = engine(&state);
    blocking(move || e.build_plan(&args.scan_id)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanIdArgs {
    pub plan_id: String,
}

#[tauri::command]
pub async fn get_plan(state: State<'_, AppEngine>, args: PlanIdArgs) -> Reply<ConsolidationPlan> {
    let e = engine(&state);
    blocking(move || e.plan(&args.plan_id)).await
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_apply(
    app: AppHandle,
    state: State<'_, AppEngine>,
    args: ApplyRequest,
) -> Reply<StartedApply> {
    let e = engine(&state);
    let sink = Arc::new(events::EventSink::<ApplyProgress>::new(
        app.clone(),
        events::APPLY_PROGRESS,
    ));
    let done_app = app.clone();
    let apply_id = e.start_apply(
        args,
        sink,
        Arc::new(move |result| {
            events::emit_result(&done_app, events::APPLY_DONE, events::APPLY_ERROR, result);
        }),
    )?;
    Ok(StartedApply { apply_id })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyIdArgs {
    pub apply_id: String,
}

#[tauri::command]
pub async fn cancel_apply(state: State<'_, AppEngine>, args: ApplyIdArgs) -> Reply<Cancelled> {
    let e = engine(&state);
    blocking(move || {
        e.cancel(&args.apply_id)?;
        Ok(Cancelled { cancelled: true })
    })
    .await
}

#[tauri::command]
pub async fn get_apply_result(
    state: State<'_, AppEngine>,
    args: ApplyIdArgs,
) -> Reply<ApplyRecord> {
    let e = engine(&state);
    blocking(move || e.apply_record(&args.apply_id)).await
}

#[tauri::command]
pub async fn list_applies(state: State<'_, AppEngine>) -> Reply<Vec<ApplyRecord>> {
    let e = engine(&state);
    blocking(move || e.applies()).await
}

#[tauri::command]
pub async fn get_interrupted_applies(
    state: State<'_, AppEngine>,
) -> Reply<Vec<InterruptedApply>> {
    let e = engine(&state);
    blocking(move || e.interrupted_applies()).await
}

#[tauri::command]
pub async fn resume_apply(
    app: AppHandle,
    state: State<'_, AppEngine>,
    args: ApplyIdArgs,
) -> Reply<StartedApply> {
    let e = engine(&state);
    let sink = Arc::new(events::EventSink::<ApplyProgress>::new(
        app.clone(),
        events::APPLY_PROGRESS,
    ));
    let done_app = app.clone();
    let apply_id = e.start_resume(
        args.apply_id,
        sink,
        Arc::new(move |result| {
            events::emit_result(&done_app, events::APPLY_DONE, events::APPLY_ERROR, result);
        }),
    )?;
    Ok(StartedApply { apply_id })
}

#[tauri::command]
pub async fn preview_revert(
    state: State<'_, AppEngine>,
    args: ApplyIdArgs,
) -> Reply<RevertPreview> {
    let e = engine(&state);
    blocking(move || e.preview_revert(&args.apply_id)).await
}

#[tauri::command]
pub async fn revert_apply(
    app: AppHandle,
    state: State<'_, AppEngine>,
    args: ApplyIdArgs,
) -> Reply<StartedApply> {
    let e = engine(&state);
    let sink = Arc::new(events::EventSink::<RevertProgress>::new(
        app.clone(),
        events::REVERT_PROGRESS,
    ));
    let done_app = app.clone();
    let apply_id = e.start_revert(
        args.apply_id,
        sink,
        Arc::new(move |result| {
            events::emit_result(&done_app, events::REVERT_DONE, events::REVERT_ERROR, result);
        }),
    )?;
    Ok(StartedApply { apply_id })
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_link(
    state: State<'_, AppEngine>,
    args: CreateLinkRequest,
) -> Reply<LinkRecord> {
    let e = engine(&state);
    blocking(move || e.create_link(&args)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkIdArgs {
    pub link_id: String,
}

#[tauri::command]
pub async fn remove_link(state: State<'_, AppEngine>, args: LinkIdArgs) -> Reply<Removed> {
    let e = engine(&state);
    blocking(move || {
        e.remove_link(&args.link_id)?;
        Ok(Removed { removed: true })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFolderArgs {
    pub install_id: String,
    pub relative_dir: String,
}

#[tauri::command]
pub async fn create_model_folder(
    state: State<'_, AppEngine>,
    args: CreateFolderArgs,
) -> Reply<CreatedFolder> {
    let e = engine(&state);
    blocking(move || {
        let (path, created) = e.create_model_folder(&args.install_id, &args.relative_dir)?;
        Ok(CreatedFolder {
            abs_path: comfyvault_core::paths::display_path(&path),
            created,
        })
    })
    .await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListLinksArgs {
    #[serde(default)]
    pub install_id: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub state: Option<LinkState>,
}

#[tauri::command]
pub async fn list_links(
    state: State<'_, AppEngine>,
    args: Option<ListLinksArgs>,
) -> Reply<Vec<LinkWithState>> {
    let e = engine(&state);
    let args = args.unwrap_or_default();
    blocking(move || e.links(args.install_id.as_deref(), args.sha256.as_deref(), args.state)).await
}

// ---------------------------------------------------------------------------
// Vault contents
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListVaultArgs {
    pub offset: u64,
    pub limit: u64,
    #[serde(default)]
    pub filter: Option<VaultFilter>,
    #[serde(default)]
    pub sort: Option<VaultSort>,
    #[serde(default)]
    pub descending: bool,
}

#[tauri::command]
pub async fn list_vault_files(
    state: State<'_, AppEngine>,
    args: ListVaultArgs,
) -> Reply<VaultPage> {
    let e = engine(&state);
    blocking(move || {
        e.vault_files(
            args.offset,
            args.limit,
            &args.filter.unwrap_or_default(),
            args.sort.unwrap_or(VaultSort::Name),
            args.descending,
        )
    })
    .await
}

/// One row per unique content, across the vault and the installs.
///
/// This is the Library's own listing. Stitching it from a plan plus a vault
/// listing means paging two lists to draw one screen, which is fine at a
/// hundred models and is not fine at ten thousand.
#[tauri::command]
pub async fn list_contents(
    state: State<'_, AppEngine>,
    args: ListVaultArgs,
) -> Reply<ContentPage> {
    let e = engine(&state);
    blocking(move || {
        e.contents(
            args.offset,
            args.limit,
            &args.filter.unwrap_or_default(),
            args.sort.unwrap_or(VaultSort::Size),
            args.descending,
        )
    })
    .await
}

/// The open vault's facts, above all how much room its drive has left.
#[tauri::command]
pub async fn get_vault_info(state: State<'_, AppEngine>) -> Reply<VaultInfo> {
    let e = engine(&state);
    blocking(move || e.vault_info()).await
}

#[tauri::command]
pub async fn list_name_groups(state: State<'_, AppEngine>) -> Reply<Vec<NameGroup>> {
    let e = engine(&state);
    blocking(move || e.name_groups()).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameArgs {
    pub sha256: String,
    pub name: String,
}

#[tauri::command]
pub async fn set_canonical_name(
    state: State<'_, AppEngine>,
    args: NameArgs,
) -> Reply<VaultFile> {
    let e = engine(&state);
    blocking(move || e.set_canonical_name(&args.sha256, &args.name)).await
}

#[tauri::command]
pub async fn remove_alias(state: State<'_, AppEngine>, args: NameArgs) -> Reply<Removed> {
    let e = engine(&state);
    blocking(move || {
        e.remove_alias(&args.sha256, &args.name)?;
        Ok(Removed { removed: true })
    })
    .await
}

#[tauri::command]
pub async fn list_orphans(state: State<'_, AppEngine>) -> Reply<Vec<VaultFile>> {
    let e = engine(&state);
    blocking(move || e.orphans()).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteVaultFileArgs {
    pub sha256: String,
    /// Must equal `sha256`. This delete cannot be undone.
    pub confirm: String,
}

#[tauri::command]
pub async fn delete_vault_file(
    state: State<'_, AppEngine>,
    args: DeleteVaultFileArgs,
) -> Reply<Deleted> {
    let e = engine(&state);
    blocking(move || {
        let bytes_freed = e.delete_vault_file(&args.sha256, &args.confirm)?;
        Ok(Deleted { deleted: true, bytes_freed })
    })
    .await
}

#[tauri::command]
pub async fn check_vault_health(state: State<'_, AppEngine>) -> Reply<VaultHealth> {
    let e = engine(&state);
    blocking(move || e.vault_health()).await
}

#[tauri::command]
pub async fn remove_dangling_links(state: State<'_, AppEngine>) -> Reply<RemovedLinks> {
    let e = engine(&state);
    blocking(move || Ok(RemovedLinks { removed: e.remove_dangling_links()? })).await
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageArgs {
    pub names: Vec<String>,
    #[serde(default)]
    pub install_ids: Option<Vec<String>>,
}

/// Returns one answer per requested name, as the contract defines it.
///
/// The report the engine produces also says how many workflow files were
/// searched. That does not fit a flat list, so when nothing could be searched
/// the engine puts it in the `method` sentence instead, which the interface
/// already shows beside every answer.
#[tauri::command]
pub async fn check_model_usage(
    state: State<'_, AppEngine>,
    args: UsageArgs,
) -> Reply<Vec<UsageResult>> {
    let e = engine(&state);
    blocking(move || Ok(e.check_usage(&args.names, args.install_ids)?.results)).await
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataArgs {
    pub sha256: String,
    #[serde(default)]
    pub refresh: bool,
}

#[tauri::command]
pub async fn get_metadata(
    state: State<'_, AppEngine>,
    args: MetadataArgs,
) -> Reply<Option<ModelMetadata>> {
    let e = engine(&state);
    blocking(move || e.metadata(&args.sha256, args.refresh)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataBatchArgs {
    pub sha256: Vec<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[tauri::command]
pub async fn fetch_metadata_batch(
    state: State<'_, AppEngine>,
    args: MetadataBatchArgs,
) -> Reply<Vec<ModelMetadata>> {
    let e = engine(&state);
    blocking(move || e.metadata_batch(&args.sha256, args.refresh)).await
}

#[tauri::command]
pub async fn clear_metadata_cache(state: State<'_, AppEngine>) -> Reply<Cleared> {
    let e = engine(&state);
    blocking(move || Ok(Cleared { cleared: e.clear_metadata_cache()? })).await
}

// ---------------------------------------------------------------------------
// Running programs and locked files
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_running_comfy(state: State<'_, AppEngine>) -> Reply<Vec<RunningComfy>> {
    let e = engine(&state);
    blocking(move || e.running_comfy()).await
}

#[derive(Deserialize)]
pub struct PathsArgs {
    pub paths: Vec<String>,
}

#[tauri::command]
pub async fn check_locked_files(
    state: State<'_, AppEngine>,
    args: PathsArgs,
) -> Reply<Vec<LockState>> {
    let e = engine(&state);
    blocking(move || {
        let paths: Vec<PathBuf> = args.paths.iter().map(PathBuf::from).collect();
        Ok(e.locked_files(&paths))
    })
    .await
}

// ---------------------------------------------------------------------------
// The folder picker
// ---------------------------------------------------------------------------
//
// The interface needs to walk the disk so the person can choose a vault folder
// or an install folder without typing a path. These two commands exist so that
// walking can go through the engine instead of granting the window broad file
// system access of its own.

#[derive(Deserialize)]
pub struct ListDirectoryArgs {
    /// Absent or empty means the drives, or the root folder.
    #[serde(default)]
    pub path: Option<String>,
}

/// Lists the folders inside one folder, for the picker.
///
/// Only folders come back, because the picker only ever chooses a folder.
/// A folder that cannot be read is reported as an error with the path in it,
/// rather than being silently shown as empty.
///
/// With no path, the answer is **every drive**, not the contents of `C:`. A
/// person whose models live on `D:` has to be able to reach them, and a picker
/// that starts inside one drive can never leave it.
#[tauri::command]
pub async fn list_directory(
    state: State<'_, AppEngine>,
    args: ListDirectoryArgs,
) -> Reply<DirectoryListing> {
    let e = engine(&state);
    blocking(move || e.list_directory(&args.path.unwrap_or_default()))
    .await
}

#[derive(Deserialize)]
pub struct CreateDirectoryArgs {
    pub path: String,
}

/// Creates a folder the person named in the picker.
///
/// This is how a new vault folder gets made. It is deliberately not bounded to
/// an install: a vault can live anywhere, and the engine checks separately that
/// it is not inside one.
#[tauri::command]
pub async fn create_directory(args: CreateDirectoryArgs) -> Reply<CreatedDirectory> {
    blocking(move || {
        let path = PathBuf::from(&args.path);
        if let Some(name) = path.file_name() {
            comfyvault_core::paths::validate_file_name(&name.to_string_lossy())?;
        }

        // One folder, inside a folder the picker is already showing. Without
        // this, create_dir_all builds a whole tree at any path on the machine,
        // which is the half of the classic pairing that turns one rendering
        // mistake into a machine-wide problem.
        let parent = path
            .parent()
            .ok_or_else(|| VaultError::invalid("Choose where the new folder should go."))?;
        if !parent.is_dir() {
            return Err(VaultError::new(
                ErrorCode::NotFound,
                "That folder's parent does not exist, so the folder was not created. Browse to where it should go first.",
            )
            .with_path(parent));
        }

        let existed = path.is_dir();
        if !existed {
            std::fs::create_dir(&path)
                .map_err(|e| VaultError::from_io(&e, &path, "creating the folder"))?;
        }
        Ok(CreatedDirectory {
            path: comfyvault_core::paths::display_path(&path),
            created: !existed,
        })
    })
    .await
}
