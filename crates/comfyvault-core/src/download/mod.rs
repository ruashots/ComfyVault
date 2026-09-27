//! Downloading a model into the vault, and linking it in the installs the
//! person chooses.
//!
//! # The order of things
//!
//! 1. The address is read: the site says what the file is, how big, its
//!    SHA-256 when it knows it, and whether this person may have it. Nothing
//!    is downloaded, and a refusal shows here.
//! 2. The download waits its turn. One transfer runs at a time.
//! 3. The bytes go into a part file inside the vault's own folder, on the
//!    vault's drive. A stop, a dropped line or a closed app keeps the part,
//!    and Continue asks the site again and carries on from it.
//! 4. The whole file is hashed. A file that is not the one the site named is
//!    deleted, and nothing goes into the vault.
//! 5. The file is renamed into the vault, never over anything, and each
//!    chosen install gets its link, each step written to the journal first.
//!    Running this step again after a crash finishes it.

pub mod address;
pub mod folders;
pub mod http;
pub mod sites;
#[cfg(test)]
pub mod test_server;
pub mod tokens;
pub mod transfer;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use self::address::{AddressProblem, ModelAddress};
use self::http::Web;
use self::sites::{FileChoice, HfPage, Host, Reading, Refusal, RefusalKind, RemoteFile, Sites, VersionChoice};
use self::tokens::TokenStore;
use self::transfer::{Failure, FailureKind, Outcome};
use crate::error::{ErrorCode, Result, VaultError};
use crate::install::Install;
use crate::platform::Platform;
use crate::progress::{CancelToken, ProgressSink, Throttle};
use crate::store::{JournalEntry, JournalState, JournalStep, LinkOrigin, Store, VaultFileRecord};
use crate::time_util::Timestamp;

/// Room left free on the vault's drive beyond the file itself.
pub const SPACE_MARGIN: u64 = 5_000_000_000;

/// How a journal of a download is named.
pub const JOURNAL_PREFIX: &str = "download-";

// ---------------------------------------------------------------------------
// What the interface sees
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallTargetState {
    /// The link can go there.
    Free,
    /// That install already has a link to this model.
    HasLink,
    /// A different file has that name in a folder ComfyUI searches.
    NameTaken,
    /// The install's folder is not a ComfyUI install right now.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallTarget {
    pub install_id: String,
    pub link_path: Option<String>,
    pub state: InstallTargetState,
    /// Ticked by default: every install the first time, then the ones the
    /// person ticked last time.
    pub ticked: bool,
    /// Every folder ComfyUI reads for the category in this install, in its
    /// order. The link can go in one of them or in any folder inside one.
    pub roots: Vec<folders::Root>,
    /// Where the link goes unless the person picks another folder: the one
    /// they picked last time for this category, or else where ComfyUI saves
    /// new files of that kind.
    pub default_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlreadyInVault {
    pub vault_rel_path: String,
}

/// What the plan card shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressPlan {
    pub host: Host,
    pub title: String,
    pub subtitle: String,
    pub versions: Vec<VersionChoice>,
    pub version_id: Option<u64>,
    pub files: Vec<FileChoice>,
    pub file_id: Option<u64>,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub suggested_category: Option<String>,
    pub suggested_because: Option<String>,
    /// The folder this plan was worked out for.
    pub category: Option<String>,
    /// Every folder the menu offers.
    pub categories: Vec<String>,
    pub already_in_vault: Option<AlreadyInVault>,
    pub vault_rel_path: Option<String>,
    /// A different model already has this name in this vault folder. With a
    /// hash, `vaultRelPath` already carries the tag. Without one, the tag is
    /// added when the file is in hand.
    pub vault_name_taken: bool,
    pub installs: Vec<InstallTarget>,
    pub vault_free_bytes: Option<u64>,
    pub space_needed_bytes: u64,
    pub page: Option<HfPage>,
    pub model_id: Option<u64>,
}

/// The answer to reading an address: a plan, or a refusal. Never both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressReading {
    pub plan: Option<AddressPlan>,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Waiting,
    Running,
    Checking,
    Stopped,
    Failed,
    Mismatch,
    CutOff,
    Done,
    LinkedOnly,
}

impl DownloadState {
    fn is_active(self) -> bool {
        matches!(self, Self::Waiting | Self::Running | Self::Checking)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotLinked {
    pub install_id: String,
    pub reason: String,
}

/// One download, as the list shows it and as the vault remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub download_id: String,
    pub host: Host,
    pub title: String,
    pub file_name: String,
    pub category: String,
    pub vault_rel_path: String,
    pub install_ids: Vec<String>,
    pub linked_install_ids: Vec<String>,
    pub not_linked: Vec<NotLinked>,
    pub already_in_vault: bool,
    /// The file's SHA-256, once the file is proven: after the check, or at
    /// once when the vault already had it. `None` until then.
    pub sha256: Option<String>,
    pub state: DownloadState,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bytes_per_second: Option<u64>,
    pub error: Option<Failure>,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

/// A download as the vault keeps it: what the list shows, and what the
/// engine needs to carry on, which never goes to the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    #[serde(flatten)]
    pub download: Download,
    pub address: String,
    pub version_id: Option<u64>,
    pub file_id: Option<u64>,
    pub expected_sha256: Option<String>,
    /// The storage's name for the version the part came from.
    pub part_version: Option<String>,
    /// Queue order.
    pub seq: u64,
    /// The folder chosen for each install, where the person chose one.
    #[serde(default)]
    pub link_dirs: Vec<LinkChoice>,
}

impl std::ops::Deref for DownloadRecord {
    type Target = Download;
    fn deref(&self) -> &Download {
        &self.download
    }
}

impl std::ops::DerefMut for DownloadRecord {
    fn deref_mut(&mut self) -> &mut Download {
        &mut self.download
    }
}

impl DownloadRecord {
    fn journal_id(&self) -> String {
        format!("{JOURNAL_PREFIX}{}", self.download_id)
    }
}

/// What `start_download` is asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownload {
    pub address: String,
    #[serde(default)]
    pub version_id: Option<u64>,
    #[serde(default)]
    pub file_id: Option<u64>,
    pub category: String,
    #[serde(default)]
    pub install_ids: Vec<String>,
    /// The folder chosen for each install. Given, it replaces `install_ids`.
    #[serde(default)]
    pub links: Option<Vec<LinkChoice>>,
}

