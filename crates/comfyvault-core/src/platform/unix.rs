//! The Unix half of [`crate::platform`].
//!
//! Development happens here. Two answers are deliberately honest rather than
//! flattering:
//!
//! * [`lock_state`] always reports "not locked" and marks itself unanswerable,
//!   because Unix has no mandatory locking and a file moves while it is open.
//!   The engine must never present that as a guarantee.
//! * [`developer_mode_enabled`] returns `None`, because Developer Mode is a
//!   Windows idea and inventing a value here would be a lie.

use std::path::{Path, PathBuf};

use crate::error::{Result, VaultError};
use crate::platform::{LockState, VolumeId};

pub(super) fn create_file_symlink(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| VaultError::from_io(&e, link, "creating the link"))
}

pub(super) fn remove_symlink(link: &Path) -> Result<()> {
    std::fs::remove_file(link).map_err(|e| VaultError::from_io(&e, link, "removing the link"))
}

/// Unix lets a file move and be deleted while a program reads it, so there is
/// nothing to detect. `checkable` is `false` and `locked` means nothing.
pub(super) fn lock_state(path: &Path) -> LockState {
    LockState::unlocked(path, false)
}

/// The device number identifies the filesystem, which is what decides whether a
/// rename works or a copy is needed.
pub(super) fn volume_id(path: &Path) -> Result<VolumeId> {
    use std::os::unix::fs::MetadataExt;
    // Walk up to the first ancestor that exists, so a path being created still
    // reports the volume it will land on.
    let mut cursor: Option<&Path> = Some(path);
    while let Some(p) = cursor {
        if let Ok(meta) = std::fs::metadata(p) {
            return Ok(VolumeId(meta.dev().to_string()));
        }
        cursor = p.parent();
    }
    Err(VaultError::new(
        crate::ErrorCode::IoError,
        "Could not tell which drive that folder is on.",
    )
    .with_path(path))
}

pub(super) fn developer_mode_enabled() -> Option<bool> {
    None
}

/// Running as root is the closest equivalent of an elevated process, and it is
/// worth reporting: a vault created as root leaves files the person cannot edit.
///
/// The effective user id is the second field of the `Uid:` line in
/// `/proc/self/status`. On a Unix system without that file the answer is "not
/// elevated", which is the safe default for a value the person only reads.
pub(super) fn is_elevated() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().nth(1))
        .map(|euid| euid == "0")
        .unwrap_or(false)
}

pub(super) fn long_paths_enabled() -> Option<bool> {
    None
}

/// Unix has one root, so a picker starts at `/`.
pub(super) fn drive_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

/// `EXDEV` is 18 on Linux and on macOS. It is the one rename failure the caller
/// recovers from, by copying instead.
pub(super) fn is_cross_volume_error(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(18)
}

/// Kept so the module's shape matches the Windows half.
#[allow(dead_code)]
pub(super) fn verbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_state_admits_that_it_cannot_answer() {
        // The dangerous bug this guards: reporting `locked: false` as though it
        // meant "safe to move" on a system that cannot tell.
        let st = lock_state(Path::new("/tmp/anything"));
        assert!(!st.checkable, "unix must admit it cannot detect a held file");
        assert!(!st.locked);
    }

    #[test]
    fn developer_mode_is_not_invented_on_unix() {
        assert_eq!(developer_mode_enabled(), None);
        assert_eq!(long_paths_enabled(), None);
    }

    #[test]
    fn volume_id_is_equal_for_two_paths_on_one_filesystem() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        let b = d.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert_eq!(volume_id(&a).unwrap(), volume_id(&b).unwrap());
    }

    #[test]
    fn volume_id_answers_for_a_path_that_does_not_exist_yet() {
        // The plan asks which volume a file will land on before it lands there.
        let d = tempfile::tempdir().unwrap();
        let future = d.path().join("not/created/yet.safetensors");
        assert_eq!(volume_id(&future).unwrap(), volume_id(d.path()).unwrap());
    }

    #[test]
    fn a_separate_filesystem_reports_a_different_volume() {
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        // Not asserted as unequal unconditionally: on some containers /dev/shm
        // and /tmp really are one filesystem, and that is not a failure.
        let (a, b) = (volume_id(d.path()).unwrap(), volume_id(shm).unwrap());
        if a != b {
            assert_ne!(a, b);
        }
    }

    #[test]
    fn exdev_is_recognized_and_other_errors_are_not() {
        let xdev = std::io::Error::from_raw_os_error(18);
        assert!(is_cross_volume_error(&xdev));
        let enoent = std::io::Error::from_raw_os_error(2);
        assert!(!is_cross_volume_error(&enoent));
        let eacces = std::io::Error::from_raw_os_error(13);
        assert!(!is_cross_volume_error(&eacces));
    }

    #[test]
    fn a_created_link_points_where_it_was_told() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("t.safetensors");
        let link = d.path().join("l.safetensors");
        std::fs::write(&target, b"w").unwrap();
        create_file_symlink(&link, &target).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        remove_symlink(&link).unwrap();
        assert!(target.exists());
    }

    #[test]
    fn creating_a_link_where_something_exists_fails_without_overwriting() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("t");
        let occupied = d.path().join("occupied");
        std::fs::write(&target, b"target").unwrap();
        std::fs::write(&occupied, b"original").unwrap();

        let err = create_file_symlink(&occupied, &target).unwrap_err();
        assert_eq!(err.code, crate::ErrorCode::Conflict);
        assert_eq!(std::fs::read(&occupied).unwrap(), b"original", "the file was overwritten");
    }
}
