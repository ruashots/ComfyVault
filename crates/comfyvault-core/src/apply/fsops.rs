//! The filesystem steps, written so that no sequence of them can lose bytes.
//!
//! # The rule
//!
//! Nothing is ever deleted before its replacement exists and has been checked.
//! Where that cannot be done in one step, the original is renamed aside first
//! and removed last, so an interruption leaves the original recoverable under
//! a different name rather than gone.
//!
//! # Moving across drives
//!
//! A rename only works inside one drive. Across drives the bytes are copied to
//! a temporary file inside the vault, flushed to the disk, hashed, compared
//! against the hash the scan recorded, and only then put in place. The source
//! is removed after that, never before.
//!
//! The fallback is driven by what the operating system actually answered, not
//! by a prediction about which drive a path is on. A wrong prediction can then
//! only cost time, never data.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{ErrorCode, IoResultExt, Result, VaultError};
use crate::platform::{Platform, RenameError};
use crate::progress::CancelToken;
use crate::scan::hash;

/// The suffix a duplicate is renamed to before its link takes its place.
pub const STASH_SUFFIX: &str = ".comfyvault-old";

/// How the bytes got to the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    /// A rename inside one drive. Instant, and it needs no extra space.
    Renamed,
    /// Copied to the other drive, checked, then the source was removed.
    Copied,
}

/// Creates a folder and every folder above it.
pub fn ensure_dir(path: &Path) -> Result<bool> {
    if path.is_dir() {
        return Ok(false);
    }
    std::fs::create_dir_all(path).ctx(path, "creating the folder")?;
    Ok(true)
}

/// Moves a file, copying it when the two paths are on different drives.
///
/// `expected_sha256` is compared against the copy before the source is removed.
/// A copy that does not match is deleted and the source is left exactly where
/// it was.
pub fn move_file(
    platform: &dyn Platform,
    from: &Path,
    to: &Path,
    expected_sha256: &str,
    temp_dir: &Path,
    cancel: &CancelToken,
) -> Result<MoveKind> {
    if to.exists() || platform.is_symlink(to) {
        return Err(VaultError::new(
            ErrorCode::Conflict,
            "Something already exists at the vault path for this file, so nothing was moved.",
        )
        .with_path(to));
    }
    if let Some(parent) = to.parent() {
        ensure_dir(parent)?;
    }

    match platform.rename(from, to) {
        Ok(()) => Ok(MoveKind::Renamed),
        Err(RenameError::CrossVolume) => {
            copy_verify_then_remove(from, to, expected_sha256, temp_dir, cancel)?;
            Ok(MoveKind::Copied)
        }
        Err(RenameError::Io(e)) => Err(VaultError::from_io(&e, from, "moving the file into the vault")),
    }
}

/// Copies to a temporary file, checks it, puts it in place, then removes the
/// source.
///
/// The temporary file lives inside the vault, so the final step is a rename on
/// one drive and cannot half succeed.
pub fn copy_verify_then_remove(
    from: &Path,
    to: &Path,
    expected_sha256: &str,
    temp_dir: &Path,
    cancel: &CancelToken,
) -> Result<()> {
    ensure_dir(temp_dir)?;
    let temp = temp_dir.join(format!(
        "{}-{}.part",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));

    let result = (|| -> Result<()> {
        copy_with_cancel(from, &temp, cancel)?;

        // Read the copy back off the disk. Comparing what was written, rather
        // than what was meant to be written, is the whole point of the check.
        let actual = hash::hash_file_cancellable(&temp, cancel)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err(VaultError::new(
                ErrorCode::IoError,
                "The copy in the vault did not match the original, so nothing was removed.",
            )
            .with_detail(format!("expected {expected_sha256}, the copy is {actual}"))
            .with_path(from));
        }
        std::fs::rename(&temp, to).ctx(to, "putting the copy in place")?;
        Ok(())
    })();

    if result.is_err() {
        // The source has not been touched, so removing the failed copy loses
        // nothing.
        let _ = std::fs::remove_file(&temp);
        return result;
    }

    // Only now, with a verified copy in place, does the original go.
    std::fs::remove_file(from).ctx(from, "removing the original after the copy")?;
    Ok(())
}

