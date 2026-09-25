//! Everything the engine needs from the operating system, behind one trait.
//!
//! # Why this is a trait
//!
//! Two of the product's hard truths are Windows facts that Linux cannot
//! reproduce:
//!
//! * Windows only creates a symbolic link without administrator rights when
//!   Developer Mode is on.
//! * Windows refuses to move a file that another program holds open.
//!
//! Development happens on Linux, where symbolic links always work and an open
//! file moves without complaint. Writing the apply engine against the real
//! system would leave both failure paths untested until someone ran the
//! application on Windows.
//!
//! So the split is deliberate:
//!
//! * [`NativePlatform`] asks the operating system and does no interpretation.
//!   Its Windows half is small and cannot be exercised from Linux.
//! * The logic that *acts* on the answers is ordinary Rust in the other
//!   modules, and it is fully tested through [`FakePlatform`], which returns
//!   real answers for everything except the failures a test chooses to inject.
//!
//! A test that says "this file is locked" therefore drives the real apply
//! engine down its real locked-file path on Linux.
//!
//! # What is measured, not assumed
//!
//! [`Platform::symlink_capability`] does not read the operating system name and
//! conclude anything. It creates a symbolic link in a temporary folder, reads it
//! back, and deletes it. That result is the answer. The registry value and the
//! elevation flag are reported beside it to explain the answer, never to decide
//! it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, Result, VaultError};
use crate::time_util::Timestamp;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;

/// What this computer can actually do with symbolic links.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SymlinkCapability {
    /// The engine created a link, read it back, and deleted it.
    pub supported: bool,
    /// Why the test failed, when it failed.
    pub probe_error: Option<String>,
    /// Windows only. `None` on every other system.
    pub developer_mode: Option<bool>,
    /// The process runs with administrator rights.
    pub elevated: bool,
    /// One sentence to put in front of the person.
    pub guidance: Option<String>,
}

impl SymlinkCapability {
    pub fn working() -> Self {
        Self {
            supported: true,
            probe_error: None,
            developer_mode: None,
            elevated: false,
            guidance: None,
        }
    }
}

/// The sentence the interface shows when links cannot be created on Windows.
pub const DEVELOPER_MODE_GUIDANCE: &str = "Windows needs Developer Mode to create the links this app uses. Open Settings, go to System, then For developers, and turn Developer Mode on. You do not need to restart.";

/// What this computer is, and what it can do, as the contract defines it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformReport {
    pub os: String,
    pub symlinks: SymlinkCapability,
    /// Windows only. `None` on every other system.
    pub long_paths_enabled: Option<bool>,
}

/// The name the contract uses for this operating system.
pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Whether another program holds a file open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    pub path: String,
    pub locked: bool,
    /// `false` on systems that let a file move while it is open. On those
    /// systems `locked` is always `false` and means nothing.
    pub checkable: bool,
    pub detail: Option<String>,
}

impl LockState {
    pub fn unlocked(path: &Path, checkable: bool) -> Self {
        Self {
            path: crate::paths::display_path(path),
            locked: false,
            checkable,
            detail: None,
        }
    }

    pub fn locked(path: &Path, detail: impl Into<String>) -> Self {
        Self {
            path: crate::paths::display_path(path),
            locked: true,
            checkable: true,
            detail: Some(detail.into()),
        }
    }
}

/// Identifies one physical file, however many names point at it.
///
/// Two hard links are two names for one set of bytes. Removing one of them
/// frees nothing, because the bytes stay while any name remains. Without this
/// the headline "space you get back" counts those bytes twice, and that number
/// is the one the person presses Apply for and checks afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileIdentity(pub String);

/// Identifies the volume a path sits on, so the engine knows whether a move is a
/// rename or a copy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VolumeId(pub String);

/// Space on the volume that holds a path.
/// What kind of drive a letter is attached to.
///
/// The interface uses this to decide what to offer. A vault on a drive that
/// can be unplugged or that lives on another machine turns every link in every
/// install into a broken one the moment it goes away, so those are not the
/// same kind of suggestion as a drive bolted into the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    RamDisk,
    Unknown,
}

/// One drive on this computer, with how much room it has.
///
/// `freeBytes` and `totalBytes` are null together when the drive could not be
/// read: an empty card reader, or a network drive that is no longer answering.
/// They are never zero to mean "do not know", because zero of zero reads as a
/// full drive rather than as an unanswered question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub root: String,
    pub kind: DriveKind,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// One running process, as the operating system reports it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe_path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub command_line: Vec<String>,
    /// When the process started. `None` when the system would not say.
    #[serde(default)]
    pub started_at: Option<Timestamp>,
}

/// A running process that belongs to a registered install.
///
/// The last three fields are what tell a working ComfyUI from a process that
/// never exited. Each is `None` when the operating system could not answer,
/// which is never the same as "no": a false "not listening" reads to a person
/// as "safe to end", and could end a ComfyUI in the middle of its work.
///
/// Older payloads lack them, so each reads back as `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningComfy {
    pub pid: u32,
    pub name: String,
    pub exe_path: Option<String>,
    pub cwd: Option<String>,
    pub command_line: Vec<String>,
    pub matched_install_ids: Vec<String>,
    pub match_reason: MatchReason,
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    /// The TCP ports this process listens on. Empty means it serves nothing.
    #[serde(default)]
    pub listening_ports: Option<Vec<u16>>,
    /// Whether it holds open any model file the vault knows about.
    #[serde(default)]
    pub holds_model_files: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MatchReason {
    ExeUnderRoot,
    CwdUnderRoot,
    ArgUnderRoot,
}

/// Everything the engine asks the operating system for.
pub trait Platform: Send + Sync {
    fn create_file_symlink(&self, link: &Path, target: &Path) -> Result<()>;
    fn remove_symlink(&self, link: &Path) -> Result<()>;
    fn read_symlink(&self, link: &Path) -> Result<PathBuf>;
    fn symlink_capability(&self) -> SymlinkCapability;
    fn lock_state(&self, path: &Path) -> LockState;
    fn volume_id(&self, path: &Path) -> Result<VolumeId>;

