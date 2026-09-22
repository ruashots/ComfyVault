//! The Windows half of [`crate::platform`].
//!
//! This module holds the two facts the product exists to be honest about, and
//! it is the only part of the engine that cannot be exercised from Linux.
//! Everything here is a thin question to the operating system with no logic of
//! its own, so the untestable surface stays as small as possible. Every caller
//! interprets the answers in ordinary Rust, which is tested through
//! [`crate::platform::FakePlatform`].
//!
//! ## Developer Mode
//!
//! Windows only lets an unprivileged process create a symbolic link when
//! Developer Mode is on. Rust's `symlink_file` already passes
//! `SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE`, so the call succeeds under
//! Developer Mode and fails without it. [`developer_mode_enabled`] reads the
//! registry value that the Settings switch writes, for the explanation only.
//! The answer the engine acts on comes from the live probe in the parent
//! module.
//!
//! ## A file another program holds open
//!
//! Windows refuses to rename or delete a file that another program has open
//! without `FILE_SHARE_DELETE`. [`lock_state`] asks for exclusive access. A
//! sharing violation means somebody holds it. The check is deliberately a
//! little stricter than a rename needs, so it can report "held" for a file that
//! might in fact have moved. That direction is the safe one: the engine refuses
//! to touch it and says why.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
    GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetLogicalDrives, GetVolumePathNameW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING,
};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::error::{ErrorCode, Result, VaultError};
use crate::platform::{LockState, VolumeId};

/// Encodes a path as a NUL-terminated wide string for the Windows API.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_str(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(super) fn create_file_symlink(link: &Path, target: &Path) -> Result<()> {
    // `symlink_file` is correct here because the engine only ever links a file.
    // A directory would need `symlink_dir`, and linking a directory with this
    // call produces a link that resolves to nothing.
    std::os::windows::fs::symlink_file(target, link).map_err(|e| {
        // Without Developer Mode, Windows answers ERROR_PRIVILEGE_NOT_HELD
        // (1314), and on some builds ERROR_INVALID_PARAMETER (87) because of
        // the unprivileged-create flag. Both mean the same thing to a person.
        match e.raw_os_error() {
            Some(1314) | Some(87) => VaultError::new(
                ErrorCode::SymlinkUnsupported,
                "Windows refused to create the link. Turn Developer Mode on in Settings, under System, then For developers.",
            )
            .with_detail(e.to_string())
            .with_path(link),
            _ => VaultError::from_io(&e, link, "creating the link"),
        }
    })
}

pub(super) fn remove_symlink(link: &Path) -> Result<()> {
    // A file symlink is removed with remove_file even though it points at a
    // file that stays. remove_dir would be needed for a directory link, which
    // the engine never creates.
    std::fs::remove_file(link).map_err(|e| VaultError::from_io(&e, link, "removing the link"))
}

/// Opens the file with no sharing. A sharing violation means another program
/// holds it, which is the exact condition that makes a move fail.
pub(super) fn lock_state(path: &Path) -> LockState {
    let w = wide(path);
    // GENERIC_READ only: asking for write access would report a read-only file
    // as held, which is a different problem with a different answer.
    let handle: HANDLE = unsafe {
        CreateFileW(
            w.as_ptr(),
            GENERIC_READ,
            0, // no sharing: fail if anyone else has it open
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };

    if handle != INVALID_HANDLE_VALUE {
        unsafe { CloseHandle(handle) };
        return LockState::unlocked(path, true);
    }

    let err = std::io::Error::last_os_error();
    match err.raw_os_error().map(|c| c as u32) {
        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION) => LockState::locked(
            path,
            "Another program has this file open. ComfyUI keeps model files open while a model is loaded.",
        ),
        Some(ERROR_ACCESS_DENIED) => LockState {
            path: crate::paths::display_path(path),
            locked: false,
            // Access was refused for a different reason, so the question went
            // unanswered. Saying "not locked" here would be a false guarantee.
            checkable: false,
            detail: Some("Windows refused access, so whether another program holds this file is unknown.".to_string()),
        },
        _ => LockState {
            path: crate::paths::display_path(path),
            locked: false,
            checkable: false,
            detail: Some(err.to_string()),
        },
    }
}

/// The mount point that holds the path, for example `C:\`.
///
/// Two paths on the same mount point can be renamed into each other. Two paths
/// on different mount points need a copy.
pub(super) fn volume_id(path: &Path) -> Result<VolumeId> {
    // GetVolumePathNameW needs a path that exists, so walk up to the first
    // ancestor that does. A file about to be created reports its future volume.
    let mut cursor: Option<&Path> = Some(path);
    while let Some(p) = cursor {
        if p.as_os_str().is_empty() {
            break;
        }
        if std::fs::symlink_metadata(p).is_ok() {
            let w = wide(p);
            let mut buf = vec![0u16; 260];
            let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
            if ok != 0 {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let s = String::from_utf16_lossy(&buf[..len]);
                return Ok(VolumeId(s.to_uppercase()));
            }
            return Err(VaultError::from_io(
                &std::io::Error::last_os_error(),
                p,
                "checking which drive the folder is on",
            ));
        }
        cursor = p.parent();
    }
    Err(VaultError::new(
        ErrorCode::IoError,
        "Could not tell which drive that folder is on.",
    )
    .with_path(path))
}

/// Reads the registry value the Developer Mode switch writes.
///
/// This explains the probe result. It never replaces it: a policy can block the
/// privilege while the switch reads as on, and an elevated process can create
/// links while the switch reads as off.
pub(super) fn developer_mode_enabled() -> Option<bool> {
    read_hklm_dword(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock",
        "AllowDevelopmentWithoutDevLicense",
    )
    .map(|v| v == 1)
}

