//! The one error type the engine returns, and the stable code the UI matches on.
//!
//! Every variant carries a sentence a person can read. Nothing here ever holds a
//! stack trace, a credential, or an API key: `VaultError` crosses the IPC
//! boundary and is rendered in the interface.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable machine-readable code. The UI switches on this, never on the message.
///
/// The set is documented in `docs/IPC-CONTRACT.md` section 1.3. Adding a variant
/// is a contract change and gets announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    NotInitialized,
    VaultBusy,
    InvalidArgument,
    NotFound,
    PathOutsideBoundary,
    NotAComfyInstall,
    AlreadyRegistered,
    IoError,
    PermissionDenied,
    FileLocked,
    FileChanged,
    SymlinkUnsupported,
    StoreError,
    ParseError,
    NetworkUnavailable,
    Cancelled,
    Conflict,
}

impl ErrorCode {
    /// Every code, for the checks that compare what the engine can send
    /// against what the contract names.
    ///
    /// The match is what keeps it complete: a new code stops this compiling.
    #[cfg(test)]
    pub fn every() -> Vec<Self> {
        let all = vec![
            Self::NotInitialized,
            Self::VaultBusy,
            Self::InvalidArgument,
            Self::NotFound,
            Self::PathOutsideBoundary,
            Self::NotAComfyInstall,
            Self::AlreadyRegistered,
            Self::IoError,
            Self::PermissionDenied,
            Self::FileLocked,
            Self::FileChanged,
            Self::SymlinkUnsupported,
            Self::StoreError,
            Self::ParseError,
            Self::NetworkUnavailable,
            Self::Cancelled,
            Self::Conflict,
        ];
        for c in &all {
            match c {
                Self::NotInitialized => {}
                Self::VaultBusy => {}
                Self::InvalidArgument => {}
                Self::NotFound => {}
                Self::PathOutsideBoundary => {}
                Self::NotAComfyInstall => {}
                Self::AlreadyRegistered => {}
                Self::IoError => {}
                Self::PermissionDenied => {}
                Self::FileLocked => {}
                Self::FileChanged => {}
                Self::SymlinkUnsupported => {}
                Self::StoreError => {}
                Self::ParseError => {}
                Self::NetworkUnavailable => {}
                Self::Cancelled => {}
                Self::Conflict => {}
            }
        }
        all
    }
}


impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInitialized => "notInitialized",
            Self::VaultBusy => "vaultBusy",
            Self::InvalidArgument => "invalidArgument",
            Self::NotFound => "notFound",
            Self::PathOutsideBoundary => "pathOutsideBoundary",
            Self::NotAComfyInstall => "notAComfyInstall",
            Self::AlreadyRegistered => "alreadyRegistered",
            Self::IoError => "ioError",
            Self::PermissionDenied => "permissionDenied",
            Self::FileLocked => "fileLocked",
            Self::FileChanged => "fileChanged",
            Self::SymlinkUnsupported => "symlinkUnsupported",
            Self::StoreError => "storeError",
            Self::ParseError => "parseError",
            Self::NetworkUnavailable => "networkUnavailable",
            Self::Cancelled => "cancelled",
            Self::Conflict => "conflict",
        }
    }
}

/// The error shape that crosses the IPC boundary.
///
/// `message` is for a person. `detail` is for a developer. `path` is the file
/// that caused it, when there is one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl VaultError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), detail: None, path: None }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(crate::paths::display_path(path.as_ref()));
        self
    }

    pub fn not_initialized() -> Self {
        Self::new(
            ErrorCode::NotInitialized,
            "No vault folder is open yet. Choose a vault folder to continue.",
        )
    }

    pub fn busy(kind: &str) -> Self {
        Self::new(
            ErrorCode::VaultBusy,
            format!("A {kind} is already running. Wait for it to finish, or cancel it."),
        )
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "The operation was cancelled.")
    }

    pub fn outside_boundary(path: &Path, boundary: &Path) -> Self {
        Self::new(
            ErrorCode::PathOutsideBoundary,
            "That folder is outside the install this app is allowed to change.",
        )
        .with_detail(format!(
            "resolved path escaped its boundary; boundary = {}",
            crate::paths::display_path(boundary)
        ))
        .with_path(path)
    }

    /// Turns an [`std::io::Error`] into the right code, so the UI can tell
    /// "you lack permission" apart from "the disk refused".
    pub fn from_io(err: &std::io::Error, path: &Path, doing: &str) -> Self {
        use std::io::ErrorKind;
        let (code, message) = match err.kind() {
            ErrorKind::PermissionDenied => (
                ErrorCode::PermissionDenied,
                format!("Windows refused access while {doing}. Another program may hold the file, or the file may be read only."),
            ),
            ErrorKind::NotFound => (
                ErrorCode::NotFound,
                format!("The file is no longer there. It was removed or renamed while {doing}."),
            ),
            ErrorKind::AlreadyExists => (
                ErrorCode::Conflict,
                format!("Something already exists at that path, so {doing} was stopped. Nothing was overwritten."),
            ),
            _ => (
                ErrorCode::IoError,
                format!("The disk refused the operation while {doing}."),
            ),
        };
        Self::new(code, message).with_detail(err.to_string()).with_path(path)
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)?;
        if let Some(d) = &self.detail {
            write!(f, " ({d})")?;
        }
        Ok(())
    }
}