    /// Which physical file this path names.
    ///
    /// `None` when the system cannot say, and then the caller must treat every
    /// path as its own file, which counts space conservatively rather than
    /// optimistically.
    fn file_identity(&self, path: &Path) -> Option<FileIdentity>;
    fn disk_space(&self, path: &Path) -> Result<DiskSpace>;
    fn list_processes(&self) -> Vec<ProcessInfo>;

    /// The TCP ports each of these processes listens on.
    ///
    /// The map has an entry for every process the system answered for, with
    /// an empty list for one that listens on nothing. A process with no entry
    /// is unknown, never "not listening".
    fn listening_ports(&self, pids: &[u32]) -> HashMap<u32, Vec<u16>>;

    /// Which of these processes hold any of these files open.
    ///
    /// Same rule: an entry for every process the system answered for, and no
    /// entry for one it could not.
    fn processes_holding(&self, pids: &[u32], files: &[PathBuf]) -> HashMap<u32, bool>;

    /// Windows only. `None` elsewhere.
    fn long_paths_enabled(&self) -> Option<bool>;

    /// Every place a folder picker can start from.
    ///
    /// On Windows that is every drive the computer has, not just `C:`. A person
    /// whose models live on `D:` must be able to reach them, and a picker that
    /// starts inside `C:\` can never get there.
    fn drive_roots(&self) -> Vec<PathBuf>;

    /// Every drive on this computer, with its size and its free space.
    ///
    /// Answers before a vault exists, because it is what the first screen uses
    /// to ask where the vault should go.
    fn drives(&self) -> Vec<DriveInfo>;

    /// Renames a file, and reports whether the operating system refused because
    /// the two paths sit on different volumes.
    ///
    /// The caller falls back to copy, verify, and delete. The fallback is driven
    /// by what the operating system actually said, not by a prediction, so a
    /// wrong volume guess can never cause data loss.
    fn rename(&self, from: &Path, to: &Path) -> std::result::Result<(), RenameError>;

    fn is_symlink(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }
}

/// Why a rename failed. `CrossVolume` is the one the caller recovers from.
#[derive(Debug)]
pub enum RenameError {
    CrossVolume,
    Io(std::io::Error),
}

impl RenameError {
    pub fn into_vault_error(self, path: &Path, doing: &str) -> VaultError {
        match self {
            RenameError::CrossVolume => VaultError::new(
                ErrorCode::IoError,
                "The vault is on a different drive, so the file has to be copied.",
            )
            .with_path(path),
            RenameError::Io(e) => VaultError::from_io(&e, path, doing),
        }
    }
}

// ---------------------------------------------------------------------------
// The real implementation
// ---------------------------------------------------------------------------

/// Asks the operating system. Interprets nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePlatform;

impl NativePlatform {
    pub fn new() -> Self {
        Self
    }
}

impl Platform for NativePlatform {
    fn create_file_symlink(&self, link: &Path, target: &Path) -> Result<()> {
        sys::create_file_symlink(link, target)
    }

    fn remove_symlink(&self, link: &Path) -> Result<()> {
        sys::remove_symlink(link)
    }

    fn read_symlink(&self, link: &Path) -> Result<PathBuf> {
        std::fs::read_link(link)
            .map_err(|e| VaultError::from_io(&e, link, "reading where the link points"))
    }

    fn symlink_capability(&self) -> SymlinkCapability {
        probe_symlink_capability(self)
    }

    fn lock_state(&self, path: &Path) -> LockState {
        sys::lock_state(path)
    }

    fn volume_id(&self, path: &Path) -> Result<VolumeId> {
        sys::volume_id(path)
    }

    fn file_identity(&self, path: &Path) -> Option<FileIdentity> {
        sys::file_identity(path)
    }

    fn disk_space(&self, path: &Path) -> Result<DiskSpace> {
        disk_space_via_sysinfo(path)
    }

    fn list_processes(&self) -> Vec<ProcessInfo> {
        list_processes_via_sysinfo()
    }

    fn listening_ports(&self, pids: &[u32]) -> HashMap<u32, Vec<u16>> {
        sys::listening_ports(pids)
    }

    fn processes_holding(&self, pids: &[u32], files: &[PathBuf]) -> HashMap<u32, bool> {
        sys::processes_holding(pids, files)
    }

    fn long_paths_enabled(&self) -> Option<bool> {
        sys::long_paths_enabled()
    }

    fn drive_roots(&self) -> Vec<PathBuf> {
        sys::drive_roots()
    }

    fn drives(&self) -> Vec<DriveInfo> {
        sys::drives()
    }

    fn rename(&self, from: &Path, to: &Path) -> std::result::Result<(), RenameError> {
        match std::fs::rename(from, to) {
            Ok(()) => Ok(()),
            Err(e) if sys::is_cross_volume_error(&e) => Err(RenameError::CrossVolume),
            Err(e) => Err(RenameError::Io(e)),
        }
    }
}

/// Marks `dst` sparse and compressed when `src` is, before any byte is
/// written. Returns whether `src` is sparse.
pub fn copy_storage_traits(src: &std::fs::File, dst: &std::fs::File) -> std::io::Result<bool> {
    sys::copy_storage_traits(src, dst)
}

/// Renames `from` to `to`, and refuses if anything sits at `to` by then. The
/// check and the rename are one call to the operating system.
pub fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    sys::rename_new(from, to)
}

/// The bytes a file occupies on its drive, or nothing if that cannot be read.
pub fn size_on_disk(path: &Path) -> Option<u64> {
    sys::size_on_disk(path)
}

/// Creates a link in a temporary folder, reads it back, and deletes it.
///
/// This is the only honest way to answer the question. Developer Mode can be on
/// while a policy still blocks the privilege, and an elevated process can create
/// links with Developer Mode off.
pub fn probe_symlink_capability(platform: &dyn Platform) -> SymlinkCapability {
    probe_symlink_capability_in(platform, &std::env::temp_dir())
}

