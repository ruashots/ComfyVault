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
///
/// `temp_dir` must be on the same drive as `to`, because the checked copy is
/// renamed from there into place. `copied` hears every byte a copy advances.
#[allow(clippy::too_many_arguments)]
pub fn move_file(
    platform: &dyn Platform,
    from: &Path,
    to: &Path,
    expected_sha256: &str,
    temp_dir: &Path,
    cancel: &CancelToken,
    copied: &mut dyn FnMut(u64),
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
            copy_verify_then_remove(platform, from, to, expected_sha256, temp_dir, cancel, copied)?;
            Ok(MoveKind::Copied)
        }
        Err(RenameError::Io(e)) => Err(VaultError::from_io(&e, from, "moving the file into the vault")),
    }
}

/// Copies to a temporary file, checks it, puts it in place, then removes the
/// source.
///
/// The temporary file lives on the destination's drive, so the final step is a
/// rename on one drive and cannot half succeed.
pub fn copy_verify_then_remove(
    platform: &dyn Platform,
    from: &Path,
    to: &Path,
    expected_sha256: &str,
    temp_dir: &Path,
    cancel: &CancelToken,
    copied: &mut dyn FnMut(u64),
) -> Result<()> {
    ensure_dir(temp_dir)?;
    let temp = temp_dir.join(format!(
        "{}-{}.part",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));

    let result = (|| -> Result<()> {
        copy_with_cancel(from, &temp, cancel, copied)?;

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
        // Through the platform, so a temporary file on the wrong drive fails
        // here in a test the way it fails on Windows.
        platform
            .rename(&temp, to)
            .map_err(|e| e.into_vault_error(to, "putting the copy in place"))?;
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

/// The granularity at which a sparse copy leaves zeros unwritten. It is the
/// unit NTFS allocates a sparse file in on a drive with 4 KB clusters.
const SPARSE_BLOCK: usize = 64 * 1024;

/// Copies bytes and flushes them to the disk.
///
/// The copy keeps what the file system knew about the file, not only its
/// bytes: a sparse file stays sparse, a compressed one stays compressed, and
/// the modification time stays. A copy that lost them took the full size on
/// the drive for a file that had occupied almost nothing, and gave the scan a
/// new time, so the next scan read the whole file again.
fn copy_with_cancel(
    from: &Path,
    to: &Path,
    cancel: &CancelToken,
    copied: &mut dyn FnMut(u64),
) -> Result<u64> {
    use std::io::{Seek, SeekFrom};

    let mut src = std::fs::File::open(from).ctx(from, "opening the file to copy it")?;
    let modified = src.metadata().and_then(|m| m.modified()).ctx(from, "reading the file's time")?;
    // Read access too: Windows refuses to compress a file through a handle that
    // can only write.
    let mut dst = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(to)
        .ctx(to, "creating the copy")?;
    let sparse = crate::platform::copy_storage_traits(&src, &dst)
        .ctx(to, "marking the copy sparse or compressed like the original")?;

    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;

    loop {
        cancel.check()?;
        let n = src.read(&mut buf).ctx(from, "reading the file to copy it")?;
        if n == 0 {
            break;
        }
        if sparse {
            // A range of zeros in a sparse file is skipped, not written, so it
            // stays a hole. A file that was not sparse is written in full: a
            // hole there would be a change the person did not ask for.
            for block in buf[..n].chunks(SPARSE_BLOCK) {
                if block.iter().all(|&b| b == 0) {
                    dst.seek(SeekFrom::Current(block.len() as i64)).ctx(to, "writing the copy")?;
                } else {
                    dst.write_all(block).ctx(to, "writing the copy")?;
                }
            }
        } else {
            dst.write_all(&buf[..n]).ctx(to, "writing the copy")?;
        }
        total += n as u64;
        copied(n as u64);
    }
    // A file ending in a hole is shorter than it should be until its length is
    // set.
    dst.set_len(total).ctx(to, "writing the copy")?;
    dst.set_modified(modified).ctx(to, "setting the copy's time")?;
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
///
/// A link standing in its place is replaced by the rename itself, so the path
/// is never empty, not even for an instant.
pub fn unstash(stash: &Path, original: &Path) -> Result<()> {
    let is_link = std::fs::symlink_metadata(original)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if std::fs::symlink_metadata(original).is_ok() && !is_link {
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
///
/// `mtime_nanos` is the time the original carried, when the journal recorded
/// it. Without it the copy keeps the vault file's time, which is the time of
/// the copy that was kept.
pub fn restore_from_vault(
    vault_file: &Path,
    original: &Path,
    expected_sha256: &str,
    mtime_nanos: Option<i128>,
    cancel: &CancelToken,
    copied: &mut dyn FnMut(u64),
) -> Result<()> {
    replaceable(original, vault_file)?;
    if let Some(parent) = original.parent() {
        ensure_dir(parent)?;
    }
    let temp = original.with_extension(format!(
        "comfyvault-restore-{}",
        uuid::Uuid::new_v4().simple()
    ));

    let result = (|| -> Result<()> {
        copy_with_cancel(vault_file, &temp, cancel, copied)?;
        if let Some(time) = mtime_nanos.and_then(time_from_nanos) {
            std::fs::File::options()
                .write(true)
                .open(&temp)
                .and_then(|f| f.set_modified(time))
                .ctx(&temp, "setting the file's time")?;
        }
        let actual = hash::hash_file_cancellable(&temp, cancel)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err(VaultError::new(
                ErrorCode::IoError,
                "The file put back did not match what was taken, so it was not kept.",
            )
            .with_detail(format!("expected {expected_sha256}, got {actual}"))
            .with_path(original));
        }
        // Replaces the link in one step. Removing the link first left the
        // path empty for as long as the copy took, and for good when the copy
        // was stopped or failed: ComfyUI then had neither the file nor a link.
        std::fs::rename(&temp, original).ctx(original, "putting the file back")?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Moves a vault file back to the place it came from, replacing the link that
/// stands there in one step.
///
/// On one drive it is a rename. Across drives it is a checked copy staged
/// beside `place`, renamed over the link, and only then is the vault file
/// removed. Either way the path holds the link or the file at every moment.
pub fn move_back(
    platform: &dyn Platform,
    vault_file: &Path,
    place: &Path,
    expected_sha256: &str,
    cancel: &CancelToken,
    copied: &mut dyn FnMut(u64),
) -> Result<MoveKind> {
    replaceable(place, vault_file)?;
    let parent = place.parent().unwrap_or(place);
    ensure_dir(parent)?;
    match platform.rename(vault_file, place) {
        Ok(()) => Ok(MoveKind::Renamed),
        Err(RenameError::CrossVolume) => {
            copy_verify_then_remove(platform, vault_file, place, expected_sha256, parent, cancel, copied)?;
            Ok(MoveKind::Copied)
        }
        Err(RenameError::Io(e)) => Err(VaultError::from_io(&e, vault_file, "putting the file back")),
    }
}

/// A place a file may be put back into: empty, or holding a link to this
/// vault file. Anything else there belongs to somebody, and is refused.
fn replaceable(place: &Path, vault_file: &Path) -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(place) else { return Ok(()) };
    let ours = meta.file_type().is_symlink()
        && matches!(
            (std::fs::canonicalize(place), std::fs::canonicalize(vault_file)),
            (Ok(a), Ok(b)) if a == b
        );
    if ours {
        return Ok(());
    }
    Err(VaultError::new(
        ErrorCode::Conflict,
        "Could not put the file back, because something is in its place.",
    )
    .with_path(place))
}

/// The inverse of [`crate::time_util::Timestamp::mtime_nanos`].
fn time_from_nanos(nanos: i128) -> Option<std::time::SystemTime> {
    let magnitude = std::time::Duration::from_nanos(u64::try_from(nanos.unsigned_abs()).ok()?);
    if nanos >= 0 {
        std::time::UNIX_EPOCH.checked_add(magnitude)
    } else {
        std::time::UNIX_EPOCH.checked_sub(magnitude)
    }
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

    /// The folder is drive C:, and its `vault` and `tmp` folders are drive D:.
    fn two_drives(p: &FakePlatform, root: &Path) {
        p.set_volume(root, "C:\\");
        p.set_volume(root.join("vault"), "D:\\");
        p.set_volume(root.join("tmp"), "D:\\");
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
            &mut |_| {},
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
        two_drives(&p, d.path());

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
            &mut |_| {},
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
        two_drives(&p, d.path());

        let from = d.path().join("a.safetensors");
        let to = d.path().join("vault/loras/a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();

        let wrong_hash = crate::scan::hash::hash_bytes(b"something else entirely");
        let err = move_file(&p, &from, &to, &wrong_hash, &d.path().join("tmp"), &CancelToken::new(), &mut |_| {})
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
        two_drives(&p, d.path());
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
            &mut |_| {},
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
            &mut |_| {},
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
            None,
            &CancelToken::new(),
            &mut |_| {},
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

        let err = restore_from_vault(&vault_file, &original, "x", None, &CancelToken::new(), &mut |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(&original).unwrap(), b"already here");
    }

    #[test]
    fn restoring_a_file_whose_content_is_wrong_keeps_nothing() {
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        let original = d.path().join("o.safetensors");
        std::fs::write(&vault_file, weights("a")).unwrap();

        let err = restore_from_vault(&vault_file, &original, &"0".repeat(64), None, &CancelToken::new(), &mut |_| {})
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::IoError);
        assert!(!original.exists(), "a file that failed its check must not be left behind");
    }

    /// Sixteen megabytes, of which only the header is ever written.
    const SPARSE_LEN: u64 = 16 * 1024 * 1024;

    #[test]
    fn a_sparse_file_put_back_stays_sparse() {
        // Measured on Windows before this: a model occupying 64 KB came back
        // occupying its full size, and an undo filled the drive with zeros.
        let d = tmp();
        let vault_file = d.path().join("vault/loras/a.safetensors");
        crate::testkit::write_sparse(&vault_file, "a", SPARSE_LEN);
        let vault_on_disk = crate::platform::size_on_disk(&vault_file).unwrap();
        assert!(vault_on_disk < SPARSE_LEN / 4, "the test file is not sparse: {vault_on_disk}");

        let original = d.path().join("install/models/loras/a.safetensors");
        let sha = crate::scan::hash::hash_file(&vault_file).unwrap();
        restore_from_vault(&vault_file, &original, &sha, None, &CancelToken::new(), &mut |_| {})
            .unwrap();

        assert_eq!(std::fs::metadata(&original).unwrap().len(), SPARSE_LEN, "the length must survive");
        let on_disk = crate::platform::size_on_disk(&original).unwrap();
        assert!(
            on_disk <= vault_on_disk + SPARSE_BLOCK as u64,
            "the copy takes {on_disk} bytes on the drive, the vault file takes {vault_on_disk}"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            let attributes = std::fs::metadata(&original).unwrap().file_attributes();
            assert!(attributes & 0x200 != 0, "Windows no longer marks the copy sparse");
        }
    }

    #[test]
    fn a_file_that_was_not_sparse_is_not_made_sparse() {
        // Zeros in an ordinary file are written. A hole would change how the
        // file sits on the drive, which nobody asked for.
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        std::fs::write(&vault_file, vec![0u8; 4 * 1024 * 1024]).unwrap();
        let original = d.path().join("o.safetensors");
        let sha = crate::scan::hash::hash_file(&vault_file).unwrap();
        restore_from_vault(&vault_file, &original, &sha, None, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(
            crate::platform::size_on_disk(&original).unwrap(),
            crate::platform::size_on_disk(&vault_file).unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_compressed_file_put_back_stays_compressed() {
        use std::os::windows::fs::MetadataExt;
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        std::fs::File::create(&vault_file).unwrap();
        let out = std::process::Command::new("compact")
            .args(["/c", "/q"])
            .arg(&vault_file)
            .output()
            .unwrap();
        assert!(out.status.success(), "compact could not compress the file: {out:?}");
        std::fs::write(&vault_file, vec![7u8; 4 * 1024 * 1024]).unwrap();
        assert!(std::fs::metadata(&vault_file).unwrap().file_attributes() & 0x800 != 0);

        let original = d.path().join("o.safetensors");
        let sha = crate::scan::hash::hash_file(&vault_file).unwrap();
        restore_from_vault(&vault_file, &original, &sha, None, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert!(
            std::fs::metadata(&original).unwrap().file_attributes() & 0x800 != 0,
            "the copy lost NTFS compression"
        );
    }

    #[test]
    fn a_file_put_back_carries_its_own_time_or_else_the_vault_files() {
        // A new time makes the next scan read the whole file again, because
        // size and time are what the scan cache trusts.
        let d = tmp();
        let vault_file = d.path().join("v.safetensors");
        std::fs::write(&vault_file, weights("a")).unwrap();
        let vault_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(&vault_file).unwrap().set_modified(vault_time).unwrap();
        let sha = crate::scan::hash::hash_bytes(&weights("a"));

        let recorded = 1_600_000_000_123_456_700i128;
        let own = d.path().join("own.safetensors");
        restore_from_vault(&vault_file, &own, &sha, Some(recorded), &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(meta_of(&own).1, recorded, "the time the journal recorded was not put back");

        let unrecorded = d.path().join("unrecorded.safetensors");
        restore_from_vault(&vault_file, &unrecorded, &sha, None, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(meta_of(&unrecorded).1, meta_of(&vault_file).1);
    }

    #[test]
    fn a_cross_drive_move_keeps_the_time_and_reports_every_byte() {
        let d = tmp();
        let p = FakePlatform::new();
        two_drives(&p, d.path());
        let from = d.path().join("a.safetensors");
        std::fs::write(&from, weights("a")).unwrap();
        // Far from now, because a copy made in the same clock tick as the
        // source would share its time without anything having kept it.
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_500_000_000);
        std::fs::File::options().write(true).open(&from).unwrap().set_modified(old).unwrap();
        let before = meta_of(&from).1;

        let mut heard = 0u64;
        move_file(
            &p,
            &from,
            &d.path().join("vault/a.safetensors"),
            &crate::scan::hash::hash_bytes(&weights("a")),
            &d.path().join("tmp"),
            &CancelToken::new(),
            &mut |n| heard += n,
        )
        .unwrap();

        assert_eq!(meta_of(&d.path().join("vault/a.safetensors")).1, before);
        assert_eq!(heard, weights("a").len() as u64);
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
        two_drives(&p, d.path());
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
            &mut |_| {},
        )
        .unwrap_err();

        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(std::fs::read(&from).unwrap(), weights("a"));
    }
}