/// Long path support, which decides whether a path over 260 characters works.
pub(super) fn long_paths_enabled() -> Option<bool> {
    read_hklm_dword(
        r"SYSTEM\CurrentControlSet\Control\FileSystem",
        "LongPathsEnabled",
    )
    .map(|v| v == 1)
}

fn read_hklm_dword(subkey: &str, value: &str) -> Option<u32> {
    let sub = wide_str(subkey);
    let val = wide_str(value);
    let mut data: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    // A missing key is not an error. It means the switch was never turned on.
    if status == 0 {
        Some(data)
    } else {
        None
    }
}

/// Does this process run with administrator rights?
pub(super) fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned: u32 = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    unsafe { CloseHandle(token) };
    ok != 0 && elevation.TokenIsElevated != 0
}

/// Every drive letter the computer currently has.
///
/// A folder picker has to start here, not inside `C:\`. Models often live on a
/// second drive, and that is exactly the case this product exists for.
pub(super) fn drive_roots() -> Vec<PathBuf> {
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        // The call failed. `C:` is a better answer than nothing at all.
        return vec![PathBuf::from("C:\\")];
    }
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", (b'A' + i as u8) as char)))
        .filter(|p| std::fs::metadata(p).is_ok())
        .collect()
}

/// `ERROR_NOT_SAME_DEVICE` is 17. It is the one rename failure the caller
/// recovers from, by copying instead.
pub(super) fn is_cross_volume_error(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(17)
}

/// Prefixes a path with `\\?\` so it can exceed 260 characters.
///
/// Model folders nest deeply and file names are long, so this matters in
/// practice. Rust's standard library applies the prefix for most operations
/// already; this exists for the Windows API calls above, which do not.
#[allow(dead_code)]
pub(super) fn verbatim(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    if s.starts_with(r"\\?\") || !path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    PathBuf::from(format!(r"\\?\{s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // These run only on Windows. On Linux the module is not compiled at all,
    // which is why the parent module carries the fake and the shared logic.

    #[test]
    fn a_file_this_process_holds_open_is_reported_as_held() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("held.safetensors");
        std::fs::write(&p, b"weights").unwrap();

        assert!(!lock_state(&p).locked, "nothing holds it yet");

        // Hold it the way a model loader does: open, and keep the handle.
        let held = std::fs::File::open(&p).unwrap();
        let st = lock_state(&p);
        assert!(st.locked, "an open handle was not detected");
        assert!(st.checkable);
        drop(held);

        assert!(!lock_state(&p).locked, "the file is free again");
    }

    #[test]
    fn volume_id_is_the_drive_root() {
        let d = tempfile::tempdir().unwrap();
        let v = volume_id(d.path()).unwrap();
        assert!(v.0.ends_with('\\'), "expected a drive root like C:\\, got {}", v.0);
    }

    #[test]
    fn volume_id_answers_for_a_path_that_does_not_exist_yet() {
        let d = tempfile::tempdir().unwrap();
        let future = d.path().join("not/created/yet.safetensors");
        assert_eq!(volume_id(&future).unwrap(), volume_id(d.path()).unwrap());
    }

    #[test]
    fn the_registry_reads_do_not_panic_when_the_key_is_absent() {
        // Both values are absent on a clean install. `None` is the right answer
        // and it must not be a crash.
        let _ = developer_mode_enabled();
        let _ = long_paths_enabled();
    }

    #[test]
    fn the_drive_list_has_the_system_drive_in_it() {
        let roots = drive_roots();
        assert!(!roots.is_empty(), "a computer always has at least one drive");
        for r in &roots {
            let s = r.to_string_lossy();
            assert!(s.len() == 3 && s.ends_with(":\\"), "expected a drive root, got {s}");
        }
    }

    #[test]
    fn verbatim_prefixes_an_absolute_path_once() {
        assert_eq!(verbatim(Path::new(r"C:\a\b")), PathBuf::from(r"\\?\C:\a\b"));
        assert_eq!(verbatim(Path::new(r"\\?\C:\a")), PathBuf::from(r"\\?\C:\a"));
        assert_eq!(verbatim(Path::new(r"\\srv\share\x")), PathBuf::from(r"\\?\UNC\srv\share\x"));
        assert_eq!(verbatim(Path::new("rel\\path")), PathBuf::from("rel\\path"));
    }

    #[test]
    fn not_same_device_is_recognized_and_other_errors_are_not() {
        assert!(is_cross_volume_error(&std::io::Error::from_raw_os_error(17)));
        assert!(!is_cross_volume_error(&std::io::Error::from_raw_os_error(2)));
        assert!(!is_cross_volume_error(&std::io::Error::from_raw_os_error(32)));
    }

    #[test]
    fn a_created_link_points_where_it_was_told() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("t.safetensors");
        let link = d.path().join("l.safetensors");
        std::fs::write(&target, b"w").unwrap();

        match create_file_symlink(&link, &target) {
            Ok(()) => {
                assert_eq!(std::fs::read_link(&link).unwrap(), target);
                assert_eq!(std::fs::read(&link).unwrap(), b"w");
                remove_symlink(&link).unwrap();
                assert!(target.exists(), "removing the link must not touch the target");
            }
            Err(e) => {
                // Developer Mode is off on this machine. That is a real answer,
                // not a broken test, and the error must say so clearly.
                assert_eq!(e.code, ErrorCode::SymlinkUnsupported);
                assert!(e.message.contains("Developer Mode"));
            }
        }
    }
}