/// The probe, with the folder to test in supplied by the caller.
///
/// The public entry point uses the system temporary folder. Tests pass their
/// own, so they can prove the probe cleans up after itself without racing other
/// tests that probe at the same time.
pub fn probe_symlink_capability_in(platform: &dyn Platform, parent: &Path) -> SymlinkCapability {
    let developer_mode = sys::developer_mode_enabled();
    let elevated = sys::is_elevated();

    let probe = (|| -> Result<()> {
        let dir = tempfile::Builder::new()
            .prefix("comfyvault-linkprobe-")
            .tempdir_in(parent)
            .map_err(|e| VaultError::from_io(&e, parent, "making a temporary folder"))?;
        let target = dir.path().join("target.bin");
        let link = dir.path().join("link.bin");
        std::fs::write(&target, b"probe")
            .map_err(|e| VaultError::from_io(&e, &target, "writing the test file"))?;
        platform.create_file_symlink(&link, &target)?;
        let read_back = platform.read_symlink(&link)?;
        if read_back != target {
            return Err(VaultError::new(
                ErrorCode::SymlinkUnsupported,
                "This computer created a link that points somewhere unexpected.",
            ));
        }
        // A link that cannot be removed is as bad as one that cannot be made.
        platform.remove_symlink(&link)?;
        Ok(())
    })();

    match probe {
        Ok(()) => SymlinkCapability {
            supported: true,
            probe_error: None,
            developer_mode,
            elevated,
            guidance: None,
        },
        Err(e) => SymlinkCapability {
            supported: false,
            probe_error: Some(e.to_string()),
            developer_mode,
            elevated,
            guidance: Some(if cfg!(windows) {
                DEVELOPER_MODE_GUIDANCE.to_string()
            } else {
                "This computer refused to create a symbolic link. Check the folder permissions."
                    .to_string()
            }),
        },
    }
}

fn disk_space_via_sysinfo(path: &Path) -> Result<DiskSpace> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // The right disk is the one whose mount point is the longest prefix of the
    // path. On Windows that is the drive; on Linux it can be a nested mount.
    let mut best: Option<(usize, DiskSpace)> = None;
    for disk in disks.list() {
        let mp = disk.mount_point();
        if path.starts_with(mp) {
            let len = mp.as_os_str().len();
            let space = DiskSpace {
                free_bytes: disk.available_space(),
                total_bytes: disk.total_space(),
            };
            if best.as_ref().map(|(l, _)| len > *l).unwrap_or(true) {
                best = Some((len, space));
            }
        }
    }
    best.map(|(_, s)| s).ok_or_else(|| {
        VaultError::new(ErrorCode::IoError, "Could not read how much space that drive has.")
            .with_path(path)
    })
}

fn list_processes_via_sysinfo() -> Vec<ProcessInfo> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().to_string(),
            exe_path: p.exe().map(Path::to_path_buf),
            cwd: p.cwd().map(Path::to_path_buf),
            command_line: p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
            // Zero is sysinfo's word for "could not read it".
            started_at: match p.start_time() {
                0 => None,
                secs => Some(Timestamp::from_millis(secs as i64 * 1000)),
            },
        })
        .collect()
}

/// Opens Windows Task Manager, where a person can end a process.
///
/// The engine never ends a process itself. This only puts the tool that can in
/// front of the person.
pub fn open_task_manager() -> Result<()> {
    sys::open_task_manager()
}

// ---------------------------------------------------------------------------
// Matching processes to installs: pure logic, fully tested
// ---------------------------------------------------------------------------

/// Decides which running processes belong to which registered install.
///
/// A process matches when its executable, its working directory, or one of its
/// arguments sits under that install root. The comparison is lexical and
/// case-insensitive on Windows, because a running process's paths cannot always
/// be canonicalized: the engine may lack the rights to open another user's
/// process image.
pub fn match_processes_to_installs(
    processes: &[ProcessInfo],
    installs: &[(String, PathBuf)],
) -> Vec<RunningComfy> {
    let mut out = Vec::new();

    for p in processes {
        if !looks_like_a_python_or_comfy_process(&p.name) {
            continue;
        }
        let mut matched: Vec<String> = Vec::new();
        let mut reason: Option<MatchReason> = None;

        for (id, root) in installs {
            let hit = if p.exe_path.as_deref().map(|e| under(root, e)).unwrap_or(false) {
                Some(MatchReason::ExeUnderRoot)
            } else if p.cwd.as_deref().map(|c| under(root, c)).unwrap_or(false) {
                Some(MatchReason::CwdUnderRoot)
            } else if p.command_line.iter().any(|a| under(root, Path::new(a))) {
                Some(MatchReason::ArgUnderRoot)
            } else {
                None
            };

            if let Some(r) = hit {
                matched.push(id.clone());
                // The strongest reason wins, so the interface shows the clearest
                // explanation when several apply.
                reason = Some(match (reason, r) {
                    (Some(MatchReason::ExeUnderRoot), _) => MatchReason::ExeUnderRoot,
                    (_, r) => r,
                });
            }
        }

        if !matched.is_empty() {
            out.push(RunningComfy {
                pid: p.pid,
                name: p.name.clone(),
                exe_path: p.exe_path.as_deref().map(crate::paths::display_path),
                cwd: p.cwd.as_deref().map(crate::paths::display_path),
                command_line: p.command_line.clone(),
                matched_install_ids: matched,
                match_reason: reason.unwrap_or(MatchReason::ArgUnderRoot),
                started_at: p.started_at,
                listening_ports: None,
                holds_model_files: None,
            });
        }
    }
    out
}

fn looks_like_a_python_or_comfy_process(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("python")
        || n.starts_with("pythonw")
        || n.contains("comfy")
        || n == "python3"
        // The Windows portable build ships its own interpreter.
        || n.starts_with("python_embeded")
}

