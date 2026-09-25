//! The shapes the vault database stores.
//!
//! These are storage records, not the shapes the interface sees. A few differ
//! on purpose: a modification time is stored as whole nanoseconds so the scan
//! cache can compare it exactly, and rendered as a readable timestamp at the
//! boundary, because a nanosecond count does not survive a JSON number in a
//! browser.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::time_util::Timestamp;

/// One unique content in the vault.
///
/// The vault keeps one real file per content. Every other name that content
/// arrived under becomes a link beside it, listed in `aliases`, so the vault
/// shows every name the model was ever known by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFileRecord {
    pub sha256: String,
    /// The name of the real file.
    pub canonical_name: String,
    pub category: String,
    pub size_bytes: u64,
    pub added_at: Timestamp,
    /// Other names, each a link beside the real file.
    pub aliases: Vec<String>,
}

impl VaultFileRecord {
    /// Where the real file sits, relative to the vault root.
    pub fn vault_rel_path(&self) -> PathBuf {
        PathBuf::from(&self.category).join(&self.canonical_name)
    }

    pub fn alias_rel_path(&self, alias: &str) -> PathBuf {
        PathBuf::from(&self.category).join(alias)
    }

    /// Every name this content carries, the real one first.
    pub fn all_names(&self) -> Vec<String> {
        let mut v = vec![self.canonical_name.clone()];
        v.extend(self.aliases.iter().cloned());
        v
    }
}

/// Who created a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkOrigin {
    Apply,
    Manual,
}

/// What a recorded link looks like on disk right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkState {
    /// The link is there and it resolves.
    Ok,
    /// The link is there and it points at nothing.
    ///
    /// This is the worst state. ComfyUI lists the model in its menu and then
    /// fails to load it, and a custom node that re-downloads the "missing"
    /// model writes through the link into the vault.
    Dangling,
    /// A real file sits where the link belonged.
    Replaced,
    /// Nothing is there at all.
    Missing,
}

/// One symbolic link the engine created inside an install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRecord {
    pub id: String,
    pub install_id: String,
    pub abs_path: PathBuf,
    pub rel_path: PathBuf,
    /// The name the link carries, which is the name the file had. It can differ
    /// from the vault file's name after a clash.
    pub link_name: String,
    pub sha256: String,
    pub vault_rel_path: PathBuf,
    pub created_at: Timestamp,
    pub created_by: LinkOrigin,
    /// The apply run that made it, when it was not made by hand.
    pub apply_id: Option<String>,
}

/// A hash the engine already computed.
///
/// The key is the path. The row is reused only when the size and the
/// modification time both still match, so an edited file is always read again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashCacheRecord {
    pub path: PathBuf,
    pub size_bytes: u64,
    #[serde(with = "crate::time_util::nanos_as_string")]
    pub mtime_nanos: i128,
    pub sha256: String,
}

impl HashCacheRecord {
    /// Can this row stand in for reading the file?
    pub fn matches(&self, size_bytes: u64, mtime_nanos: i128) -> bool {
        self.size_bytes == size_bytes && self.mtime_nanos == mtime_nanos
    }
}

/// What the scan decided about one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Classification {
    /// A weight file the engine may consolidate.
    Movable,
    /// Bundled with a custom node. Counted, never moved: those packs reach
    /// their weights by a path relative to their own folder, so moving one
    /// breaks the node.
    CustomNodes,
    /// Inside the Hugging Face cache. Counted, never moved.
    HuggingFaceCache,
    /// Already a link into this vault.
    AlreadyInVault,
    /// A link that points somewhere that is not this vault.
    ExternalLink,
    /// The file could not be read.
    Unreadable,
}

impl Classification {
    pub fn is_movable(self) -> bool {
        matches!(self, Self::Movable)
    }

    /// Counted in the report, never moved.
    pub fn is_counted_only(self) -> bool {
        matches!(self, Self::CustomNodes | Self::HuggingFaceCache)
    }
}

/// One file a scan found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntryRecord {
    pub abs_path: PathBuf,
    pub rel_path: PathBuf,
    pub install_id: String,
    pub category: String,
    pub size_bytes: u64,
    /// Absent when the file could not be read.
    pub sha256: Option<String>,
    #[serde(with = "crate::time_util::nanos_as_string")]
    pub mtime_nanos: i128,
    pub classification: Classification,
    /// Where it points, when the entry is a link.
    pub link_target: Option<PathBuf>,
}

