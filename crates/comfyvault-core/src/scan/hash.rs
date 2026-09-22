//! Hashing model files.
//!
//! # Why SHA-256 and why only one hash
//!
//! The hash does two jobs. It decides which files are the same content, and it
//! is the identity Civitai looks a model up by. Civitai accepts several hash
//! types on its single lookup, but its batch lookup accepts SHA-256 and nothing
//! else. Identifying three hundred files with the batch form is three requests
//! instead of three hundred, so SHA-256 is the one hash that serves both jobs.
//!
//! A faster hash would not make the scan faster. Reading 1.8 TB from a SATA
//! solid state drive takes about an hour, and SHA-256 runs at gigabytes per
//! second per core. The disk is the limit, not the hash, so there is no reason
//! to compute a second one.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Result, VaultError};
use crate::progress::CancelToken;

/// Read size. Large enough to keep the drive busy, small enough that a
/// cancellation is noticed within a few milliseconds.
const CHUNK: usize = 1024 * 1024;

/// Hashes a whole file, as 64 uppercase hexadecimal characters.
///
/// Uppercase matches what Civitai returns, so a comparison never needs to
/// normalize.
pub fn hash_file(path: &Path) -> Result<String> {
    hash_file_cancellable(path, &CancelToken::new())
}

/// Hashes a whole file, stopping when the caller cancels.
pub fn hash_file_cancellable(path: &Path, cancel: &CancelToken) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| VaultError::from_io(&e, path, "opening the file to identify it"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];

    loop {
        cancel.check()?;
        let n = file
            .read(&mut buf)
            .map_err(|e| VaultError::from_io(&e, path, "reading the file to identify it"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Hashes a file and reports how many bytes it read, for progress.
pub fn hash_file_counting(path: &Path, cancel: &CancelToken, read: &mut u64) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| VaultError::from_io(&e, path, "opening the file to identify it"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];

    loop {
        cancel.check()?;
        let n = file
            .read(&mut buf)
            .map_err(|e| VaultError::from_io(&e, path, "reading the file to identify it"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        *read += n as u64;
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Hashes bytes already in memory.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    to_hex(&h.finalize())
}

fn to_hex(digest: &[u8]) -> String {
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// Is this text a SHA-256 hash?
///
/// Civitai answers the same "not found" for an unknown file and for nonsense,
/// so the shape is checked here rather than by asking the network.
pub fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Puts a hash in the one form the engine stores and compares.
pub fn normalize_sha256(s: &str) -> Option<String> {
    let t = s.trim();
    is_valid_sha256(t).then(|| t.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known answers, so a broken hash cannot pass by agreeing with itself.
    const EMPTY_SHA256: &str = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
    const ABC_SHA256: &str = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

    #[test]
    fn hashing_matches_the_published_answer_for_known_input() {
        assert_eq!(hash_bytes(b""), EMPTY_SHA256);
        assert_eq!(hash_bytes(b"abc"), ABC_SHA256);
    }

    #[test]
    fn a_file_hashes_to_the_same_value_as_its_bytes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("m.safetensors");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(hash_file(&p).unwrap(), ABC_SHA256);
    }

    #[test]
    fn an_empty_file_hashes_without_complaint() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("empty.safetensors");
        std::fs::write(&p, b"").unwrap();
        assert_eq!(hash_file(&p).unwrap(), EMPTY_SHA256);
    }

    #[test]
    fn a_file_larger_than_one_chunk_hashes_correctly() {
        // Proves the chunked loop, which is where an off-by-one would hide.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.safetensors");
        let data: Vec<u8> = (0..(CHUNK * 2 + 12345)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        assert_eq!(hash_file(&p).unwrap(), hash_bytes(&data));
    }

    #[test]
    fn two_files_with_one_content_hash_the_same_whatever_they_are_called() {
        // The product's central claim: the same weights under different names
        // in different folders are one file.
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("loras/awesomeloras");
        let b = d.path().join("loras/newloras");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let content = b"the same weights";
        std::fs::write(a.join("lora1.safetensors"), content).unwrap();
        std::fs::write(b.join("totally-different-name.safetensors"), content).unwrap();

        assert_eq!(
            hash_file(&a.join("lora1.safetensors")).unwrap(),
            hash_file(&b.join("totally-different-name.safetensors")).unwrap()
        );
    }

    #[test]
    fn one_changed_byte_changes_the_hash() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.safetensors");
        let b = d.path().join("b.safetensors");
        std::fs::write(&a, b"weights-v1").unwrap();
        std::fs::write(&b, b"weights-v2").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn hashing_a_missing_file_gives_a_clear_error() {
        let d = tempfile::tempdir().unwrap();
        let err = hash_file(&d.path().join("gone.safetensors")).unwrap_err();
        assert_eq!(err.code, crate::ErrorCode::NotFound);
        assert!(err.path.is_some());
    }

    #[test]
    fn hashing_stops_when_the_caller_cancels() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.safetensors");
        std::fs::write(&p, vec![7u8; CHUNK * 4]).unwrap();

        let cancel = CancelToken::new();
        cancel.cancel();
        let err = hash_file_cancellable(&p, &cancel).unwrap_err();
        assert_eq!(err.code, crate::ErrorCode::Cancelled);
    }

    #[test]
    fn hashing_through_a_link_reads_the_file_it_points_at() {
        // After consolidation an install holds links. Reading one must give the
        // vault file's hash, or a rescan would call every link unreadable.
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("real.safetensors");
        let link = d.path().join("link.safetensors");
        std::fs::write(&target, b"abc").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).unwrap();

        assert_eq!(hash_file(&link).unwrap(), ABC_SHA256);
    }

    #[test]
    fn the_byte_counter_reports_the_whole_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("m.safetensors");
        let size = CHUNK + 999;
        std::fs::write(&p, vec![1u8; size]).unwrap();

        let mut read = 0u64;
        hash_file_counting(&p, &CancelToken::new(), &mut read).unwrap();
        assert_eq!(read, size as u64);
    }

    #[test]
    fn hash_shape_checking_accepts_real_hashes_and_rejects_the_rest() {
        assert!(is_valid_sha256(ABC_SHA256));
        assert!(is_valid_sha256(&ABC_SHA256.to_lowercase()));
        assert!(!is_valid_sha256(""));
        assert!(!is_valid_sha256("zz"));
        assert!(!is_valid_sha256(&ABC_SHA256[..63]), "too short");
        assert!(!is_valid_sha256(&format!("{ABC_SHA256}0")), "too long");
        // 64 characters, but not all hexadecimal.
        assert!(!is_valid_sha256(&"z".repeat(64)));
    }

    #[test]
    fn normalizing_a_hash_uppercases_it_and_trims_it() {
        assert_eq!(normalize_sha256(&format!("  {}  ", ABC_SHA256.to_lowercase())).unwrap(), ABC_SHA256);
        assert_eq!(normalize_sha256("not a hash"), None);
    }
}