/// Lexical containment. Case-insensitive on Windows.
fn under(root: &Path, candidate: &Path) -> bool {
    let root = crate::paths::lexical_normalize(root);
    let cand = crate::paths::lexical_normalize(candidate);
    if cfg!(windows) {
        let (r, c) = (
            root.to_string_lossy().to_lowercase(),
            cand.to_string_lossy().to_lowercase(),
        );
        // Compare component-wise so `C:\Comfy` does not match `C:\ComfyOther`.
        Path::new(&c).starts_with(Path::new(&r))
    } else {
        cand.starts_with(&root)
    }
}

// ---------------------------------------------------------------------------
// The fake, for tests
// ---------------------------------------------------------------------------

/// Wraps the real platform and injects the failures Linux cannot produce.
///
/// Everything not overridden goes to the real operating system, so a test drives
/// the real apply engine over real files and real links, and only the specific
/// condition under test is simulated.
#[derive(Default)]
pub struct FakePlatform {
    inner: NativePlatform,
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    locked: HashSet<PathBuf>,
    /// Paths where creating a link fails, with the error to return.
    symlink_failures: HashMap<PathBuf, VaultError>,
    /// Every link creation fails.
    symlinks_unsupported: bool,
    capability_override: Option<SymlinkCapability>,
    /// Paths whose volume is reported as this identifier.
    volume_overrides: Vec<(PathBuf, VolumeId)>,
    processes: Option<Vec<ProcessInfo>>,
    listening: Option<HashMap<u32, Vec<u16>>>,
    holding: Option<HashMap<u32, bool>>,
    /// The files the engine last asked about, so a test can see which.
    asked_about: Option<Vec<PathBuf>>,
    free_bytes_override: Option<u64>,
    disk_space_fails: bool,
    /// Paths where a rename fails outright, with the error kind to raise.
    rename_failures: HashMap<PathBuf, std::io::ErrorKind>,
    /// Stands in for a computer with several drives.
    drive_roots: Option<Vec<PathBuf>>,
    drives: Option<Vec<DriveInfo>>,
}

impl FakePlatform {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lock_file(&self, path: impl Into<PathBuf>) -> &Self {
        self.state.lock().unwrap().locked.insert(path.into());
        self
    }

    pub fn unlock_file(&self, path: &Path) -> &Self {
        self.state.lock().unwrap().locked.remove(path);
        self
    }

    pub fn fail_symlink_at(&self, path: impl Into<PathBuf>, err: VaultError) -> &Self {
        self.state.lock().unwrap().symlink_failures.insert(path.into(), err);
        self
    }

    pub fn set_symlinks_unsupported(&self, yes: bool) -> &Self {
        let mut s = self.state.lock().unwrap();
        s.symlinks_unsupported = yes;
        s.capability_override = Some(SymlinkCapability {
            supported: !yes,
            probe_error: yes.then(|| "developer mode is off".to_string()),
            developer_mode: Some(!yes),
            elevated: false,
            guidance: yes.then(|| DEVELOPER_MODE_GUIDANCE.to_string()),
        });
        drop(s);
        self
    }

    pub fn set_volume(&self, path: impl Into<PathBuf>, id: &str) -> &Self {
        self.state
            .lock()
            .unwrap()
            .volume_overrides
            .push((path.into(), VolumeId(id.to_string())));
        self
    }

    /// Makes the machine look like it has these drives.
    pub fn set_drives(&self, drives: Vec<DriveInfo>) -> &Self {
        self.state.lock().unwrap().drives = Some(drives);
        self
    }

    /// The volume override covering this path, longest prefix first.
    fn volume_override(&self, path: &Path) -> Option<VolumeId> {
        let s = self.state.lock().unwrap();
        s.volume_overrides
            .iter()
            .filter(|(p, _)| path.starts_with(p))
            .max_by_key(|(p, _)| p.as_os_str().len())
            .map(|(_, id)| id.clone())
    }

    pub fn fail_rename_at(&self, path: impl Into<PathBuf>, kind: std::io::ErrorKind) -> &Self {
        self.state.lock().unwrap().rename_failures.insert(path.into(), kind);
        self
    }

    pub fn set_processes(&self, procs: Vec<ProcessInfo>) -> &Self {
        self.state.lock().unwrap().processes = Some(procs);
        self
    }

    /// What the port table says. Processes left out are unknown.
    pub fn set_listening(&self, table: HashMap<u32, Vec<u16>>) -> &Self {
        self.state.lock().unwrap().listening = Some(table);
        self
    }

    /// Who holds a model file open. Processes left out are unknown.
    pub fn set_holding(&self, table: HashMap<u32, bool>) -> &Self {
        self.state.lock().unwrap().holding = Some(table);
        self
    }

    /// The files the last open-file question named, or `None` if none was asked.
    pub fn files_asked_about(&self) -> Option<Vec<PathBuf>> {
        self.state.lock().unwrap().asked_about.clone()
    }

    pub fn set_drive_roots(&self, roots: Vec<PathBuf>) -> &Self {
        self.state.lock().unwrap().drive_roots = Some(roots);
        self
    }

    /// Makes the drive stop answering, the way a card reader with nothing in
    /// it or a network drive that went away does.
    pub fn fail_disk_space(&self, yes: bool) -> &Self {
        self.state.lock().unwrap().disk_space_fails = yes;
        self
    }

    pub fn set_free_bytes(&self, bytes: u64) -> &Self {
        self.state.lock().unwrap().free_bytes_override = Some(bytes);
        self
    }

    pub fn clear_injections(&self) {
        *self.state.lock().unwrap() = FakeState::default();
    }
}

impl Platform for FakePlatform {
    fn create_file_symlink(&self, link: &Path, target: &Path) -> Result<()> {
        {
            let s = self.state.lock().unwrap();
            if s.symlinks_unsupported {
                return Err(VaultError::new(
                    ErrorCode::SymlinkUnsupported,
                    "This computer cannot create the links this app uses.",
                )
                .with_path(link));
            }
            if let Some(e) = s.symlink_failures.get(link) {
                return Err(e.clone());
            }
        }
        self.inner.create_file_symlink(link, target)
    }