/// Totals for a whole scan, or for one install inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanTotals {
    pub files_seen: u64,
    pub movable_files: u64,
    pub movable_bytes: u64,
    pub unique_contents: u64,
    pub unique_bytes: u64,
    /// `movable_bytes` minus `unique_bytes`: the space a full apply returns.
    pub reclaimable_bytes: u64,
    pub duplicate_files: u64,
    pub already_linked_files: u64,
    pub already_linked_bytes: u64,
    pub custom_node_files: u64,
    pub custom_node_bytes: u64,
    pub hf_cache_files: u64,
    pub hf_cache_bytes: u64,
    pub skipped_files: u64,
    pub error_count: u64,
    pub bytes_read: u64,
    pub bytes_from_cache: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallScanTotals {
    pub install_id: String,
    pub install_label: String,
    #[serde(flatten)]
    pub totals: ScanTotals,
}

/// A path the scan could not read. Never stops the scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub path: String,
    pub install_id: Option<String>,
    pub code: crate::ErrorCode,
    pub detail: String,
}

/// The stored result of one scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub scan_id: String,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub install_ids: Vec<String>,
    pub cancelled: bool,
    pub totals: ScanTotals,
    pub per_install: Vec<InstallScanTotals>,
    pub errors: Vec<ScanError>,
}

/// How an apply run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyState {
    Running,
    Completed,
    CompletedWithErrors,
    Cancelled,
    /// The process stopped in the middle. The journal says where.
    Interrupted,
    /// An undo started and did not finish: it was stopped, it failed, or the
    /// app closed. Every path still holds its file or a working link, and
    /// undoing again finishes the rest.
    PartlyReverted,
    Reverted,
    /// A cut-off run the engine would not touch, because it names places
    /// outside the vault and the registered installs, which the person set
    /// aside. Nothing on the disk was changed by that, and it can no longer be
    /// finished or undone from the app.
    SetAside,
}

/// One group that did not go through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyFailure {
    pub group_id: String,
    pub abs_path: String,
    pub reason: crate::plan::BlockReason,
    pub detail: String,
}

/// The stored result of one apply run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRecord {
    pub apply_id: String,
    pub plan_id: String,
    pub state: ApplyState,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub groups_requested: u64,
    /// Exactly the groups the caller named.
    ///
    /// Resume needs this. Without it a resumed run cannot know what was asked
    /// for, and substituting the whole plan moves and deletes files the person
    /// deliberately left unticked. A record from an older build has none, and
    /// then a resume does nothing, which is the safe direction.
    #[serde(default)]
    pub group_ids: Vec<String>,
    pub groups_applied: u64,
    pub groups_failed: u64,
    pub bytes_freed: u64,
    pub files_moved: u64,
    pub links_created: u64,
    /// Free space on the vault's drive, read from the drive itself, once
    /// before the first file moved and once after the last one.
    ///
    /// Both are measurements, not sums. `bytesFreed` says what the run
    /// accounted for, and these two say what the drive actually reports, which
    /// is the number a person checks the product against. They can disagree
    /// for honest reasons: something else on the computer wrote or deleted
    /// files during the run, the files were sparse and never occupied what
    /// their size claimed, or the drive rounds to its allocation unit.
    ///
    /// Null when the drive could not be read. Never a subtraction.
    #[serde(default)]
    pub vault_free_bytes_before: Option<u64>,
    #[serde(default)]
    pub vault_free_bytes_after: Option<u64>,
    pub failures: Vec<ApplyFailure>,
    pub revertible: bool,
    /// When an undo last began a step on the disk. Null if no undo ever did.
    ///
    /// Written before each step, in the database, so a step cut off by a crash
    /// is never missed. A scan that finished before this time saw files an
    /// undo has since put back, whether that undo finished, was stopped,
    /// failed, or was cut off.
    #[serde(default)]
    pub last_undo_step_at: Option<Timestamp>,
}

/// Where one journal entry stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JournalState {
    /// Written, not performed yet. Finding one of these after a restart means
    /// the process stopped between the record and the action.
    Pending,
    Done,
    Failed,
    /// Put back while an apply rolled back a group it could not finish.
    Reverted,
    /// Put back by an undo the person asked for. Kept apart from `Reverted`
    /// so a partly undone run can say how many of its files are back, even
    /// after a restart.
    Undone,
}