/// The folder a person chose for the link in one install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkChoice {
    pub install_id: String,
    pub dir: PathBuf,
}

/// The answer to asking about a saved token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatus {
    pub saved: bool,
    pub ok: Option<bool>,
    pub account: Option<String>,
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// The downloader
// ---------------------------------------------------------------------------

/// What a download needs from the engine around it.
#[derive(Clone)]
pub struct Context {
    pub store: Arc<Store>,
    pub platform: Arc<dyn Platform>,
    /// Held while anything changes a link or a name in the vault.
    pub vault_writes: Arc<Mutex<()>>,
}

pub struct Downloader {
    web: Arc<dyn Web>,
    sites: Sites,
    tokens: Arc<dyn TokenStore>,
    sink: Mutex<Option<Arc<dyn ProgressSink<Download>>>>,
    worker: Mutex<Worker>,
}

#[derive(Default)]
struct Worker {
    /// The download being transferred now, and how to stop it.
    running: Option<(String, CancelToken)>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// A worker is looking for work. Set and cleared under the lock, so a
    /// download queued while the worker is on its way out still starts.
    active: bool,
    /// Asked to stop taking new downloads, because the vault is closing.
    closing: bool,
}

impl Downloader {
    pub fn new(web: Arc<dyn Web>, sites: Sites, tokens: Arc<dyn TokenStore>) -> Self {
        Self { web, sites, tokens, sink: Mutex::new(None), worker: Mutex::new(Worker::default()) }
    }

    pub fn set_sink(&self, sink: Arc<dyn ProgressSink<Download>>) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    fn emit(&self, d: &Download) {
        if let Some(s) = self.sink.lock().unwrap().clone() {
            s.emit(d);
        }
    }

    // -- tokens -------------------------------------------------------------

    pub fn set_token(&self, host: Host, token: &str) -> Result<Option<String>> {
        let token = tokens::clean(token)?;
        match sites::check_token(self.web.as_ref(), &self.sites, host, &token)? {
            Ok(account) => {
                self.tokens.set(host, &token)?;
                Ok(account)
            }
            Err(said) => Err(VaultError::conflict(format!(
                "{} did not accept this token, so it was not saved.",
                host.name()
            ))
            .with_detail(said)),
        }
    }

    pub fn token_status(&self, host: Host) -> Result<TokenStatus> {
        let Some(token) = self.tokens.get(host)? else {
            return Ok(TokenStatus { saved: false, ok: None, account: None, message: None });
        };
        Ok(match sites::check_token(self.web.as_ref(), &self.sites, host, &token) {
            Ok(Ok(account)) => TokenStatus { saved: true, ok: Some(true), account, message: None },
            Ok(Err(said)) => TokenStatus { saved: true, ok: Some(false), account: None, message: Some(said) },
            Err(_) => TokenStatus {
                saved: true,
                ok: None,
                account: None,
                message: Some(format!("{} did not answer, so the token could not be checked.", host.name())),
            },
        })
    }

    pub fn remove_token(&self, host: Host) -> Result<bool> {
        self.tokens.remove(host)
    }

    // -- reading an address -------------------------------------------------

    /// What the plan card shows for an address.
    pub fn read_address(
        &self,
        ctx: &Context,
        text: &str,
        version_id: Option<u64>,
        file_id: Option<u64>,
        category: Option<&str>,
    ) -> Result<AddressReading> {
        let addr = match address::parse(text) {
            Ok(a) => a,
            Err(p) => return Ok(refused(p)),
        };
        let file = match self.read_site(ctx, &addr, version_id, file_id)? {
            Reading::File(f) => f,
            Reading::Refused(r) => return Ok(AddressReading { plan: None, refusal: Some(r) }),
        };
        if let Some(c) = category {
            validate_category(c)?;
        }
        let in_vault_category = match &file.sha256 {
            Some(sha) => ctx.store.vault_file(sha)?.map(|r| r.category),
            None => None,
        };
        // A model the vault already holds is linked from the folder it is in,
        // unless the person chose another.
        let category = category
            .map(str::to_string)
            .or(in_vault_category)
            .or_else(|| file.suggested_category.clone());
        let plan = self.plan_for(ctx, &file, category)?;
        Ok(AddressReading { plan: Some(plan), refusal: None })
    }

    fn read_site(&self, ctx: &Context, addr: &ModelAddress, version_id: Option<u64>, file_id: Option<u64>) -> Result<Reading> {
        let host = match addr {
            ModelAddress::HuggingFace { .. } => Host::HuggingFace,
            ModelAddress::Civitai { .. } => Host::Civitai,
        };
        let token = self.tokens.get(host)?;
        let settings = ctx.store.settings()?;
        let known = vault_categories(&ctx.store)?;
        sites::read(
            self.web.as_ref(),
            &self.sites,
            addr,
            version_id,
            file_id,
            token.as_deref(),
            &known,
            &|n: &str| settings.matches_extension(n),
        )
    }