    fn remove_symlink(&self, link: &Path) -> Result<()> {
        self.inner.remove_symlink(link)
    }

    fn read_symlink(&self, link: &Path) -> Result<PathBuf> {
        self.inner.read_symlink(link)
    }

    fn symlink_capability(&self) -> SymlinkCapability {
        if let Some(c) = self.state.lock().unwrap().capability_override.clone() {
            return c;
        }
        probe_symlink_capability(self)
    }

    fn lock_state(&self, path: &Path) -> LockState {
        if self.state.lock().unwrap().locked.contains(path) {
            return LockState::locked(path, "a test marked this file as held open");
        }
        // Report as checkable, because a test that injects locks is standing in
        // for Windows, where the answer is meaningful.
        LockState::unlocked(path, true)
    }

    fn volume_id(&self, path: &Path) -> Result<VolumeId> {
        let s = self.state.lock().unwrap();
        // Longest matching prefix wins, so a nested override beats its parent.
        let mut best: Option<(usize, VolumeId)> = None;
        for (p, id) in &s.volume_overrides {
            if path.starts_with(p) {
                let len = p.as_os_str().len();
                if best.as_ref().map(|(l, _)| len > *l).unwrap_or(true) {
                    best = Some((len, id.clone()));
                }
            }
        }
        if let Some((_, id)) = best {
            return Ok(id);
        }
        drop(s);
        self.inner.volume_id(path)
    }

    fn file_identity(&self, path: &Path) -> Option<FileIdentity> {
        self.inner.file_identity(path)
    }

    fn disk_space(&self, path: &Path) -> Result<DiskSpace> {
        if self.state.lock().unwrap().disk_space_fails {
            return Err(crate::VaultError::new(
                crate::ErrorCode::IoError,
                "Could not read how much space that drive has.",
            )
            .with_path(path));
        }
        let mut space = self.inner.disk_space(path)?;
        if let Some(free) = self.state.lock().unwrap().free_bytes_override {
            space.free_bytes = free;
        }
        Ok(space)
    }

    fn list_processes(&self) -> Vec<ProcessInfo> {
        if let Some(p) = self.state.lock().unwrap().processes.clone() {
            return p;
        }
        self.inner.list_processes()
    }

    fn listening_ports(&self, pids: &[u32]) -> HashMap<u32, Vec<u16>> {
        if let Some(t) = self.state.lock().unwrap().listening.clone() {
            return t.into_iter().filter(|(p, _)| pids.contains(p)).collect();
        }
        self.inner.listening_ports(pids)
    }

    fn processes_holding(&self, pids: &[u32], files: &[PathBuf]) -> HashMap<u32, bool> {
        let mut s = self.state.lock().unwrap();
        s.asked_about = Some(files.to_vec());
        if let Some(t) = s.holding.clone() {
            return t.into_iter().filter(|(p, _)| pids.contains(p)).collect();
        }
        drop(s);
        self.inner.processes_holding(pids, files)
    }

    fn long_paths_enabled(&self) -> Option<bool> {
        self.inner.long_paths_enabled()
    }

    fn drives(&self) -> Vec<DriveInfo> {
        if let Some(d) = self.state.lock().unwrap().drives.clone() {
            return d;
        }
        self.inner.drives()
    }

    fn drive_roots(&self) -> Vec<PathBuf> {
        if let Some(r) = self.state.lock().unwrap().drive_roots.clone() {
            return r;
        }
        self.inner.drive_roots()
    }