/// One filesystem step, with everything needed to undo it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JournalStep {
    /// A folder the engine made. Undone by removing it, if it is still empty.
    CreateDir { path: PathBuf },
    /// The chosen copy moved into the vault. Undone by moving it back.
    MoveToVault {
        from: PathBuf,
        to: PathBuf,
        /// True when the move was a copy and a delete, because the two paths
        /// sit on different drives.
        copied: bool,
        sha256: String,
        size_bytes: u64,
    },
    /// A duplicate renamed aside, before its link takes its place. Undone by
    /// renaming it back.
    StashOriginal { path: PathBuf, stash: PathBuf },
    /// A link created. Undone by removing the link.
    CreateLink { link: PathBuf, target: PathBuf },
    /// A stashed duplicate deleted. Undone by copying the content back out of
    /// the vault, which is safe because the content is identical by hash.
    ///
    /// `vault_path` is recorded so the undo does not have to look the file up:
    /// a revert walks backwards, so the vault file is still in place when this
    /// step is undone.
    DeleteStash {
        stash: PathBuf,
        original: PathBuf,
        vault_path: PathBuf,
        sha256: String,
        size_bytes: u64,
        /// The duplicate's own modification time, so the copy put back carries
        /// it. A journal written by an older build has none.
        #[serde(default, with = "crate::time_util::optional_nanos_as_string")]
        mtime_nanos: Option<i128>,
    },
    /// A link removed. Undone by creating it again.
    RemoveLink { link: PathBuf, target: PathBuf },
}

/// One row of the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub apply_id: String,
    pub seq: u64,
    pub group_id: String,
    pub step: JournalStep,
    pub state: JournalState,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_row_matches_only_when_size_and_time_both_match() {
        let r = HashCacheRecord {
            path: PathBuf::from("/m/a"),
            size_bytes: 100,
            mtime_nanos: 500,
            sha256: "X".into(),
        };
        assert!(r.matches(100, 500));
        assert!(!r.matches(101, 500), "a changed size must force a re-read");
        assert!(!r.matches(100, 501), "a changed time must force a re-read");
        // The trap this guards: an edit that keeps the byte count. Content
        // changes far more often with the size than without, but the time moves
        // either way, so both are compared.
        assert!(!r.matches(100, 499));
    }

    #[test]
    fn a_vault_file_puts_its_names_in_its_category_folder() {
        let f = VaultFileRecord {
            sha256: "AA".into(),
            canonical_name: "lora1.safetensors".into(),
            category: "loras".into(),
            size_bytes: 1,
            added_at: Timestamp::now(),
            aliases: vec!["alt.safetensors".into()],
        };
        assert_eq!(f.vault_rel_path(), PathBuf::from("loras").join("lora1.safetensors"));
        assert_eq!(f.alias_rel_path("alt.safetensors"), PathBuf::from("loras").join("alt.safetensors"));
        assert_eq!(f.all_names(), vec!["lora1.safetensors", "alt.safetensors"]);
    }

    #[test]
    fn classification_knows_what_may_move() {
        assert!(Classification::Movable.is_movable());
        for c in [
            Classification::CustomNodes,
            Classification::HuggingFaceCache,
            Classification::AlreadyInVault,
            Classification::ExternalLink,
            Classification::Unreadable,
        ] {
            assert!(!c.is_movable(), "{c:?} must never be moved");
        }
        assert!(Classification::CustomNodes.is_counted_only());
        assert!(Classification::HuggingFaceCache.is_counted_only());
        assert!(!Classification::AlreadyInVault.is_counted_only());
    }

    #[test]
    fn a_journal_step_round_trips_with_its_kind_tag() {
        // Recovery reads these back after a crash. A shape that does not
        // survive the round trip is a run that cannot be undone.
        let steps = vec![
            JournalStep::CreateDir { path: "/v/loras".into() },
            JournalStep::MoveToVault {
                from: "/i/a.safetensors".into(),
                to: "/v/loras/a.safetensors".into(),
                copied: true,
                sha256: "AA".into(),
                size_bytes: 10,
            },
            JournalStep::StashOriginal {
                path: "/i/b.safetensors".into(),
                stash: "/i/b.safetensors.comfyvault-old".into(),
            },
            JournalStep::CreateLink {
                link: "/i/b.safetensors".into(),
                target: "/v/loras/a.safetensors".into(),
            },
            JournalStep::DeleteStash {
                stash: "/i/b.safetensors.comfyvault-old".into(),
                original: "/i/b.safetensors".into(),
                vault_path: "/v/loras/a.safetensors".into(),
                sha256: "AA".into(),
                size_bytes: 10,
                mtime_nanos: Some(1_758_240_000_123_456_789),
            },
            JournalStep::RemoveLink {
                link: "/i/c.safetensors".into(),
                target: "/v/loras/a.safetensors".into(),
            },
        ];
        for s in steps {
            let json = serde_json::to_string(&s).unwrap();
            assert!(json.contains("\"kind\""), "the tag is what identifies the step");
            let back: JournalStep = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn install_totals_flatten_so_the_ui_reads_one_object() {
        let t = InstallScanTotals {
            install_id: "i1".into(),
            install_label: "Production".into(),
            totals: ScanTotals { movable_files: 7, ..Default::default() },
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["installId"], "i1");
        assert_eq!(v["movableFiles"], 7, "the totals must be flattened, not nested");
        assert!(v.get("totals").is_none());
    }
}