    fn plan_for(&self, ctx: &Context, file: &RemoteFile, category: Option<String>) -> Result<AddressPlan> {
        let store = &ctx.store;
        let in_vault = match &file.sha256 {
            Some(sha) => store.vault_file(sha)?.filter(|r| store.vault_root().join(r.vault_rel_path()).is_file()),
            None => None,
        };
        let vault_rel_path = match (&in_vault, &category) {
            (Some(r), _) => Some(r.vault_rel_path()),
            (None, Some(c)) => Some(vault_name(ctx, c, &file.file_name, file.sha256.as_deref())?),
            (None, None) => None,
        };
        let last = store.last_download_installs()?;
        let target = in_vault.as_ref().map(|r| store.vault_root().join(r.vault_rel_path()));

        let mut installs = Vec::new();
        for i in store.installs()? {
            let ticked_before = last.as_ref().map(|l| l.contains(&i.id)).unwrap_or(true);
            let Ok(install) = i.proved() else {
                installs.push(InstallTarget {
                    install_id: i.id.clone(),
                    link_path: None,
                    state: InstallTargetState::Unavailable,
                    ticked: false,
                    roots: Vec::new(),
                    default_dir: None,
                });
                continue;
            };
            let Some(cat) = &category else {
                installs.push(InstallTarget {
                    install_id: i.id.clone(),
                    link_path: None,
                    state: InstallTargetState::Free,
                    ticked: ticked_before,
                    roots: Vec::new(),
                    default_dir: None,
                });
                continue;
            };
            let dir = default_dir(store, &install, cat)?;
            let link = dir.join(&file.file_name);
            let state = target_state_in(&install, cat, &dir, &file.file_name, target.as_deref());
            installs.push(InstallTarget {
                install_id: i.id.clone(),
                link_path: Some(crate::paths::display_path(&link)),
                ticked: match state {
                    InstallTargetState::Free => ticked_before,
                    InstallTargetState::HasLink => true,
                    _ => false,
                },
                state,
                roots: folders::roots(&install, cat),
                default_dir: Some(crate::paths::display_path(&dir)),
            });
        }

        let mut categories: Vec<String> = folders::built_in_categories().map(str::to_string).collect();
        categories.extend(vault_categories(store)?);
        if let Some(c) = &category {
            categories.push(c.clone());
        }
        categories.sort();
        categories.dedup();

        let vault_name_taken = match (&in_vault, &category) {
            (None, Some(c)) => name_taken_by_other(ctx, c, &file.file_name, file.sha256.as_deref())?,
            _ => false,
        };
        Ok(AddressPlan {
            host: file.host,
            title: file.title.clone(),
            subtitle: file.subtitle.clone(),
            versions: file.versions.clone(),
            version_id: file.version_id,
            files: file.files.clone(),
            file_id: file.file_id,
            file_name: file.file_name.clone(),
            size_bytes: file.size_bytes,
            sha256: file.sha256.clone(),
            suggested_category: file.suggested_category.clone(),
            suggested_because: file.suggested_because.clone(),
            category,
            categories,
            already_in_vault: in_vault.as_ref().map(|r| AlreadyInVault {
                vault_rel_path: crate::paths::display_path(&r.vault_rel_path()),
            }),
            vault_name_taken,
            vault_rel_path: vault_rel_path.map(|p| crate::paths::display_path(&p)),
            installs,
            vault_free_bytes: ctx.platform.disk_space(store.vault_root()).ok().map(|s| s.free_bytes),
            space_needed_bytes: if in_vault.is_some() { 0 } else { file.size_bytes + SPACE_MARGIN },
            page: file.page.clone(),
            model_id: file.model_id,
        })
    }

    // -- the list -----------------------------------------------------------

    pub fn list(&self, ctx: &Context) -> Result<Vec<Download>> {
        Ok(ctx.store.downloads()?.into_iter().map(|d| d.download).collect())
    }

    /// Queues one download. With the model already in the vault, makes the
    /// links at once and transfers nothing.
    pub fn start(self: &Arc<Self>, ctx: &Context, req: &StartDownload) -> Result<Download> {
        validate_category(&req.category)?;
        let installs = ctx.store.installs()?;
        let install_ids: Vec<String> = match &req.links {
            Some(links) => links.iter().map(|l| l.install_id.clone()).collect(),
            None => req.install_ids.clone(),
        };
        for id in &install_ids {
            if !installs.iter().any(|i| &i.id == id) {
                return Err(VaultError::not_found("One of the chosen installs is not registered any more."));
            }
        }
        // Every chosen folder is proved before anything is queued.
        let mut link_dirs = Vec::new();
        for choice in req.links.iter().flatten() {
            let install = ctx
                .store
                .install(&choice.install_id)?
                .ok_or_else(|| VaultError::not_found("One of the chosen installs is not registered any more."))?
                .proved()?;
            folders::inside_roots(&install, &req.category, &choice.dir)?;
            link_dirs.push(choice.clone());
        }
        let addr = address::parse(&req.address)
            .map_err(|_| VaultError::invalid("That is not a Hugging Face or Civitai address."))?;
        let file = match self.read_site(ctx, &addr, req.version_id, req.file_id)? {
            Reading::File(f) => f,
            Reading::Refused(r) => return Err(refusal_error(&r)),
        };
        ctx.store.put_last_download_installs(&install_ids)?;
        for c in &link_dirs {
            ctx.store.put_link_dir(&c.install_id, &req.category, &c.dir)?;
        }

        let seq = ctx.store.downloads()?.iter().map(|d| d.seq).max().map(|m| m + 1).unwrap_or(0);
        let mut d = DownloadRecord {
            download: Download {
                download_id: uuid::Uuid::new_v4().to_string(),
                host: file.host,
                title: file.title.clone(),
                file_name: file.file_name.clone(),
                category: req.category.clone(),
                vault_rel_path: String::new(),
                install_ids: install_ids.clone(),
                linked_install_ids: Vec::new(),
                not_linked: Vec::new(),
                already_in_vault: false,
                sha256: None,
                state: DownloadState::Waiting,
                bytes_done: 0,
                bytes_total: file.size_bytes,
                bytes_per_second: None,
                error: None,
                started_at: Some(Timestamp::now()),
                finished_at: None,
            },
            // Never the pasted text: it can carry a key.
            address: addr.stored_form(),
            version_id: file.version_id,
            file_id: file.file_id,
            expected_sha256: file.sha256.clone(),
            part_version: None,
            seq,
            link_dirs,
        };

        if let Some(sha) = &file.sha256 {
            if let Some(existing) = ctx.store.vault_file(sha)? {
                if ctx.store.vault_root().join(existing.vault_rel_path()).is_file() {
                    let _w = ctx.vault_writes.lock().map_err(|_| poisoned())?;
                    d.already_in_vault = true;
                    d.sha256 = Some(existing.sha256.clone());
                    d.vault_rel_path = crate::paths::display_path(&existing.vault_rel_path());
                    self.link_all(ctx, &mut d, &existing)?;
                    d.state = DownloadState::LinkedOnly;
                    d.bytes_total = 0;
                    d.finished_at = Some(Timestamp::now());
                    ctx.store.put_download(&d)?;
                    self.emit(&d);
                    return Ok(d.download);
                }
            }
        }

        check_space(ctx, file.size_bytes)?;
        d.vault_rel_path = crate::paths::display_path(&vault_name(ctx, &d.category, &d.file_name, file.sha256.as_deref())?);
        ctx.store.put_download(&d)?;
        self.emit(&d);
        self.kick(ctx);
        Ok(d.download)
    }