    /// Refuses a rename between two paths set on different volumes, exactly
    /// as Windows refuses one between two drives. Every rename is judged, the
    /// last one of a checked copy included, so a copy staged on the wrong
    /// drive fails here the way it does on a real one.
    fn rename(&self, from: &Path, to: &Path) -> std::result::Result<(), RenameError> {
        if let (Some(a), Some(b)) = (self.volume_override(from), self.volume_override(to)) {
            if a != b {
                return Err(RenameError::CrossVolume);
            }
        }
        {
            let s = self.state.lock().unwrap();
            if let Some(kind) = s.rename_failures.get(from) {
                return Err(RenameError::Io(std::io::Error::new(*kind, "injected by a test")));
            }
        }
        self.inner.rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, exe: Option<&str>, cwd: Option<&str>, args: &[&str]) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.to_string(),
            exe_path: exe.map(PathBuf::from),
            cwd: cwd.map(PathBuf::from),
            command_line: args.iter().map(|s| s.to_string()).collect(),
            started_at: None,
        }
    }

    #[test]
    fn the_real_platform_can_create_and_read_a_link() {
        // On Linux this always passes. On Windows it is the true answer.
        let p = NativePlatform::new();
        let cap = p.symlink_capability();
        if cfg!(unix) {
            assert!(cap.supported, "links must work on this system: {:?}", cap.probe_error);
            assert_eq!(cap.developer_mode, None, "developer mode is a Windows idea");
        }
    }

    #[test]
    fn the_probe_leaves_nothing_behind() {
        // The probe runs on every application start and on every health check.
        // A leak would fill the person's temporary folder with dead links.
        let own = tempfile::tempdir().unwrap();
        let p = NativePlatform::new();
        for _ in 0..5 {
            let cap = probe_symlink_capability_in(&p, own.path());
            if cfg!(unix) {
                assert!(cap.supported);
            }
        }
        let leftovers = std::fs::read_dir(own.path()).unwrap().count();
        assert_eq!(leftovers, 0, "the probe left a temporary folder behind");
    }

    #[test]
    fn the_probe_reports_failure_when_the_folder_cannot_be_used() {
        // A probe that cannot even create its test folder must answer "not
        // supported" with a reason, never panic and never claim success.
        let p = NativePlatform::new();
        let cap = probe_symlink_capability_in(&p, Path::new("/definitely/not/a/real/folder"));
        assert!(!cap.supported);
        assert!(cap.probe_error.is_some());
        assert!(cap.guidance.is_some());
    }

    #[test]
    fn a_fake_lock_makes_the_engine_see_a_held_file() {
        let f = FakePlatform::new();
        let p = PathBuf::from("/tmp/held.safetensors");
        assert!(!f.lock_state(&p).locked);
        f.lock_file(p.clone());
        let st = f.lock_state(&p);
        assert!(st.locked);
        assert!(st.checkable);
        assert!(st.detail.is_some());
    }

    #[test]
    fn a_fake_can_turn_symlink_support_off_entirely() {
        let f = FakePlatform::new();
        f.set_symlinks_unsupported(true);
        let cap = f.symlink_capability();
        assert!(!cap.supported);
        assert_eq!(cap.developer_mode, Some(false));
        assert_eq!(cap.guidance.as_deref(), Some(DEVELOPER_MODE_GUIDANCE));

        let d = tempfile::tempdir().unwrap();
        let err = f
            .create_file_symlink(&d.path().join("l"), &d.path().join("t"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SymlinkUnsupported);
    }

    #[test]
    fn the_fake_still_does_real_work_when_nothing_is_injected() {
        // This is the point of wrapping instead of replacing: a test exercises
        // the real link code unless it asks for a failure.
        let f = FakePlatform::new();
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("t.safetensors");
        let link = d.path().join("l.safetensors");
        std::fs::write(&target, b"weights").unwrap();

        f.create_file_symlink(&link, &target).unwrap();
        assert!(f.is_symlink(&link));
        assert_eq!(f.read_symlink(&link).unwrap(), target);
        assert_eq!(std::fs::read(&link).unwrap(), b"weights");

        f.remove_symlink(&link).unwrap();
        assert!(!link.exists());
        assert!(target.exists(), "removing the link must not touch the target");
    }

    #[test]
    fn volume_override_uses_the_longest_matching_prefix() {
        let f = FakePlatform::new();
        f.set_volume("/mnt", "D:");
        f.set_volume("/mnt/deep", "E:");
        assert_eq!(f.volume_id(Path::new("/mnt/x")).unwrap(), VolumeId("D:".into()));
        assert_eq!(f.volume_id(Path::new("/mnt/deep/x")).unwrap(), VolumeId("E:".into()));
    }

    #[test]
    fn a_rename_onto_a_taken_name_refuses_and_replaces_nothing() {
        // The last step of every copy. A name taken since the copy began must
        // not be replaced, and the refusal and the rename are one call.
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("copy.part");
        let to = d.path().join("model.safetensors");
        std::fs::write(&from, b"the copy").unwrap();
        std::fs::write(&to, b"written meanwhile").unwrap();

        let err = rename_new(&from, &to).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"written meanwhile");
        assert_eq!(std::fs::read(&from).unwrap(), b"the copy");

        std::fs::remove_file(&to).unwrap();
        rename_new(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"the copy");
        assert!(!from.exists());
    }

    #[test]
    fn a_rename_between_two_fake_drives_reports_the_recoverable_error() {
        let f = FakePlatform::new();
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("c/a");
        let b = d.path().join("d/b");
        let same = d.path().join("c/same");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        std::fs::write(&a, b"x").unwrap();
        f.set_volume(d.path().join("c"), "C:\\");
        f.set_volume(d.path().join("d"), "D:\\");

        assert!(matches!(f.rename(&a, &b), Err(RenameError::CrossVolume)));
        assert!(a.exists(), "a refused rename must not move anything");

        f.rename(&a, &same).unwrap();
        assert!(same.exists() && !a.exists(), "a rename on one drive goes through");
    }

    #[test]
    fn a_real_cross_volume_rename_is_reported_as_cross_volume_not_as_io() {
        // Proves the classification, using the one cross-device move available
        // without root: /dev/shm is a separate filesystem from the temp dir on
        // most Linux systems. The test skips itself when it is not.
        if !cfg!(unix) {
            return;
        }
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            return;
        }
        let here = tempfile::tempdir().unwrap();
        let native = NativePlatform::new();
        let (Ok(v1), Ok(v2)) = (native.volume_id(here.path()), native.volume_id(shm)) else {
            return;
        };
        if v1 == v2 {
            return; // same filesystem, nothing to prove here
        }

        let src = here.path().join("weights.safetensors");
        std::fs::write(&src, b"bytes").unwrap();
        let dst = shm.join(format!("comfyvault-xdev-{}.bin", std::process::id()));

        match native.rename(&src, &dst) {
            Err(RenameError::CrossVolume) => {}
            Err(RenameError::Io(e)) => panic!("a cross-device move was misread as a plain io error: {e}"),
            Ok(()) => {
                let _ = std::fs::remove_file(&dst);
                panic!("the two paths were supposed to be on different filesystems");
            }
        }
        assert!(src.exists(), "the source must survive a refused rename");
    }

    #[test]
    fn processes_match_an_install_by_working_directory() {
        let procs = vec![proc(
            101,
            "python.exe",
            Some("/usr/bin/python3"),
            Some("/installs/comfy-a"),
            &["python", "main.py"],
        )];
        let installs = vec![("id-a".to_string(), PathBuf::from("/installs/comfy-a"))];
        let got = match_processes_to_installs(&procs, &installs);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].matched_install_ids, vec!["id-a"]);
        assert_eq!(got[0].match_reason, MatchReason::CwdUnderRoot);
    }

    #[test]
    fn processes_match_an_install_by_an_argument() {
        let procs = vec![proc(
            102,
            "python.exe",
            Some("/usr/bin/python3"),
            Some("/somewhere/else"),
            &["python", "/installs/comfy-b/main.py", "--listen"],
        )];
        let installs = vec![("id-b".to_string(), PathBuf::from("/installs/comfy-b"))];
        let got = match_processes_to_installs(&procs, &installs);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].match_reason, MatchReason::ArgUnderRoot);
    }

    #[test]
    fn the_executable_is_the_strongest_reason() {
        let procs = vec![proc(
            103,
            "python.exe",
            Some("/installs/comfy-c/python_embeded/python.exe"),
            Some("/installs/comfy-c"),
            &["python", "/installs/comfy-c/main.py"],
        )];
        let installs = vec![("id-c".to_string(), PathBuf::from("/installs/comfy-c"))];
        let got = match_processes_to_installs(&procs, &installs);
        assert_eq!(got[0].match_reason, MatchReason::ExeUnderRoot);
    }

    #[test]
    fn a_sibling_folder_with_a_shared_prefix_does_not_match() {
        // `C:\ComfyUI` must not match a process running in `C:\ComfyUI-Other`.
        // A plain string prefix test gets this wrong and would tell the person
        // to close a program that is not holding anything.
        let procs = vec![proc(
            104,
            "python.exe",
            None,
            Some("/installs/comfy-other"),
            &["python", "main.py"],
        )];
        let installs = vec![("id".to_string(), PathBuf::from("/installs/comfy"))];
        assert!(match_processes_to_installs(&procs, &installs).is_empty());
    }

    #[test]
    fn unrelated_programs_are_ignored() {
        let procs = vec![
            proc(1, "chrome.exe", None, Some("/installs/comfy-a"), &[]),
            proc(2, "notepad.exe", None, Some("/installs/comfy-a"), &[]),
        ];
        let installs = vec![("id-a".to_string(), PathBuf::from("/installs/comfy-a"))];
        assert!(
            match_processes_to_installs(&procs, &installs).is_empty(),
            "only python and comfy processes matter"
        );
    }

    #[test]
    fn one_process_can_match_several_installs() {
        // A nested layout, which is exactly the launcher case.
        let procs = vec![proc(
            105,
            "python.exe",
            None,
            Some("/installs/outer/ComfyUI-Easy-Install/ComfyUI"),
            &[],
        )];
        let installs = vec![
            ("outer".to_string(), PathBuf::from("/installs/outer")),
            (
                "inner".to_string(),
                PathBuf::from("/installs/outer/ComfyUI-Easy-Install/ComfyUI"),
            ),
        ];
        let got = match_processes_to_installs(&procs, &installs);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].matched_install_ids.len(), 2);
    }

    #[test]
    fn the_real_process_list_finds_this_test_process() {
        // Proves the real probe returns usable data, rather than an empty list
        // that would make every lock check silently pass.
        let procs = NativePlatform::new().list_processes();
        assert!(!procs.is_empty());
        let me = std::process::id();
        assert!(procs.iter().any(|p| p.pid == me), "the probe did not see this process");
    }

    #[test]
    fn a_real_running_process_is_matched_to_a_real_directory() {
        // End to end over the real operating system: start a program whose
        // working directory is inside a fabricated install, then find it.
        if !cfg!(unix) {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        std::fs::create_dir_all(&root).unwrap();

        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&root)
            .spawn()
            .expect("spawn sleep");

        let procs = NativePlatform::new().list_processes();
        let mine: Vec<ProcessInfo> = procs
            .into_iter()
            .filter(|p| p.pid == child.id())
            .map(|mut p| {
                // Stand in for a ComfyUI process: the matcher filters by name,
                // and this exercises the containment half over real data.
                p.name = "python3".to_string();
                p
            })
            .collect();
        assert_eq!(mine.len(), 1, "the spawned process was not visible");

        let installs = vec![("real".to_string(), root.clone())];
        let got = match_processes_to_installs(&mine, &installs);
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(got.len(), 1, "a real process in a real folder was not matched");
        assert_eq!(got[0].match_reason, MatchReason::CwdUnderRoot);
    }

    /// A throwaway program that holds `held` open and listens on two ports,
    /// one IPv4 and one IPv6. Ended when dropped, so a failed assertion does
    /// not leave it running.
    struct Holder {
        child: std::process::Child,
        ports: Vec<u16>,
    }

    impl Holder {
        fn start(dir: &Path, held: &Path) -> Holder {
            use std::io::BufRead;
            let mut cmd = if cfg!(windows) {
                let script = format!(
                    "$f=[IO.File]::Open('{}','Open','Read','None'); \
                     $a=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0); $a.Start(); \
                     $b=[Net.Sockets.TcpListener]::new([Net.IPAddress]::IPv6Loopback,0); $b.Start(); \
                     [Console]::Out.WriteLine(\"$($a.LocalEndpoint.Port) $($b.LocalEndpoint.Port)\"); \
                     [Console]::Out.Flush(); Start-Sleep 60",
                    held.display()
                );
                let mut c = std::process::Command::new("powershell.exe");
                c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
                c
            } else {
                let mut c = std::process::Command::new("python3");
                c.args([
                    "-c",
                    "import socket,sys,time\n\
                     f=open(sys.argv[1],'rb')\n\
                     a=socket.socket(); a.bind(('127.0.0.1',0)); a.listen()\n\
                     b=socket.socket(socket.AF_INET6); b.bind(('::1',0)); b.listen()\n\
                     print(a.getsockname()[1], b.getsockname()[1], flush=True)\n\
                     time.sleep(60)",
                ]);
                c.arg(held);
                c
            };
            let mut child = cmd
                .current_dir(dir)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("start the throwaway program");
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            let ports = line.split_whitespace().map(|p| p.parse().unwrap()).collect();
            Holder { child, ports }
        }
    }

    impl Drop for Holder {
        fn drop(&mut self) {
            // This test's own child, and nothing else.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn a_real_process_is_seen_listening_and_holding_a_file() {
        // Over the real operating system: the port table, the open-file
        // question, and the start time, each against a process whose answers
        // are known because this test made them so.
        let d = tempfile::tempdir().unwrap();
        let held = d.path().join("held.safetensors");
        let other = d.path().join("other.safetensors");
        std::fs::write(&held, b"weights").unwrap();
        std::fs::write(&other, b"weights").unwrap();

        let before = Timestamp::now().as_millis();
        let h = Holder::start(d.path(), &held);
        let pid = h.child.id();
        assert_eq!(h.ports.len(), 2);
        let p = NativePlatform::new();

        let me = std::process::id();
        let ports = p.listening_ports(&[pid, me]);
        let mut got = ports.get(&pid).expect("the port table said nothing about it").clone();
        got.sort_unstable();
        let mut want = h.ports.clone();
        want.sort_unstable();
        assert_eq!(got, want, "the ports it listens on, IPv4 and IPv6, in host order");
        // Other tests in this process may listen, so only the entry is checked:
        // every process asked about is answered for.
        assert!(ports.contains_key(&me), "no answer for this test's own process");

        let holding = p.processes_holding(&[pid, me], &[held.clone()]);
        assert_eq!(holding.get(&pid), Some(&true), "it holds the file open");
        assert_eq!(holding.get(&me), Some(&false), "this test does not");
        let holding = p.processes_holding(&[pid], &[other.clone()]);
        assert_eq!(holding.get(&pid), Some(&false), "a file it never opened");

        let started = p
            .list_processes()
            .into_iter()
            .find(|q| q.pid == pid)
            .and_then(|q| q.started_at)
            .expect("no start time for a process this test started")
            .as_millis();
        // Whole seconds on some systems, so allow for the rounding.
        assert!(
            started >= before - 2_000 && started <= Timestamp::now().as_millis() + 1_000,
            "started at {started}, the test began at {before}"
        );
    }

    #[test]
    fn task_manager_is_asked_for_only_where_it_exists() {
        if cfg!(windows) {
            return; // Opening it is the ignored test below.
        }
        let err = open_task_manager().unwrap_err();
        assert!(err.message.contains("Windows"), "{}", err.message);
    }

    /// Opens the real Task Manager on this desktop, so it runs only when
    /// asked: `--ignored`.
    #[test]
    #[ignore]
    fn task_manager_opens() {
        open_task_manager().expect("Windows did not open Task Manager");
    }

    #[test]
    fn disk_space_answers_for_the_temp_directory() {
        let d = tempfile::tempdir().unwrap();
        let space = NativePlatform::new().disk_space(d.path()).unwrap();
        assert!(space.total_bytes > 0, "total space must be a real number");
        assert!(space.free_bytes <= space.total_bytes);
    }

    #[test]
    fn a_fake_free_space_override_is_honored() {
        let d = tempfile::tempdir().unwrap();
        let f = FakePlatform::new();
        f.set_free_bytes(1234);
        assert_eq!(f.disk_space(d.path()).unwrap().free_bytes, 1234);
    }
}