/// Copies bytes and flushes them to the disk.
fn copy_with_cancel(from: &Path, to: &Path, cancel: &CancelToken) -> Result<u64> {
    let mut src = std::fs::File::open(from).ctx(from, "opening the file to copy it")?;
    let mut dst = std::fs::File::create(to).ctx(to, "creating the copy")?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;

    loop {
        cancel.check()?;
        let n = src.read(&mut buf).ctx(from, "reading the file to copy it")?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).ctx(to, "writing the copy")?;
        total += n as u64;
    }
    // Without this the bytes can still be in the operating system's cache when
    // the original is removed, and a power cut then loses the file.
    dst.sync_all().ctx(to, "flushing the copy to the disk")?;
    Ok(total)
}

/// Renames a file aside so its link can take its place.
///
/// Returns the path it was renamed to. The name is unique, so an existing file
/// is never overwritten.
pub fn stash(path: &Path, tag: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| VaultError::invalid("That path has no file name."))?
        .to_string_lossy()
        .to_string();
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::invalid("That path has no parent folder."))?;

    let mut candidate = parent.join(format!("{name}{STASH_SUFFIX}-{tag}"));
    let mut n = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{name}{STASH_SUFFIX}-{tag}-{n}"));
        n += 1;
        if n > 1000 {
            return Err(VaultError::new(
                ErrorCode::Conflict,
                "Could not set the old file aside, because too many files with that name already exist.",
            )
            .with_path(path));
        }
    }
    std::fs::rename(path, &candidate).ctx(path, "setting the old file aside")?;
    Ok(candidate)
}

/// Puts a stashed file back where it came from.
pub fn unstash(stash: &Path, original: &Path) -> Result<()> {
    if original.exists() || std::fs::symlink_metadata(original).is_ok() {
        return Err(VaultError::new(
            ErrorCode::Conflict,
            "Could not put the old file back, because something is in its place.",
        )
        .with_path(original));
    }
    std::fs::rename(stash, original).ctx(stash, "putting the old file back")
}

/// Copies a file out of the vault, to put an original back during a revert.
///
/// This is safe because the vault file and the original are the same content by
/// hash, which is exactly why the original was allowed to be removed.
pub fn restore_from_vault(
    vault_file: &Path,
    original: &Path,
    expected_sha256: &str,
    cancel: &CancelToken,
) -> Result<()> {
    if std::fs::symlink_metadata(original).is_ok() {
        return Err(VaultError::new(
            ErrorCode::Conflict,
            "Could not put the file back, because something is in its place.",
        )
        .with_path(original));
    }
    if let Some(parent) = original.parent() {
        ensure_dir(parent)?;
    }
    let temp = original.with_extension(format!(
        "comfyvault-restore-{}",
        uuid::Uuid::new_v4().simple()
    ));

    let result = (|| -> Result<()> {
        copy_with_cancel(vault_file, &temp, cancel)?;
        let actual = hash::hash_file_cancellable(&temp, cancel)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err(VaultError::new(
                ErrorCode::IoError,
                "The file put back did not match what was taken, so it was not kept.",
            )
            .with_detail(format!("expected {expected_sha256}, got {actual}"))
            .with_path(original));
        }
        std::fs::rename(&temp, original).ctx(original, "putting the file back")?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Removes a folder only when it is empty.
///
/// Used to undo a folder the engine made. A folder the person put something in
/// is left alone.
pub fn remove_dir_if_empty(path: &Path) -> Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Ok(false);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(VaultError::from_io(&e, path, "reading the folder")),
    }
    std::fs::remove_dir(path).ctx(path, "removing the empty folder")?;
    Ok(true)
}

/// Does this file still match what the scan recorded?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyMode {
    /// Compare the size and the modification time. This is what the scan cache
    /// compares, so it is the same guarantee the plan was built on.
    SizeAndMtime,
    /// Read the file again and compare the hash. Slower, and the strongest
    /// check available.
    Rehash,
}

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    Ok,
    Missing,
    Changed,
    PermissionDenied,
    Unreadable,
}

impl VerifyResult {
    pub fn to_block_reason(self) -> Option<crate::plan::BlockReason> {
        match self {
            Self::Ok => None,
            Self::Missing => Some(crate::plan::BlockReason::FileMissing),
            Self::Changed => Some(crate::plan::BlockReason::FileChanged),
            Self::PermissionDenied => Some(crate::plan::BlockReason::PermissionDenied),
            Self::Unreadable => Some(crate::plan::BlockReason::ReadError),
        }
    }
}