    /// Stops a download. The part already downloaded is kept.
    pub fn stop(&self, ctx: &Context, id: &str) -> Result<Download> {
        let mut d = find(ctx, id)?;
        if !d.state.is_active() {
            return Err(VaultError::conflict("That download is not running, so there is nothing to stop."));
        }
        if let Some((running, cancel)) = &self.worker.lock().unwrap().running {
            if running == id {
                cancel.cancel();
            }
        }
        d.state = DownloadState::Stopped;
        d.bytes_per_second = None;
        d.bytes_done = part_len(ctx, id);
        ctx.store.put_download(&d)?;
        self.emit(&d);
        Ok(d.download)
    }

    /// Puts a stopped, failed or cut-off download back in the queue.
    pub fn resume(self: &Arc<Self>, ctx: &Context, id: &str) -> Result<Download> {
        let mut d = find(ctx, id)?;
        // A waiting row that nothing is working on is one the worker gave up
        // on because the database refused to save it; continue tries again.
        let stranded = d.state == DownloadState::Waiting && !self.worker.lock().unwrap().active;
        if !stranded
            && !matches!(
                d.state,
                DownloadState::Stopped | DownloadState::Failed | DownloadState::CutOff | DownloadState::Mismatch
            )
        {
            return Err(VaultError::conflict("Only a stopped, failed or cut-off download can continue."));
        }
        if self.is_running(id) {
            return Err(VaultError::conflict("That download is still stopping. Try again in a moment."));
        }
        d.state = DownloadState::Waiting;
        d.error = None;
        d.bytes_done = part_len(ctx, id);
        ctx.store.put_download(&d)?;
        self.emit(&d);
        self.kick(ctx);
        Ok(d.download)
    }