#[cfg(test)]
mod drive_tests {
    use super::*;

    #[test]
    fn a_drive_that_cannot_be_read_says_nothing_rather_than_zero() {
        // Zero of zero reads as a completely full drive. The interface would
        // draw a full meter for an empty card reader, which is a statement
        // about the drive rather than an admission that it was not readable.
        let unreadable = DriveInfo {
            root: "E:\\".into(),
            kind: DriveKind::Removable,
            free_bytes: None,
            total_bytes: None,
        };
        let v = serde_json::to_value(&unreadable).unwrap();
        assert!(v["freeBytes"].is_null(), "free space must be null, not a number");
        assert!(v["totalBytes"].is_null(), "total size must be null, not a number");
        assert_ne!(v["freeBytes"], serde_json::json!(0));
        assert_ne!(v["totalBytes"], serde_json::json!(0));

        let back: DriveInfo = serde_json::from_value(v).unwrap();
        assert_eq!(back, unreadable);
    }

    #[test]
    fn the_drives_this_machine_reports_are_real_or_they_are_unknown() {
        // Run against the real operating system, on whichever one this is.
        let drives = Platform::drives(&NativePlatform);
        assert!(!drives.is_empty(), "a computer has at least one drive");

        for d in &drives {
            assert!(!d.root.is_empty(), "a drive with no root");
            // The pair moves together. One known and one unknown would let the
            // interface compute a meter from half an answer.
            assert_eq!(
                d.free_bytes.is_some(),
                d.total_bytes.is_some(),
                "{} reported one figure without the other",
                d.root
            );
            if let (Some(free), Some(total)) = (d.free_bytes, d.total_bytes) {
                assert!(total > 0, "{} says it has no size at all", d.root);
                assert!(free <= total, "{} says more is free than exists", d.root);
            }
        }
    }
}