/// Checks a file against what the plan recorded about it.
pub fn verify(
    path: &Path,
    size_bytes: u64,
    mtime_nanos: i128,
    sha256: &str,
    mode: VerifyMode,
    cancel: &CancelToken,
) -> VerifyResult {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return VerifyResult::Missing,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return VerifyResult::PermissionDenied
        }
        Err(_) => return VerifyResult::Unreadable,
    };
    if meta.file_type().is_symlink() {
        // The plan was built against a real file. A link in its place means
        // something changed it.
        return VerifyResult::Changed;
    }
    if meta.len() != size_bytes {
        return VerifyResult::Changed;
    }
    if crate::time_util::Timestamp::mtime_nanos(&meta) != mtime_nanos {
        return VerifyResult::Changed;
    }
    if mode == VerifyMode::Rehash {
        return match hash::hash_file_cancellable(path, cancel) {
            Ok(h) if h.eq_ignore_ascii_case(sha256) => VerifyResult::Ok,
            Ok(_) => VerifyResult::Changed,
            Err(e) if e.code == ErrorCode::PermissionDenied => VerifyResult::PermissionDenied,
            Err(_) => VerifyResult::Unreadable,
        };
    }
    VerifyResult::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{FakePlatform, NativePlatform};
    use crate::testkit::weights;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn meta_of(p: &Path) -> (u64, i128) {
        let m = std::fs::metadata(p).unwrap();
        (m.len(), crate::time_util::Timestamp::mtime_nanos(&m))
    }

    #[test]
    fn a_same_drive_move_is_a_rename() {
        let d = tmp();
        let from = d.path().join("a.safetensors");
        let to = d.path().join("vault/loras/a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();

        let kind = move_file(
            &NativePlatform::new(),
            &from,
            &to,
            &crate::scan::hash::hash_bytes(&weights("a")),
            &d.path().join("tmp"),
            &CancelToken::new(),
        )
        .unwrap();

        assert_eq!(kind, MoveKind::Renamed);
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), weights("a"));
    }

    #[test]
    fn a_cross_drive_move_copies_checks_and_only_then_removes_the_source() {
        let d = tmp();
        let p = FakePlatform::new();
        p.force_cross_volume(true);

        let from = d.path().join("a.safetensors");
        let to = d.path().join("vault/loras/a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();

        let kind = move_file(
            &p,
            &from,
            &to,
            &crate::scan::hash::hash_bytes(&weights("a")),
            &d.path().join("tmp"),
            &CancelToken::new(),
        )
        .unwrap();

        assert_eq!(kind, MoveKind::Copied);
        assert!(!from.exists(), "the source goes only after the copy is checked");
        assert_eq!(std::fs::read(&to).unwrap(), weights("a"));
    }

    #[test]
    fn a_copy_that_does_not_match_leaves_the_source_untouched() {
        // The check exists for exactly this: a drive that wrote something other
        // than what it was given must never cost the person the original.
        let d = tmp();
        let p = FakePlatform::new();
        p.force_cross_volume(true);

        let from = d.path().join("a.safetensors");
        let to = d.path().join("vault/loras/a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();

        let wrong_hash = crate::scan::hash::hash_bytes(b"something else entirely");
        let err = move_file(&p, &from, &to, &wrong_hash, &d.path().join("tmp"), &CancelToken::new())
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::IoError);
        assert!(err.message.contains("nothing was removed"));
        assert_eq!(std::fs::read(&from).unwrap(), weights("a"), "the source was lost");
        assert!(!to.exists(), "a copy that failed its check must not be left in the vault");
    }

    #[test]
    fn a_failed_copy_leaves_no_partial_file_behind() {
        let d = tmp();
        let p = FakePlatform::new();
        p.force_cross_volume(true);
        let temp_dir = d.path().join("tmp");

        let from = d.path().join("a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();
        let _ = move_file(
            &p,
            &from,
            &d.path().join("vault/a.safetensors"),
            &crate::scan::hash::hash_bytes(b"wrong"),
            &temp_dir,
            &CancelToken::new(),
        );

        let leftovers: Vec<_> = std::fs::read_dir(&temp_dir).unwrap().filter_map(|e| e.ok()).collect();
        assert!(leftovers.is_empty(), "a part file was left in the vault: {leftovers:?}");
    }

    #[test]
    fn a_move_onto_an_occupied_path_is_refused_without_overwriting() {
        let d = tmp();
        let from = d.path().join("a.safetensors");
        let to = d.path().join("taken.safetensors");
        std::fs::write(&from, weights("a")).unwrap();
        std::fs::write(&to, b"do not lose me").unwrap();

        let err = move_file(
            &NativePlatform::new(),
            &from,
            &to,
            "irrelevant",
            &d.path().join("tmp"),
            &CancelToken::new(),
        )
        .unwrap_err();

        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&to).unwrap(), b"do not lose me");
        assert!(from.exists());
    }

    #[test]
    fn stashing_renames_the_file_and_leaves_its_place_empty() {
        let d = tmp();
        let p = d.path().join("m.safetensors");
        std::fs::write(&p, weights("m")).unwrap();

        let stashed = stash(&p, "ap1").unwrap();
        assert!(!p.exists(), "the place must be free for the link");
        assert_eq!(std::fs::read(&stashed).unwrap(), weights("m"));
        assert!(stashed.to_string_lossy().contains(STASH_SUFFIX));
    }

    #[test]
    fn stashing_twice_never_overwrites_the_first_stash() {
        let d = tmp();
        let p = d.path().join("m.safetensors");

        std::fs::write(&p, b"first").unwrap();
        let one = stash(&p, "ap1").unwrap();
        std::fs::write(&p, b"second").unwrap();
        let two = stash(&p, "ap1").unwrap();

        assert_ne!(one, two);
        assert_eq!(std::fs::read(&one).unwrap(), b"first");
        assert_eq!(std::fs::read(&two).unwrap(), b"second");
    }

    #[test]
    fn unstashing_puts_the_file_back_and_refuses_when_the_place_is_taken() {
        let d = tmp();
        let p = d.path().join("m.safetensors");
        std::fs::write(&p, weights("m")).unwrap();
        let stashed = stash(&p, "ap1").unwrap();

        unstash(&stashed, &p).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), weights("m"));

        // Now something else is in the way.
        std::fs::write(&p, b"in the way").unwrap();
        std::fs::write(&stashed, b"old").unwrap();
        let err = unstash(&stashed, &p).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&p).unwrap(), b"in the way", "the file in place was overwritten");
    }

    #[test]
    fn restoring_from_the_vault_reproduces_the_original_bytes() {
        // A revert puts back a duplicate whose bytes were removed. It is only
        // safe because the vault holds the same content by hash.
        let d = tmp();
        let vault_file = d.path().join("vault/loras/a.safetensors");
        std::fs::create_dir_all(vault_file.parent().unwrap()).unwrap();
        std::fs::write(&vault_file, weights("a")).unwrap();

        let original = d.path().join("install/models/loras/a.safetensors");
        restore_from_vault(
            &vault_file,
            &original,
            &crate::scan::hash::hash_bytes(&weights("a")),
            &CancelToken::new(),
        )
        .unwrap();

        assert_eq!(std::fs::read(&original).unwrap(), weights("a"));
        assert!(vault_file.exists(), "the vault file must survive the restore");
    }

    #[test]
    fn restoring_refuses_when_something_already_occupies_the_place() {
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        let original = d.path().join("o.safetensors");
        std::fs::write(&vault_file, weights("a")).unwrap();
        std::fs::write(&original, b"already here").unwrap();

        let err = restore_from_vault(&vault_file, &original, "x", &CancelToken::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&original).unwrap(), b"already here");
    }

    #[test]
    fn restoring_a_file_whose_content_is_wrong_keeps_nothing() {
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        let original = d.path().join("o.safetensors");
        std::fs::write(&vault_file, weights("a")).unwrap();

        let err = restore_from_vault(&vault_file, &original, &"0".repeat(64), &CancelToken::new())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::IoError);
        assert!(!original.exists(), "a file that failed its check must not be left behind");
    }

    #[test]
    fn verify_accepts_an_unchanged_file() {
        let d = tmp();
        let p = d.path().join("m.safetensors");
        std::fs::write(&p, weights("m")).unwrap();
        let (size, mtime) = meta_of(&p);
        let sha = crate::scan::hash::hash_bytes(&weights("m"));

        assert_eq!(
            verify(&p, size, mtime, &sha, VerifyMode::SizeAndMtime, &CancelToken::new()),
            VerifyResult::Ok
        );
        assert_eq!(
            verify(&p, size, mtime, &sha, VerifyMode::Rehash, &CancelToken::new()),
            VerifyResult::Ok
        );
    }

    #[test]
    fn verify_notices_a_changed_size_a_changed_time_and_a_missing_file() {
        let d = tmp();
        let p = d.path().join("m.safetensors");
        std::fs::write(&p, weights("m")).unwrap();
        let (size, mtime) = meta_of(&p);
        let sha = crate::scan::hash::hash_bytes(&weights("m"));
        let c = CancelToken::new();

        assert_eq!(verify(&p, size + 1, mtime, &sha, VerifyMode::SizeAndMtime, &c), VerifyResult::Changed);
        assert_eq!(verify(&p, size, mtime + 1, &sha, VerifyMode::SizeAndMtime, &c), VerifyResult::Changed);

        std::fs::remove_file(&p).unwrap();
        assert_eq!(verify(&p, size, mtime, &sha, VerifyMode::SizeAndMtime, &c), VerifyResult::Missing);
    }

    #[test]
    fn rehashing_catches_an_edit_that_kept_the_size_and_the_time() {
        // The one case size and time cannot see. Rehash exists for it, and a
        // person who waited a long time between the scan and the apply can ask
        // for it.
        let d = tmp();
        let p = d.path().join("m.safetensors");
        let original = weights("original");
        std::fs::write(&p, &original).unwrap();
        let (size, mtime) = meta_of(&p);
        let sha = crate::scan::hash::hash_bytes(&original);

        // Same length, different bytes, and the time is put back.
        let mut tampered = original.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        std::fs::write(&p, &tampered).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(
            std::time::UNIX_EPOCH + std::time::Duration::from_nanos(mtime as u64),
        )
        .unwrap();
        drop(f);

        let c = CancelToken::new();
        assert_eq!(
            verify(&p, size, mtime, &sha, VerifyMode::SizeAndMtime, &c),
            VerifyResult::Ok,
            "size and time genuinely cannot see this"
        );
        assert_eq!(
            verify(&p, size, mtime, &sha, VerifyMode::Rehash, &c),
            VerifyResult::Changed,
            "rehash is the check that catches it"
        );
    }

    #[test]
    fn verify_treats_a_link_where_a_real_file_was_as_a_change() {
        let d = tmp();
        let target = d.path().join("t.safetensors");
        let p = d.path().join("m.safetensors");
        std::fs::write(&target, weights("m")).unwrap();
        std::fs::write(&p, weights("m")).unwrap();
        let (size, mtime) = meta_of(&p);

        std::fs::remove_file(&p).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &p).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &p).unwrap();

        assert_eq!(
            verify(&p, size, mtime, "x", VerifyMode::SizeAndMtime, &CancelToken::new()),
            VerifyResult::Changed
        );
    }

    #[test]
    fn ensure_dir_reports_whether_it_made_the_folder() {
        let d = tmp();
        let p = d.path().join("a/b/c");
        assert!(ensure_dir(&p).unwrap(), "it made the folder");
        assert!(!ensure_dir(&p).unwrap(), "it was already there");
        assert!(p.is_dir());
    }

    #[test]
    fn an_empty_folder_is_removed_and_one_with_something_in_it_is_kept() {
        let d = tmp();
        let empty = d.path().join("empty");
        let used = d.path().join("used");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&used).unwrap();
        std::fs::write(used.join("keep.safetensors"), b"x").unwrap();

        assert!(remove_dir_if_empty(&empty).unwrap());
        assert!(!empty.exists());
        assert!(!remove_dir_if_empty(&used).unwrap(), "a folder with a file in it must stay");
        assert!(used.join("keep.safetensors").exists());
        assert!(!remove_dir_if_empty(&d.path().join("never-existed")).unwrap());
    }

    #[test]
    fn a_cancelled_copy_leaves_the_source_alone() {
        let d = tmp();
        let p = FakePlatform::new();
        p.force_cross_volume(true);
        let from = d.path().join("a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();

        let cancel = CancelToken::new();
        cancel.cancel();
        let err = move_file(
            &p,
            &from,
            &d.path().join("vault/a.safetensors"),
            &crate::scan::hash::hash_bytes(&weights("a")),
            &d.path().join("tmp"),
            &cancel,
        )
        .unwrap_err();

        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(std::fs::read(&from).unwrap(), weights("a"));
    }
}