    /// Deletes the part and forgets the download.
    pub fn discard(&self, ctx: &Context, id: &str) -> Result<()> {
        let d = find(ctx, id)?;
        if d.state.is_active() || self.is_running(id) {
            return Err(VaultError::conflict("Stop that download first, then discard it."));
        }
        let part = part_path(ctx, id);
        match std::fs::remove_file(&part) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(VaultError::from_io(&e, &part, "deleting the part already downloaded")),
        }
        ctx.store.delete_download(id)?;
        Ok(())
    }

    /// Takes a download off the list. Only one with no part to lose.
    pub fn remove(&self, ctx: &Context, id: &str) -> Result<()> {
        let d = find(ctx, id)?;
        let has_part = part_path(ctx, id).exists();
        let removable = match d.state {
            DownloadState::Waiting => !has_part && !self.is_running(id),
            DownloadState::Done | DownloadState::LinkedOnly | DownloadState::Mismatch => true,
            _ => false,
        };
        if !removable {
            return Err(VaultError::conflict(
                "That download has a part already downloaded. Discard it to delete the part, or continue it.",
            ));
        }
        ctx.store.delete_download(id)?;
        Ok(())
    }

    fn is_running(&self, id: &str) -> bool {
        self.worker.lock().unwrap().running.as_ref().map(|(r, _)| r == id).unwrap_or(false)
    }

    /// What a vault just opened should show: a transfer the app closed on is
    /// cut off, and a finished one is gone from the list.
    pub fn on_open(&self, ctx: &Context) -> Result<()> {
        for mut d in ctx.store.downloads()? {
            // Not one the engine made. It never reaches a path.
            if !is_download_id(&d.download_id) {
                ctx.store.delete_download(&d.download_id)?;
                continue;
            }
            match d.state {
                DownloadState::Done | DownloadState::LinkedOnly | DownloadState::Mismatch => {
                    ctx.store.delete_download(&d.download_id)?;
                }
                DownloadState::Running | DownloadState::Checking | DownloadState::Waiting => {
                    d.state = DownloadState::CutOff;
                    d.bytes_per_second = None;
                    d.bytes_done = part_len(ctx, &d.download_id);
                    ctx.store.put_download(&d)?;
                }
                _ => {}
            }
        }
        self.worker.lock().unwrap().closing = false;
        Ok(())
    }

    /// Stops the transfer and waits for it, before the vault is closed or
    /// another one opened. The part is kept, and the download shows as
    /// stopped when this vault opens again.
    pub fn close(&self, ctx: Option<&Context>) {
        // Marked first, so the attempt that ends because of the stop sees
        // it was stopped, rather than recording a dropped line.
        if let Some(ctx) = ctx {
            if let Ok(list) = ctx.store.downloads() {
                for mut d in list.into_iter().filter(|d| d.state.is_active()) {
                    d.state = DownloadState::Stopped;
                    d.bytes_per_second = None;
                    d.bytes_done = part_len(ctx, &d.download_id);
                    let _ = ctx.store.put_download(&d);
                }
            }
        }
        let thread = {
            let mut w = self.worker.lock().unwrap();
            w.closing = true;
            if let Some((_, cancel)) = &w.running {
                cancel.cancel();
            }
            w.thread.take()
        };
        if let Some(t) = thread {
            let _ = t.join();
        }
    }

    /// Starts the worker if it is not running. It takes waiting downloads in
    /// order until none is left.
    fn kick(self: &Arc<Self>, ctx: &Context) {
        let mut w = self.worker.lock().unwrap();
        if w.closing || w.active {
            return;
        }
        w.active = true;
        let me = Arc::clone(self);
        let ctx = ctx.clone();
        w.thread = Some(
            std::thread::Builder::new()
                .name("download".into())
                .spawn(move || me.work(&ctx))
                .expect("a thread for the downloads"),
        );
    }

    fn work(self: &Arc<Self>, ctx: &Context) {
        loop {
            let next = {
                let mut w = self.worker.lock().unwrap();
                if w.closing {
                    w.running = None;
                    w.active = false;
                    return;
                }
                let next = ctx
                    .store
                    .downloads()
                    .ok()
                    .and_then(|l| l.into_iter().find(|d| d.state == DownloadState::Waiting));
                match next {
                    Some(d) => {
                        let cancel = CancelToken::new();
                        w.running = Some((d.download_id.clone(), cancel.clone()));
                        Some((d, cancel))
                    }
                    None => {
                        w.running = None;
                        w.active = false;
                        None
                    }
                }
            };
            let Some((d, cancel)) = next else { return };
            let id = d.download_id.clone();
            let result = self.run_one(ctx, d, &cancel);
            if let Err(e) = result {
                // A failure the run could not turn into a state of its own,
                // such as the database refusing a write. Shown on the row.
                if let Ok(Some(mut d)) = ctx.store.download(&id) {
                    d.state = DownloadState::Failed;
                    d.bytes_per_second = None;
                    d.error = Some(
                        Failure::new(
                            FailureKind::Disk,
                            "The vault's database would not save this download, so it was stopped. Check that the vault's drive has free space, then continue it.",
                        )
                        .with_detail(e.detail.unwrap_or(e.message)),
                    );
                    let saved = ctx.store.put_download(&d).is_ok();
                    self.emit(&d);
                    if !saved {
                        // The row still says it is waiting, and picking it
                        // again would fail again, at once and for ever. The
                        // worker stops until the person acts.
                        let mut w = self.worker.lock().unwrap();
                        w.running = None;
                        w.active = false;
                        return;
                    }
                }
            }
            self.worker.lock().unwrap().running = None;
        }
    }

    /// Writes a download's new state, unless the person stopped it meanwhile.
    fn save(&self, ctx: &Context, d: &DownloadRecord) -> Result<bool> {
        if let Some(stored) = ctx.store.download(&d.download_id)? {
            if stored.state == DownloadState::Stopped && d.state.is_active() {
                return Ok(false);
            }
        }
        ctx.store.put_download(d)?;
        self.emit(d);
        Ok(true)
    }

    fn fail(&self, ctx: &Context, d: &mut DownloadRecord, failure: Failure) -> Result<()> {
        d.state = if failure.kind == FailureKind::Mismatch { DownloadState::Mismatch } else { DownloadState::Failed };
        d.bytes_per_second = None;
        d.bytes_done = part_len(ctx, &d.download_id);
        d.error = Some(failure);
        d.finished_at = Some(Timestamp::now());
        if let Some(mut stored) = ctx.store.download(&d.download_id)? {
            if stored.state == DownloadState::Stopped {
                // Stopped by the person while the attempt ended: stopped
                // wins, and keeps what the part now is, so it can continue.
                stored.part_version = d.part_version.clone();
                stored.bytes_done = d.bytes_done;
                stored.bytes_total = d.bytes_total;
                ctx.store.put_download(&stored)?;
                self.emit(&stored);
                return Ok(());
            }
        }
        ctx.store.put_download(d)?;
        self.emit(d);
        Ok(())
    }

    fn run_one(&self, ctx: &Context, mut d: DownloadRecord, cancel: &CancelToken) -> Result<()> {
        if !is_download_id(&d.download_id) {
            return Err(VaultError::invalid("That is not a download of this list."));
        }
        let part = part_path(ctx, &d.download_id);
        crate::apply::fsops::ensure_dir(&ctx.store.downloads_dir())?;

        // A crash after the file went into the vault: finish from there.
        if !part.exists() {
            if let Some(dest) = moved_to(ctx, &d)? {
                d.state = DownloadState::Checking;
                self.save(ctx, &d)?;
                return self.finish_moved(ctx, &mut d, &dest, cancel);
            }
        }

        d.state = DownloadState::Running;
        d.error = None;
        d.bytes_done = part_len(ctx, &d.download_id);
        if !self.save(ctx, &d)? {
            return Ok(());
        }

        // The address is read again every time: a signed storage address
        // from an earlier attempt may have expired, and the site may have
        // changed its mind about this person.
        let site = d.host.name();
        let addr = address::parse(&d.address).map_err(|_| VaultError::invalid("The saved address can no longer be read."))?;
        let file = match self.read_site(ctx, &addr, d.version_id, d.file_id) {
            Ok(Reading::File(f)) => f,
            Ok(Reading::Refused(r)) => {
                let message = match r.kind {
                    RefusalKind::NotFound => format!("{} no longer has this file.", d.host.name()),
                    _ => format!("{} refused the download.", d.host.name()),
                };
                return self.fail(
                    ctx,
                    &mut d,
                    Failure { kind: FailureKind::Refused, message, service_message: r.service_message, detail: None },
                );
            }
            Err(e) if e.code == ErrorCode::NetworkUnavailable => {
                return self.fail(
                    ctx,
                    &mut d,
                    Failure::new(FailureKind::Connection, format!("{site} did not answer. The part already downloaded is kept."))
                        .with_detail(e.detail.unwrap_or(e.message)),
                );
            }
            Err(e) => return Err(e),
        };
        if let (Some(want), Some(now)) = (&d.expected_sha256, &file.sha256) {
            if want != now {
                return self.fail(
                    ctx,
                    &mut d,
                    Failure::new(
                        FailureKind::ChangedOnSite,
                        format!("The file on {site} changed since this download started. Discard it, then read the address again."),
                    ),
                );
            }
        }
        if d.expected_sha256.is_none() {
            d.expected_sha256 = file.sha256.clone();
        }

        // Room for the rest of it, and the margin.
        let have = part_len(ctx, &d.download_id);
        if let Err(e) = check_space(ctx, file.size_bytes.saturating_sub(have)) {
            return self.fail(ctx, &mut d, Failure::new(FailureKind::NoSpace, e.message));
        }

        let token = self.tokens.get(file.host)?;
        let throttle = Throttle::per_second(4);
        let mut speed = Speed::default();
        let mut version = d.part_version.clone();
        let started_with = d.part_version.clone();
        let outcome = {
            let d_ref = &mut d;
            transfer::run(
                self.web.as_ref(),
                &file,
                token.as_deref(),
                &part,
                started_with.as_deref(),
                &mut version,
                cancel,
                &mut |done, total| {
                    d_ref.bytes_done = done;
                    if let Some(t) = total {
                        d_ref.bytes_total = t;
                    }
                    d_ref.bytes_per_second = speed.add(done);
                    if throttle.ready() {
                        self.emit(d_ref);
                    }
                },
            )
        };
        d.part_version = version;

        match outcome {
            Err(f) => self.fail(ctx, &mut d, f),
            Ok(Outcome::Stopped { .. }) => {
                // The row already says stopped. Keep what the part is.
                if let Some(mut stored) = ctx.store.download(&d.download_id)? {
                    stored.part_version = d.part_version.clone();
                    stored.bytes_done = part_len(ctx, &d.download_id);
                    stored.bytes_total = d.bytes_total;
                    ctx.store.put_download(&stored)?;
                    self.emit(&stored);
                }
                Ok(())
            }
            Ok(Outcome::Complete { total, .. }) => {
                d.bytes_done = total;
                d.bytes_total = total;
                d.bytes_per_second = None;
                d.state = DownloadState::Checking;
                if !self.save(ctx, &d)? {
                    return Ok(());
                }
                self.check_and_finish(ctx, &mut d, &part, cancel)
            }
        }
    }

    /// Hashes the part, and moves it into the vault only if it is the file.
    fn check_and_finish(&self, ctx: &Context, d: &mut DownloadRecord, part: &Path, cancel: &CancelToken) -> Result<()> {
        let sha = match crate::scan::hash::hash_file_cancellable(part, cancel) {
            Ok(s) => s.to_ascii_uppercase(),
            Err(e) if e.code == ErrorCode::Cancelled => return Ok(()),
            Err(e) => return self.fail(ctx, d, Failure::new(FailureKind::Disk, e.message)),
        };
        if let Some(want) = &d.expected_sha256 {
            if !want.eq_ignore_ascii_case(&sha) {
                let _ = std::fs::remove_file(part);
                d.part_version = None;
                return self.fail(
                    ctx,
                    d,
                    Failure::new(
                        FailureKind::Mismatch,
                        format!("The downloaded file did not match the SHA-256 {} gave, so it was deleted. Nothing went into the vault and nothing was linked.", d.host.name()),
                    ),
                );
            }
        }
        d.expected_sha256 = Some(sha.clone());
        let _w = ctx.vault_writes.lock().map_err(|_| poisoned())?;

        // Already in the vault: a file whose hash was only known now, or one
        // another download or run brought in meanwhile.
        if let Some(existing) = ctx.store.vault_file(&sha)? {
            if ctx.store.vault_root().join(existing.vault_rel_path()).is_file() {
                std::fs::remove_file(part).map_err(|e| VaultError::from_io(&e, part, "deleting the downloaded copy"))?;
                d.already_in_vault = true;
                d.vault_rel_path = crate::paths::display_path(&existing.vault_rel_path());
                self.link_all(ctx, d, &existing)?;
                return self.done(ctx, d);
            }
        }

        let rel = vault_name(ctx, &d.category, &d.file_name, Some(&sha))?;
        let dest = crate::paths::resolve_new_path_within(ctx.store.vault_root(), &rel)?;
        if let Some(dir) = dest.parent() {
            crate::apply::fsops::ensure_dir(dir)?;
        }
        let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(d.bytes_total);
        let mut journal = Journal::new(ctx, d);
        let seq = journal.pending(JournalStep::MoveToVault {
            from: part.to_path_buf(),
            to: dest.clone(),
            copied: false,
            sha256: sha.clone(),
            size_bytes: size,
        })?;
        // One call that refuses if anything is there by then.
        if let Err(e) = crate::platform::rename_new(part, &dest) {
            journal.mark(seq, JournalState::Failed)?;
            return self.fail(ctx, d, Failure::new(FailureKind::Disk, VaultError::from_io(&e, &dest, "moving the download into the vault").message));
        }
        journal.mark(seq, JournalState::Done)?;
        d.vault_rel_path = crate::paths::display_path(&rel);
        let record = self.record(ctx, &sha, &rel, size)?;
        self.link_all(ctx, d, &record)?;
        self.done(ctx, d)
    }

    /// After a crash that came after the move: prove the file, then finish.
    fn finish_moved(&self, ctx: &Context, d: &mut DownloadRecord, dest: &Path, cancel: &CancelToken) -> Result<()> {
        let rel = dest.strip_prefix(ctx.store.vault_root()).map(Path::to_path_buf).unwrap_or_default();
        let sha = match crate::scan::hash::hash_file_cancellable(dest, cancel) {
            Ok(s) => s.to_ascii_uppercase(),
            Err(e) if e.code == ErrorCode::Cancelled => return Ok(()),
            Err(e) => return self.fail(ctx, d, Failure::new(FailureKind::Disk, e.message)),
        };
        if d.expected_sha256.as_deref().map(|w| !w.eq_ignore_ascii_case(&sha)).unwrap_or(false) {
            return self.fail(
                ctx,
                d,
                Failure::new(FailureKind::Disk, "The file in the vault is not the one this download brought. Nothing was linked."),
            );
        }
        d.expected_sha256 = Some(sha.clone());
        let _w = ctx.vault_writes.lock().map_err(|_| poisoned())?;
        let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        let record = match ctx.store.vault_file(&sha)? {
            Some(r) => r,
            None => self.record(ctx, &sha, &rel, size)?,
        };
        d.vault_rel_path = crate::paths::display_path(&record.vault_rel_path());
        self.link_all(ctx, d, &record)?;
        self.done(ctx, d)
    }

    fn record(&self, ctx: &Context, sha: &str, rel: &Path, size: u64) -> Result<VaultFileRecord> {
        let record = VaultFileRecord {
            sha256: sha.to_string(),
            canonical_name: rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            category: rel.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            size_bytes: size,
            added_at: Timestamp::now(),
            aliases: Vec::new(),
        };
        ctx.store.put_vault_file(&record)?;
        Ok(record)
    }

    fn done(&self, ctx: &Context, d: &mut DownloadRecord) -> Result<()> {
        d.sha256 = d.expected_sha256.clone();
        d.state = if d.already_in_vault && d.bytes_done == 0 { DownloadState::LinkedOnly } else { DownloadState::Done };
        d.bytes_per_second = None;
        d.finished_at = Some(Timestamp::now());
        d.part_version = None;
        ctx.store.put_download(d)?;
        self.emit(d);
        Ok(())
    }

    /// Links the model in every chosen install, each step journaled. An
    /// install that cannot take its link is named, and the rest go on.
    /// Running it again skips what is already done.
    fn link_all(&self, ctx: &Context, d: &mut DownloadRecord, record: &VaultFileRecord) -> Result<()> {
        let target = ctx.store.vault_root().join(record.vault_rel_path());
        let links = crate::links::Links::new(&ctx.store, ctx.platform.as_ref());
        let mut journal = Journal::new(ctx, d);
        let mut made: Vec<PathBuf> = Vec::new();
        d.linked_install_ids.clear();
        d.not_linked.clear();
        for id in d.install_ids.clone() {
            let install = match ctx.store.install(&id)?.map(|i| i.proved()) {
                Some(Ok(i)) => i,
                _ => {
                    d.not_linked.push(NotLinked {
                        install_id: id,
                        reason: "That install's folder is not a ComfyUI install right now.".into(),
                    });
                    continue;
                }
            };
            let chosen = d.link_dirs.iter().find(|c| c.install_id == id).map(|c| c.dir.clone());
            let folder = match chosen {
                // Proved again: the disk may have changed since it was chosen.
                Some(dir) => match folders::inside_roots(&install, &d.category, &dir) {
                    Ok(_) => dir,
                    Err(e) => {
                        d.not_linked.push(NotLinked { install_id: id, reason: e.message });
                        continue;
                    }
                },
                None => default_dir(&ctx.store, &install, &d.category)?,
            };
            let path = folder.join(&d.file_name);
            // Two installs that share one folder share one link.
            if made.iter().any(|m| crate::paths::same_path_lexically(m, &path)) {
                d.linked_install_ids.push(id);
                continue;
            }
            match target_state_in(&install, &d.category, &folder, &d.file_name, Some(&target)) {
                InstallTargetState::HasLink => {
                    if ctx.store.link_at_path(&path)?.is_none() && leads_to(&path, &target) {
                        links_record(ctx, &install, &path, record, &journal.id)?;
                    }
                    made.push(path);
                    d.linked_install_ids.push(id);
                    continue;
                }
                InstallTargetState::NameTaken => {
                    d.not_linked.push(NotLinked {
                        install_id: id,
                        reason: "A different file with this name is in one of the folders ComfyUI searches in that install, so no link was made there.".into(),
                    });
                    continue;
                }
                _ => {}
            }
            // A folder the person made in the chooser exists only once the
            // link is made, and its making is journaled like the link.
            if !folder.is_dir() {
                let seq = journal.pending(JournalStep::CreateDir { path: folder.clone() })?;
                if let Err(e) = std::fs::create_dir_all(&folder) {
                    journal.mark(seq, JournalState::Failed)?;
                    d.not_linked.push(NotLinked { install_id: id, reason: VaultError::from_io(&e, &folder, "making the folder").message });
                    continue;
                }
                journal.mark(seq, JournalState::Done)?;
            }
            let seq = journal.pending(JournalStep::CreateLink { link: path.clone(), target: target.clone() })?;
            match links.create_in(&install, &folder, &record.sha256, &d.file_name, LinkOrigin::Download, Some(journal.id.clone())) {
                Ok(_) => {
                    journal.mark(seq, JournalState::Done)?;
                    made.push(path);
                    d.linked_install_ids.push(id);
                }
                Err(e) => {
                    journal.mark(seq, JournalState::Failed)?;
                    d.not_linked.push(NotLinked { install_id: id, reason: e.message });
                }
            }
        }
        Ok(())
    }
}