impl std::error::Error for VaultError {}

impl From<redb::Error> for VaultError {
    fn from(e: redb::Error) -> Self {
        VaultError::new(ErrorCode::StoreError, "The vault database refused the change.")
            .with_detail(e.to_string())
    }
}

macro_rules! redb_err {
    ($t:ty) => {
        impl From<$t> for VaultError {
            fn from(e: $t) -> Self {
                VaultError::new(
                    ErrorCode::StoreError,
                    "The vault database refused the change.",
                )
                .with_detail(e.to_string())
            }
        }
    };
}
redb_err!(redb::DatabaseError);
redb_err!(redb::TransactionError);
redb_err!(redb::TableError);
redb_err!(redb::StorageError);
redb_err!(redb::CommitError);

impl From<serde_json::Error> for VaultError {
    fn from(e: serde_json::Error) -> Self {
        VaultError::new(ErrorCode::StoreError, "A stored record could not be read.")
            .with_detail(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, VaultError>;

/// Attaches the path and the action to an `io::Result`, so callers never have to
/// remember which file they were touching.
pub(crate) trait IoResultExt<T> {
    fn ctx(self, path: impl AsRef<Path>, doing: &str) -> Result<T>;
}

impl<T> IoResultExt<T> for std::io::Result<T> {
    fn ctx(self, path: impl AsRef<Path>, doing: &str) -> Result<T> {
        self.map_err(|e| VaultError::from_io(&e, path.as_ref(), doing))
    }
}

/// A path plus the reason it could not be used, for the rows a plan blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathProblem {
    pub path: PathBuf,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_serializes_to_its_documented_camel_case_string() {
        // The UI matches on these strings. A rename here is a contract break, so
        // this test exists to make the break loud.
        let cases = [
            (ErrorCode::NotInitialized, "notInitialized"),
            (ErrorCode::VaultBusy, "vaultBusy"),
            (ErrorCode::InvalidArgument, "invalidArgument"),
            (ErrorCode::NotFound, "notFound"),
            (ErrorCode::PathOutsideBoundary, "pathOutsideBoundary"),
            (ErrorCode::NotAComfyInstall, "notAComfyInstall"),
            (ErrorCode::AlreadyRegistered, "alreadyRegistered"),
            (ErrorCode::IoError, "ioError"),
            (ErrorCode::PermissionDenied, "permissionDenied"),
            (ErrorCode::FileLocked, "fileLocked"),
            (ErrorCode::FileChanged, "fileChanged"),
            (ErrorCode::SymlinkUnsupported, "symlinkUnsupported"),
            (ErrorCode::StoreError, "storeError"),
            (ErrorCode::ParseError, "parseError"),
            (ErrorCode::NetworkUnavailable, "networkUnavailable"),
            (ErrorCode::Cancelled, "cancelled"),
            (ErrorCode::Conflict, "conflict"),
        ];
        for (code, expected) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{expected}\""), "serde name drifted");
            assert_eq!(code.as_str(), expected, "as_str drifted from serde");
        }
    }

    #[test]
    fn error_json_omits_absent_optional_fields() {
        let e = VaultError::new(ErrorCode::NotFound, "gone");
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "notFound");
        assert_eq!(v["message"], "gone");
        assert!(v.get("detail").is_none(), "absent detail must not appear as null");
        assert!(v.get("path").is_none(), "absent path must not appear as null");
    }

    #[test]
    fn permission_denied_io_error_keeps_its_own_code() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = VaultError::from_io(&io, Path::new("/tmp/x"), "moving the file");
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert!(e.path.is_some());
        // The person-facing message must not be the raw OS string.
        assert!(!e.message.contains("denied"), "raw os text leaked into the message");
    }

    #[test]
    fn already_exists_io_error_maps_to_conflict_not_io() {
        let io = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists");
        let e = VaultError::from_io(&io, Path::new("/tmp/x"), "creating the link");
        assert_eq!(e.code, ErrorCode::Conflict);
        assert!(e.message.contains("Nothing was overwritten"));
    }
}