#[cfg(test)]
mod never_ends_a_process {
    /// The engine reports on a person's processes and never acts on them.
    /// Ending a ComfyUI is the person's decision, made in Task Manager.
    #[test]
    fn no_code_outside_the_tests_can_end_pause_or_signal_a_process() {
        let sources = [
            ("platform/mod.rs", include_str!("mod.rs")),
            ("platform/windows.rs", include_str!("windows.rs")),
            ("platform/unix.rs", include_str!("unix.rs")),
            ("engine.rs", include_str!("../engine.rs")),
        ];
        let forbidden = [
            "RmShutdown",
            "RmRestart",
            "TerminateProcess",
            "NtSuspendProcess",
            "DebugActiveProcess",
            "GenerateConsoleCtrlEvent",
            "libc::kill",
            ".kill(",
            "taskkill",
        ];
        let mut found = Vec::new();
        for (name, text) in sources {
            // Everything from the first test module on is test code, where the
            // tests end the throwaway programs they started themselves.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or(text);
            assert!(shipped.len() > 1000, "{name}: the shipped code was not found");
            for word in forbidden {
                if shipped.contains(word) {
                    found.push(format!("{name}: {word}"));
                }
            }
        }
        assert!(found.is_empty(), "the engine can end a process: {found:?}");
        // The code that asks about processes sits before the tests, where the
        // check above reads it.
        for (name, text) in &sources[1..3] {
            let shipped = text.split("#[cfg(test)]").next().unwrap();
            assert!(shipped.contains("fn processes_holding"), "{name}: moved past the tests");
        }
    }
}