/// A download's journal: each step written before it happens.
struct Journal<'a> {
    store: &'a Store,
    id: String,
    group: String,
    next: u64,
    entries: Vec<JournalEntry>,
}

impl<'a> Journal<'a> {
    fn new(ctx: &'a Context, d: &DownloadRecord) -> Self {
        let id = d.journal_id();
        let next = ctx.store.journal(&id).map(|j| j.iter().map(|e| e.seq + 1).max().unwrap_or(0)).unwrap_or(0);
        Self { store: &ctx.store, id, group: d.download_id.clone(), next, entries: Vec::new() }
    }

    fn pending(&mut self, step: JournalStep) -> Result<usize> {
        let e = JournalEntry {
            apply_id: self.id.clone(),
            seq: self.next,
            group_id: self.group.clone(),
            step,
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        };
        self.next += 1;
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
}

/// Where a crashed download's journal says its file went, if it is there.
fn moved_to(ctx: &Context, d: &DownloadRecord) -> Result<Option<PathBuf>> {
    for e in ctx.store.journal(&d.journal_id())?.into_iter().rev() {
        if let JournalStep::MoveToVault { to, .. } = e.step {
            // The journal is read from the vault's database, so where it says
            // the file went is held to the vault before the file is read.
            let inside = crate::paths::is_within(ctx.store.vault_root(), &to) && !ctx.store.is_internal(&to);
            if matches!(e.state, JournalState::Done | JournalState::Pending) && inside && to.is_file() {
                return Ok(Some(to));
            }
        }
    }
    Ok(None)
}

fn links_record(ctx: &Context, install: &Install, path: &Path, record: &VaultFileRecord, journal: &str) -> Result<()> {
    ctx.store.put_link(&crate::store::LinkRecord {
        id: uuid::Uuid::new_v4().to_string(),
        install_id: install.id.clone(),
        rel_path: path.strip_prefix(&install.root).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf()),
        abs_path: path.to_path_buf(),
        link_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        sha256: record.sha256.clone(),
        vault_rel_path: record.vault_rel_path(),
        created_at: Timestamp::now(),
        created_by: LinkOrigin::Download,
        apply_id: Some(journal.to_string()),
    })
}

