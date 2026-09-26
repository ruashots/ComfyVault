//! Small replies that a command sends back when there is nothing larger to say.
//!
//! These live here, and not in the command layer, for two reasons. They are
//! part of the contract, so their samples must come from the real serialiser,
//! and the command layer cannot be built on a machine that has no desktop
//! libraries. A later command line or server front end answers with the same
//! shapes rather than restating them.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterResult {
    pub removed: bool,
    pub links_left_in_place: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedScan {
    pub scan_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cancelled {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedApply {
    pub apply_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Removed {
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedFolder {
    pub abs_path: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub deleted: bool,
    pub bytes_freed: u64,
    /// Every install link the delete removed. Empty unless it was asked to
    /// remove them.
    pub links_removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedLinks {
    pub removed: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cleared {
    pub cleared: u64,
}

/// One entry in a folder listing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryInfo {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    /// The entry is a link. Following it may leave the folder being browsed.
    pub is_symlink: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<DirEntryInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedDirectory {
    pub path: String,
    pub created: bool,
}