/// What an install already holds under this name, across every folder
/// ComfyUI searches for the category.
/// The same question for a link in `dir`, which can be a folder inside a
/// root. ComfyUI names a model by its path under the root, for example
/// `portraits\x.safetensors`, so that path is what is looked for in every
/// folder it searches.
fn target_state_in(install: &Install, category: &str, dir: &Path, name: &str, vault_file: Option<&Path>) -> InstallTargetState {
    let under = folders::roots(install, category)
        .into_iter()
        .find_map(|r| dir.strip_prefix(&r.path).ok().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut has_link = false;
    for folder in folders::searched(install, category) {
        let p = folder.join(&under).join(name);
        if std::fs::symlink_metadata(&p).is_err() {
            continue;
        }
        match vault_file {
            Some(v) if leads_to(&p, v) => has_link = true,
            _ => return InstallTargetState::NameTaken,
        }
    }
    if has_link {
        InstallTargetState::HasLink
    } else {
        InstallTargetState::Free
    }
}

/// Where a new link for this category goes in this install, unless the
/// person picks another folder: the folder they picked last time, while it is
/// still one ComfyUI reads, or else where ComfyUI saves new files of that
/// kind.
pub(crate) fn default_dir(store: &Store, install: &Install, category: &str) -> Result<PathBuf> {
    if let Some(dir) = store.link_dir(&install.id, category)? {
        if folders::inside_roots(install, category, &dir).is_ok() {
            return Ok(dir);
        }
    }
    Ok(folders::link_folder(install, category))
}

fn leads_to(link: &Path, target: &Path) -> bool {
    matches!((std::fs::canonicalize(link), std::fs::canonicalize(target)), (Ok(a), Ok(b)) if a == b)
}

/// The vault name for a new model: the plain name, or the name with the
/// start of its hash when a different model already has the plain one.
fn vault_name(ctx: &Context, category: &str, name: &str, sha: Option<&str>) -> Result<PathBuf> {
    let plain = PathBuf::from(category).join(name);
    let taken_by = ctx.store.vault_name_taken(category, name)?;
    let on_disk = std::fs::symlink_metadata(ctx.store.vault_root().join(&plain)).is_ok();
    let free = match (&taken_by, sha) {
        (Some(other), Some(s)) => other.eq_ignore_ascii_case(s),
        (Some(_), None) => false,
        (None, _) => !on_disk,
    };
    // With no hash yet, the tag is decided when the file is in hand.
    let Some(sha) = sha.filter(|_| !free) else { return Ok(plain) };
    let tag = sha[..8].to_ascii_uppercase();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (name, String::new()),
    };
    Ok(PathBuf::from(category).join(format!("{stem}__{tag}{ext}")))
}

fn name_taken_by_other(ctx: &Context, category: &str, name: &str, sha: Option<&str>) -> Result<bool> {
    let taken_by = ctx.store.vault_name_taken(category, name)?;
    Ok(match (taken_by, sha) {
        (Some(other), Some(s)) => !other.eq_ignore_ascii_case(s),
        (Some(_), None) => true,
        (None, _) => std::fs::symlink_metadata(ctx.store.vault_root().join(category).join(name)).is_ok(),
    })
}

fn vault_categories(store: &Store) -> Result<Vec<String>> {
    let mut out: Vec<String> = store.vault_files()?.into_iter().map(|f| f.category).collect();
    out.sort();
    out.dedup();
    Ok(out)
}

fn validate_category(c: &str) -> Result<()> {
    crate::paths::validate_file_name(c)
        .map_err(|_| VaultError::invalid("That folder name cannot be used. Choose one from the list."))?;
    if c.starts_with('.') {
        return Err(VaultError::invalid("That folder name cannot be used. Choose one from the list."));
    }
    Ok(())
}

fn check_space(ctx: &Context, needed: u64) -> Result<()> {
    // Not knowing is not room. The margin exists to keep the drive from
    // filling, and a drive that cannot say how full it is may be nearly so.
    let space = ctx.platform.disk_space(ctx.store.vault_root()).map_err(|e| {
        VaultError::new(
            ErrorCode::IoError,
            "ComfyVault could not read how much free space the vault's drive has, so the download did not start. Check that the drive is connected, then try again.",
        )
        .with_detail(e.detail.unwrap_or(e.message))
    })?;
    let want = needed + SPACE_MARGIN;
    if space.free_bytes < want {
        return Err(VaultError::new(
            ErrorCode::IoError,
            "There is not enough free space on the vault's drive for this file and the 5 GB kept free.",
        )
        .with_detail(format!("needs {want} bytes, {} free", space.free_bytes)));
    }
    Ok(())
}

fn part_path(ctx: &Context, id: &str) -> PathBuf {
    ctx.store.downloads_dir().join(format!("{id}.part"))
}

fn part_len(ctx: &Context, id: &str) -> u64 {
    std::fs::metadata(part_path(ctx, id)).map(|m| m.len()).unwrap_or(0)
}

fn find(ctx: &Context, id: &str) -> Result<DownloadRecord> {
    if !is_download_id(id) {
        return Err(VaultError::invalid("That is not a download of this list."));
    }
    ctx.store.download(id)?.ok_or_else(|| VaultError::not_found("That download is not in the list any more."))
}

/// A download's id is the name of its part file, so it must be exactly the
/// kind of id the engine makes: a UUID in its plain form. The vault's
/// database is a file anyone could have prepared, and `../` in an id would
/// name a file outside the vault.
fn is_download_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).map(|u| u.hyphenated().to_string() == id).unwrap_or(false)
}

fn poisoned() -> VaultError {
    VaultError::new(ErrorCode::StoreError, "The app got into a bad state. Restart it.")
}

fn refused(p: AddressProblem) -> AddressReading {
    let kind = match p {
        AddressProblem::Bad => RefusalKind::BadAddress,
        AddressProblem::HfRepoNotFile => RefusalKind::HfRepoNotFile,
    };
    let host = matches!(p, AddressProblem::HfRepoNotFile).then_some(Host::HuggingFace);
    AddressReading {
        plan: None,
        refusal: Some(Refusal { kind, host, service_message: None, page: None, title: None, subtitle: None }),
    }
}

fn refusal_error(r: &Refusal) -> VaultError {
    let site = r.host.map(Host::name).unwrap_or("The site");
    let message = match r.kind {
        RefusalKind::TokenMissing => format!("{site} needs your token for this model. Add it in Settings, then read the address again."),
        RefusalKind::TokenRejected => format!("{site} did not accept your token. Paste a new one in Settings."),
        RefusalKind::NoAccess => format!("Your {site} account has no access to this model yet."),
        RefusalKind::NotFound => format!("{site} has no such file."),
        RefusalKind::BadAddress | RefusalKind::HfRepoNotFile => "That is not the address of one model file.".to_string(),
    };
    let e = VaultError::conflict(message);
    match &r.service_message {
        Some(m) => e.with_detail(m.clone()),
        None => e,
    }
}

/// Bytes per second over the last five seconds.
#[derive(Default)]
struct Speed {
    samples: VecDeque<(Instant, u64)>,
}

impl Speed {
    fn add(&mut self, done: u64) -> Option<u64> {
        let now = Instant::now();
        self.samples.push_back((now, done));
        while self.samples.front().map(|(t, _)| now.duration_since(*t) > Duration::from_secs(5)).unwrap_or(false) {
            self.samples.pop_front();
        }
        let (t0, b0) = *self.samples.front()?;
        let secs = now.duration_since(t0).as_secs_f64();
        (secs >= 1.0).then(|| ((done.saturating_sub(b0)) as f64 / secs) as u64)
    }
}

#[cfg(test)]
mod tests;
