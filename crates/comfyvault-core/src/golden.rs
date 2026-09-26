//! Contract samples, written by the serialiser the product itself uses.
//!
//! Every payload that crosses to the interface has one file in
//! `docs/golden/`. The interface builds its test doubles from those files
//! instead of restating the shapes by hand.
//!
//! This exists because a hand written double agrees with whoever wrote it. The
//! engine sent `huggingfaceCacheDirs`, the interface read
//! `huggingFaceCacheDirs`, and both sides were green: the interface tested
//! against a double that spelled the name the interface's way, and the
//! engine's own test named three fields and stepped over the fourth. Three
//! panels of Settings were empty on every machine from the first run, with no
//! error anywhere.
//!
//! A sample is real output, not a description of output. It is produced by
//! `serde_json` from a real Rust value, so a rename, a retyped field or a
//! changed enum spelling moves the file, and a moved file fails this test.
//!
//! To accept a deliberate change:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p comfyvault-core golden
//! ```
//!
//! Read the diff before committing it. The whole point is that the change is
//! seen.
//!
//! The values are chosen rather than measured. They are written to look like
//! the Windows machine the product runs on, and they are fixed, so the files
//! do not move when a clock ticks or a temporary folder gets a new name. The
//! names, the types and the shapes are the engine's own.

#![cfg(test)]

use std::path::PathBuf;

use serde::Serialize;

use crate::apply::{
    ApplyPhase, ApplyProgress, ApplyStep, InterruptedApply, RevertAction, RevertDrive, RevertPhase,
    RevertPreview, RevertProgress,
};
use crate::engine::{AppState, BusyKind, BusyOp, ScanEntryPage, ScanEntryWithCount, VaultInfo};
use crate::error::{ErrorCode, VaultError};
use crate::install::detect::{InstallCandidate, OutputModelDir, RootOrigin, VersionSource};
use crate::install::extra_paths::ExtraPath;
use crate::install::Install;
use crate::links::{LinkWithState, ModelDirNode};
use crate::metadata::{MetadataSourceName, ModelMetadata};
use crate::plan::{
    BlockReason, BlockedRow, ConsolidationPlan, PlanGroup, PlanLink, PlanSource, PlanTotals,
    SourceChoice,
};
use crate::platform::{
    DriveInfo, DriveKind, LockState, MatchReason, PlatformReport, RunningComfy, SymlinkCapability,
};
use crate::reply::{
    Cancelled, Cleared, CreatedDirectory, CreatedFolder, Deleted, DirEntryInfo, DirectoryListing,
    Removed, RemovedLinks, StartedApply, StartedScan, UnregisterResult,
};
use crate::scan::{ScanPhase, ScanProgress};
use crate::settings::Settings;
use crate::store::{
    ApplyFailure, ApplyRecord, ApplyState, Classification, InstallScanTotals, LinkOrigin,
    LinkRecord, ScanEntryRecord, ScanError, ScanRecord, ScanTotals,
};
use crate::time_util::Timestamp;
use crate::usage::{UsageMatch, UsageResult};
use crate::vault::{ContentPage, ContentRow, NameGroup, VaultFile, VaultHealth, VaultName, VaultPage};

/// Fixed instants, so a sample does not move when the clock does.
const ADDED_AT: Timestamp = Timestamp(1_758_412_800_000); // 2025-09-21T00:00:00Z
const SCAN_START: Timestamp = Timestamp(1_758_499_200_000); // 2025-09-22T00:00:00Z
const SCAN_END: Timestamp = Timestamp(1_758_499_412_000);
const MTIME: i128 = 1_758_240_000_000_000_000;

/// Real hashes, from the engine's own hasher.
///
/// Written by hand they were lowercase, and the engine writes uppercase. A
/// sample is meant to teach the interface what arrives, so it must not carry a
/// value the engine would never send.
fn hash_a() -> String {
    crate::testkit::weights_hash("detail-tweaker")
}

fn hash_b() -> String {
    crate::testkit::weights_hash("sdxl-base")
}

fn vault_dir() -> PathBuf {
    PathBuf::from(r"C:\ComfyVault")
}

fn production() -> Install {
    Install {
        id: "inst-1".into(),
        label: "ComfyUI Main".into(),
        registered_path: PathBuf::from(r"C:\ComfyUI-Main"),
        root: PathBuf::from(r"C:\ComfyUI-Main"),
        models_dir: PathBuf::from(r"C:\ComfyUI-Main\models"),
        version: Some("0.3.41".into()),
        version_source: Some(VersionSource::VersionFile),
        extra_paths: vec![ExtraPath {
            section: "comfyui".into(),
            category: "loras".into(),
            raw_category: "loras".into(),
            path: PathBuf::from(r"D:\ai-models\loras"),
            is_default: false,
            exists: true,
        }],
        output_model_dirs: vec![OutputModelDir {
            path: PathBuf::from(r"C:\ComfyUI-Main\output\models"),
            category: "checkpoints".into(),
            exists: false,
        }],
        added_at: ADDED_AT,
        last_scan_at: Some(SCAN_END),
        last_scan_totals: Some(InstallScanTotals {
            install_id: "inst-1".into(),
            install_label: "ComfyUI Main".into(),
            totals: totals(),
        }),
    }
}

fn totals() -> ScanTotals {
    ScanTotals {
        files_seen: 412,
        movable_files: 388,
        movable_bytes: 612_000_000_000,
        unique_contents: 241,
        unique_bytes: 388_000_000_000,
        reclaimable_bytes: 224_000_000_000,
        duplicate_files: 147,
        already_linked_files: 6,
        already_linked_bytes: 12_000_000_000,
        custom_node_files: 9,
        custom_node_bytes: 1_400_000_000,
        hf_cache_files: 11,
        hf_cache_bytes: 22_000_000_000,
        skipped_files: 4,
        error_count: 1,
        bytes_read: 388_000_000_000,
        bytes_from_cache: 224_000_000_000,
        duration_ms: 212_000,
    }
}

fn link_record() -> LinkRecord {
    LinkRecord {
        id: "link-1".into(),
        install_id: "inst-1".into(),
        abs_path: PathBuf::from(r"C:\ComfyUI-Main\models\loras\detail-tweaker.safetensors"),
        rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        link_name: "detail-tweaker.safetensors".into(),
        sha256: hash_a(),
        vault_rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        created_at: SCAN_END,
        created_by: LinkOrigin::Apply,
        apply_id: Some("apply-1".into()),
    }
}

fn metadata() -> ModelMetadata {
    ModelMetadata {
        sha256: hash_a(),
        source: MetadataSourceName::Civitai,
        fetched_at: SCAN_END,
        found: true,
        model_name: Some("Detail Tweaker".into()),
        model_type: Some("LORA".into()),
        version_name: Some("v1.2".into()),
        base_model: Some("SDXL 1.0".into()),
        trigger_words: vec!["detailed".into()],
        nsfw: false,
        nsfw_level: 0,
        civitai_model_id: Some(58_390),
        civitai_version_id: Some(135_867),
        page_url: Some("https://civitai.com/models/58390".into()),
        download_url: Some("https://civitai.com/api/download/models/135867".into()),
        ambiguous: false,
    }
}

fn vault_file() -> VaultFile {
    VaultFile {
        sha256: hash_a(),
        canonical_name: "detail-tweaker.safetensors".into(),
        category: "loras".into(),
        vault_rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        size_bytes: 151_119_872,
        added_at: ADDED_AT,
        aliases: vec!["detail_tweaker_xl.safetensors".into()],
        link_count: 2,
        links: vec![link_record()],
        metadata: Some(metadata()),
        present: true,
    }
}

fn scan_entry() -> ScanEntryRecord {
    ScanEntryRecord {
        abs_path: PathBuf::from(r"C:\ComfyUI-Main\models\loras\detail-tweaker.safetensors"),
        rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        install_id: "inst-1".into(),
        category: "loras".into(),
        size_bytes: 151_119_872,
        sha256: Some(hash_a()),
        mtime_nanos: MTIME,
        classification: Classification::Movable,
        link_target: None,
    }
}

fn scan_entry_with_count() -> ScanEntryWithCount {
    ScanEntryWithCount { entry: scan_entry(), occurrence_count: 2 }
}

fn content_row() -> ContentRow {
    ContentRow {
        sha256: hash_a(),
        name: "detail-tweaker.safetensors".into(),
        category: "loras".into(),
        size_bytes: 151_119_872,
        aliases: vec!["detail_tweaker_xl.safetensors".into()],
        occurrence_count: 2,
        link_count: 2,
        in_vault: true,
        install_ids: vec!["inst-1".into(), "inst-2".into()],
        added_at: Some(ADDED_AT),
        metadata: Some(metadata()),
    }
}

fn vault_name() -> VaultName {
    VaultName {
        name: "detail-tweaker.safetensors".into(),
        is_canonical: true,
        vault_rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        used_by_links: 2,
        seen_in_installs: vec!["inst-1".into()],
    }
}

fn usage_match() -> UsageMatch {
    UsageMatch {
        install_id: "inst-1".into(),
        install_label: "ComfyUI Main".into(),
        workflow_path: r"C:\ComfyUI-Main\user\default\workflows\portrait.json".into(),
        workflow_name: "portrait.json".into(),
    }
}

fn plan_group() -> PlanGroup {
    PlanGroup {
        group_id: "grp-1".into(),
        sha256: hash_a(),
        size_bytes: 151_119_872,
        category: "loras".into(),
        vault_rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
        vault_name_adjusted: false,
        clashes_with: None,
        vault_aliases: vec!["detail_tweaker_xl.safetensors".into()],
        source: PlanSource {
            install_id: "inst-1".into(),
            install_label: "ComfyUI Main".into(),
            abs_path: PathBuf::from(
                r"C:\ComfyUI-Main\models\loras\detail-tweaker.safetensors",
            ),
            rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
            same_volume_as_vault: true,
            chosen_because: SourceChoice::SameVolume,
            size_bytes: 151_119_872,
            mtime_nanos: MTIME,
        },
        links: vec![
            PlanLink {
                install_id: "inst-1".into(),
                install_label: "ComfyUI Main".into(),
                abs_path: PathBuf::from(
                    r"C:\ComfyUI-Main\models\loras\detail-tweaker.safetensors",
                ),
                rel_path: PathBuf::from(r"loras\detail-tweaker.safetensors"),
                link_name: "detail-tweaker.safetensors".into(),
                name_differs_from_vault: false,
                is_source: true,
                shares_bytes_with_another: false,
                size_bytes: 151_119_872,
                mtime_nanos: MTIME,
            },
            PlanLink {
                install_id: "inst-2".into(),
                install_label: "ComfyUI Testing".into(),
                abs_path: PathBuf::from(r"C:\ComfyUI-Testing\models\loras\detail_tweaker_xl.safetensors"),
                rel_path: PathBuf::from(r"loras\detail_tweaker_xl.safetensors"),
                link_name: "detail_tweaker_xl.safetensors".into(),
                name_differs_from_vault: true,
                is_source: false,
                shares_bytes_with_another: false,
                size_bytes: 151_119_872,
                mtime_nanos: MTIME,
            },
        ],
        occurrences: 2,
        distinct_files: 2,
        bytes_freed: 151_119_872,
        single_copy: false,
        cross_volume: false,
        already_in_vault: false,
    }
}

fn plan() -> ConsolidationPlan {
    ConsolidationPlan {
        plan_id: "plan-1".into(),
        scan_id: "scan-1".into(),
        created_at: SCAN_END,
        vault_root: vault_dir(),
        symlinks_supported: true,
        groups: vec![plan_group()],
        blocked: vec![BlockedRow {
            abs_path: PathBuf::from(r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors"),
            install_id: Some("inst-2".into()),
            install_label: Some("ComfyUI Testing".into()),
            size_bytes: 6_938_040_320,
            sha256: Some(hash_b()),
            reason: BlockReason::FileLocked,
            detail: "ComfyUI is running and has this file open.".into(),
        }],
        totals: PlanTotals {
            groups: 1,
            groups_freeing_space: 1,
            single_copy_groups: 0,
            name_clashes: 0,
            cross_volume_groups: 0,
            bytes_freed: 151_119_872,
            bytes_moved: 0,
            files_moved: 1,
            links_created: 2,
            blocked_rows: 1,
            blocked_bytes: 6_938_040_320,
            vault_free_bytes_if_applied: Some(288_000_000_000),
        },
    }
}

fn settings() -> Settings {
    Settings {
        huggingface_cache_dirs: Some(vec![PathBuf::from(r"D:\hf-cache\hub")]),
        ..Settings::default()
    }
}

fn platform() -> PlatformReport {
    PlatformReport {
        os: "windows".into(),
        symlinks: SymlinkCapability {
            supported: true,
            probe_error: None,
            developer_mode: Some(true),
            elevated: false,
            guidance: None,
        },
        long_paths_enabled: Some(true),
    }
}

fn apply_record() -> ApplyRecord {
    ApplyRecord {
        apply_id: "apply-1".into(),
        plan_id: "plan-1".into(),
        state: ApplyState::CompletedWithErrors,
        started_at: SCAN_END,
        finished_at: Some(Timestamp(SCAN_END.0 + 94_000)),
        groups_requested: 2,
        group_ids: vec!["grp-1".into(), "grp-2".into()],
        groups_applied: 1,
        groups_failed: 1,
        bytes_freed: 151_119_872,
        files_moved: 1,
        links_created: 2,
        vault_free_bytes_before: Some(136_880_119_808),
        vault_free_bytes_after: Some(287_999_999_680),
        failures: vec![ApplyFailure {
            group_id: "grp-2".into(),
            abs_path: r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors".into(),
            reason: BlockReason::FileLocked,
            detail: "ComfyUI is running and has this file open.".into(),
        }],
        revertible: true,
        last_undo_step_at: None,
    }
}

/// Every payload the interface receives, with the name of its file.
fn revert_preview() -> RevertPreview {
    RevertPreview {
        apply_id: "apply-1".into(),
        files_already_back: 0,
        files_renamed_back: 2,
        files_copied_back: 3,
        bytes_to_copy: 13_876_297_728,
        drives: vec![RevertDrive {
            volume: r"C:\".into(),
            predicted_room_bytes: 196_608,
            free_bytes: Some(91_204_567_040),
        }],
    }
}

fn samples() -> Vec<(&'static str, serde_json::Value)> {
    fn s<T: Serialize>(name: &'static str, v: T) -> (&'static str, serde_json::Value) {
        (name, serde_json::to_value(v).expect("serialise sample"))
    }

    vec![
        s("PlatformReport", platform()),
        s(
            "DriveInfo",
            DriveInfo {
                root: r"C:\".into(),
                kind: DriveKind::Fixed,
                free_bytes: Some(288_000_000_000),
                total_bytes: Some(2_000_398_934_016),
            },
        ),
        s(
            "AppState",
            AppState {
                vault_root: Some(r"C:\ComfyVault".into()),
                vault_initialized: true,
                install_count: 2,
                platform: platform(),
                settings: settings(),
                last_scan_id: Some("scan-1".into()),
                // A later scan, cancelled an hour after scan-1 finished.
                last_cancelled_scan_at: Some(Timestamp(SCAN_END.0 + 3_600_000)),
                last_plan_id: Some("plan-1".into()),
                interrupted_applies: vec!["apply-0".into()],
                busy: Some(BusyOp { kind: BusyKind::Scan, id: "scan-2".into() }),
            },
        ),
        s(
            "VaultInfo",
            VaultInfo {
                root: r"C:\ComfyVault".into(),
                created_at: ADDED_AT,
                // The drive root, with its separator, which is what Windows
                // returns. Written "C:" here once, and the interface compared
                // it against a path's first two characters and told people
                // their files would be copied.
                volume: r"C:\".into(),
                free_bytes: Some(288_000_000_000),
                total_bytes: Some(2_000_398_934_016),
                file_count: 241,
                total_stored_bytes: 388_000_000_000,
                schema_version: 1,
            },
        ),
        s("Settings", settings()),
        s("Install", production()),
        s(
            "InstallCandidate",
            InstallCandidate {
                valid: true,
                root: Some(PathBuf::from(r"C:\ComfyUI-Main")),
                nested_depth: 0,
                markers_found: vec!["main.py".into(), "comfy".into(), "models".into()],
                markers_missing: Vec::new(),
                content_check_passed: true,
                other_candidates: Vec::new(),
                version: Some("0.3.41".into()),
                version_source: Some(VersionSource::VersionFile),
                models_dir: Some(PathBuf::from(r"C:\ComfyUI-Main\models")),
                models_dir_exists: true,
                extra_paths_file: Some(PathBuf::from(
                    r"C:\ComfyUI-Main\extra_model_paths.yaml",
                )),
                extra_paths: production().extra_paths,
                extra_paths_problems: vec![
                    r#"In section "comfyui", "..\..\ESCAPED" cannot be a model folder name, so it was skipped. A model folder name cannot contain a path separator."#
                        .into(),
                ],
                output_model_dirs: production().output_model_dirs,
                reason: None,
            },
        ),
        s("UnregisterResult", UnregisterResult { removed: true, links_left_in_place: 14 }),
        s(
            "ModelDirNode",
            ModelDirNode {
                rel_path: "loras".into(),
                abs_path: PathBuf::from(r"C:\ComfyUI-Main\models\loras"),
                category: "loras".into(),
                origin: RootOrigin::ModelsDir,
                file_count: 61,
                children: vec![ModelDirNode {
                    rel_path: r"loras\sdxl".into(),
                    abs_path: PathBuf::from(r"C:\ComfyUI-Main\models\loras\sdxl"),
                    category: "loras".into(),
                    origin: RootOrigin::ModelsDir,
                    file_count: 12,
                    children: Vec::new(),
                }],
            },
        ),
        s("StartedScan", StartedScan { scan_id: "scan-1".into() }),
        s(
            "ScanEntryPage",
            ScanEntryPage {
                total: 412,
                offset: 0,
                entries: vec![scan_entry_with_count()],
            },
        ),
        s("Cancelled", Cancelled { cancelled: true }),
        s(
            "ScanRecord",
            ScanRecord {
                scan_id: "scan-1".into(),
                started_at: SCAN_START,
                finished_at: SCAN_END,
                install_ids: vec!["inst-1".into(), "inst-2".into()],
                cancelled: false,
                totals: totals(),
                per_install: vec![InstallScanTotals {
                    install_id: "inst-1".into(),
                    install_label: "ComfyUI Main".into(),
                    totals: totals(),
                }],
                errors: vec![ScanError {
                    path: r"C:\ComfyUI-Testing\models\checkpoints\broken.safetensors".into(),
                    install_id: Some("inst-2".into()),
                    code: ErrorCode::PermissionDenied,
                    detail: "Windows refused to open this file.".into(),
                }],
            },
        ),
        s("ConsolidationPlan", plan()),
        s("StartedApply", StartedApply { apply_id: "apply-1".into() }),
        s("ApplyRecord", apply_record()),
        s(
            "InterruptedApply",
            InterruptedApply {
                apply_id: "apply-0".into(),
                plan_id: "plan-0".into(),
                started_at: ADDED_AT,
                steps_done: 7,
                steps_pending: 2,
                description: "A consolidation of 4 groups stopped part way through.".into(),
                affected_paths: vec![
                    r"C:\ComfyUI-Main\models\loras\detail-tweaker.safetensors".into(),
                ],
                blocked: false,
                blocked_paths: vec![],
            },
        ),
        s("LinkRecord", link_record()),
        s(
            "LinkWithState",
            LinkWithState { link: link_record(), state: crate::store::LinkState::Dangling },
        ),
        s("Removed", Removed { removed: true }),
        s(
            "CreatedFolder",
            CreatedFolder { abs_path: r"C:\ComfyVault\loras\sdxl".into(), created: true },
        ),
        s("VaultPage", VaultPage { total: 241, offset: 0, files: vec![vault_file()] }),
        s(
            "ContentPage",
            ContentPage {
                total: 241,
                offset: 0,
                rows: vec![content_row()],
                scan_id: Some("scan-1".into()),
            },
        ),
        s(
            "NameGroup",
            NameGroup {
                sha256: hash_a(),
                size_bytes: 151_119_872,
                category: "loras".into(),
                canonical_name: "detail-tweaker.safetensors".into(),
                names: vec![vault_name()],
            },
        ),
        s("VaultFile", vault_file()),
        s(
            "Deleted",
            Deleted {
                deleted: true,
                bytes_freed: 151_119_872,
                links_removed: vec![
                    "C:\\ComfyUI-A\\models\\loras\\style.safetensors".into(),
                    "C:\\ComfyUI-B\\models\\loras\\new\\style.safetensors".into(),
                ],
            },
        ),
        s(
            "VaultHealth",
            VaultHealth {
                checked_links: 388,
                checked_files: 241,
                dangling_links: vec![link_record()],
                replaced_links: Vec::new(),
                missing_vault_files: Vec::new(),
                foreign_files: vec![r"C:\ComfyVault\loras\dropped-in-by-hand.safetensors".into()],
                stopped_deletes: vec![vault_file()],
                ok: false,
            },
        ),
        s("RemovedLinks", RemovedLinks { removed: 3 }),
        s(
            "UsageResult",
            UsageResult {
                name: "detail-tweaker.safetensors".into(),
                used: true,
                matches: vec![usage_match()],
                method: "Read every saved workflow in both installs.".into(),
                searched: true,
            },
        ),
        s("ModelMetadata", metadata()),
        s("Cleared", Cleared { cleared: 241 }),
        s(
            "RunningComfy",
            RunningComfy {
                pid: 18_244,
                name: "python.exe".into(),
                exe_path: Some(r"C:\ComfyUI-Main\python_embeded\python.exe".into()),
                cwd: Some(r"C:\ComfyUI-Main".into()),
                command_line: vec!["python.exe".into(), "main.py".into()],
                matched_install_ids: vec!["inst-1".into()],
                match_reason: MatchReason::ExeUnderRoot,
                started_at: Some(SCAN_START),
                listening_ports: Some(vec![8188]),
                holds_model_files: Some(true),
            },
        ),
        s(
            "LockState",
            LockState {
                path: r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors".into(),
                locked: true,
                checkable: true,
                detail: Some("Another program has this file open.".into()),
            },
        ),
        s(
            "DirectoryListing",
            DirectoryListing {
                path: r"D:\ai-models".into(),
                parent: Some(r"D:\".into()),
                entries: vec![DirEntryInfo {
                    name: "loras".into(),
                    path: r"D:\ai-models\loras".into(),
                    is_directory: true,
                    is_symlink: false,
                }],
            },
        ),
        s(
            "CreatedDirectory",
            CreatedDirectory { path: r"D:\ai-models\new".into(), created: true },
        ),
        // Streamed while work runs, not returned by a command.
        s(
            "ScanProgress",
            ScanProgress {
                scan_id: "scan-1".into(),
                phase: ScanPhase::Hashing,
                install_id: Some("inst-1".into()),
                install_label: Some("ComfyUI Main".into()),
                files_seen: 412,
                files_to_hash: 388,
                files_hashed: 211,
                bytes_to_hash: 612_000_000_000,
                bytes_hashed: 318_000_000_000,
                bytes_from_cache: 94_000_000_000,
                current_path: Some(
                    r"C:\ComfyUI-Main\models\checkpoints\sdxl-base.safetensors".into(),
                ),
                elapsed_ms: 128_000,
                eta_ms: Some(94_000),
            },
        ),
        s(
            "ApplyProgress",
            ApplyProgress {
                apply_id: "apply-1".into(),
                phase: ApplyPhase::Applying,
                group_index: 1,
                group_total: 2,
                current_group_id: Some("grp-2".into()),
                current_path: Some(
                    r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors".into(),
                ),
                step: ApplyStep::Linking,
                bytes_moved: 0,
                bytes_to_move: 0,
                bytes_freed: 151_119_872,
                files_moved: 1,
                links_created: 2,
                failures: 0,
                elapsed_ms: 41_000,
                eta_ms: Some(12_000),
            },
        ),
        s(
            "RevertProgress",
            RevertProgress {
                apply_id: "apply-1".into(),
                phase: RevertPhase::Restoring,
                step_index: 3,
                step_total: 9,
                current_path: Some(
                    r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors".into(),
                ),
                action: RevertAction::CopyingBack,
                files_put_back: 1,
                files_to_put_back: 3,
                links_removed: 1,
                bytes_copied: 2_147_483_648,
                bytes_to_copy: 13_876_297_728,
                elapsed_ms: 19_000,
                eta_ms: Some(104_000),
            },
        ),
        s("RevertPreview", revert_preview()),
        s("RevertDrive", revert_preview().drives.into_iter().next().unwrap()),
        // Pieces that only ever arrive inside a larger payload. They get
        // their own sample so that the interface can build a double for one
        // row without standing up a whole plan.
        s("PlanGroup", plan_group()),
        // A group for content the vault holds already: nothing moves in, and
        // every copy, the source included, becomes a link.
        s("PlanGroupAlreadyInVault", {
            let mut g = plan_group();
            g.already_in_vault = true;
            g.single_copy = false;
            g.cross_volume = false;
            g.bytes_freed = g.size_bytes * g.distinct_files;
            g
        }),
        s("PlanLink", plan_group().links.into_iter().next().unwrap()),
        s("PlanSource", plan_group().source),
        s("PlanTotals", plan().totals),
        s("BlockedRow", plan().blocked.into_iter().next().unwrap()),
        s("ExtraPath", production().extra_paths.into_iter().next().unwrap()),
        s("OutputModelDir", production().output_model_dirs.into_iter().next().unwrap()),
        s("ScanTotals", totals()),
        s(
            "InstallScanTotals",
            InstallScanTotals {
                install_id: "inst-1".into(),
                install_label: "ComfyUI Main".into(),
                totals: totals(),
            },
        ),
        s(
            "ScanError",
            ScanError {
                path: r"C:\ComfyUI-Testing\models\checkpoints\broken.safetensors".into(),
                install_id: Some("inst-2".into()),
                code: ErrorCode::PermissionDenied,
                detail: "Windows refused to open this file.".into(),
            },
        ),
        s("ScanEntryRecord", scan_entry()),
        s("ScanEntryWithCount", scan_entry_with_count()),
        s("ContentRow", content_row()),
        s("VaultName", vault_name()),
        s("UsageMatch", usage_match()),
        s("ApplyFailure", apply_record().failures.into_iter().next().unwrap()),
        s("SymlinkCapability", platform().symlinks),
        s(
            "DirEntryInfo",
            DirEntryInfo {
                name: "loras".into(),
                path: r"D:\ai-models\loras".into(),
                is_directory: true,
                is_symlink: false,
            },
        ),
        s("BusyOp", BusyOp { kind: BusyKind::Scan, id: "scan-2".into() }),
        // What a command rejects with.
        s(
            "VaultError",
            VaultError {
                code: ErrorCode::FileLocked,
                message: "ComfyUI is running and has this file open. Close it and try again."
                    .into(),
                detail: Some("python.exe, process 18244".into()),
                path: Some(r"C:\ComfyUI-Testing\models\checkpoints\sdxl-base.safetensors".into()),
            },
        ),
    ]
}

/// Where the repository is, for the checks that read files from it.
///
/// `CARGO_MANIFEST_DIR` is fixed when the crate is compiled, which is right
/// for an ordinary `cargo test`. It is wrong when a test program is built on
/// one machine and run on another, which is how the Windows suite is run from
/// Linux. `COMFYVAULT_REPO` overrides it for that case. See `docs/BUILD.md`.
fn repo_root() -> PathBuf {
    match std::env::var_os("COMFYVAULT_REPO") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    }
}

fn read_repo_file(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\n\
             This check reads the repository. If this program was built on one \
             machine and is running on another, set COMFYVAULT_REPO to the \
             repository folder as this machine sees it."
        , path.display())
    })
}

/// A drive root ends with a separator on every system this runs on.
fn ends_with_a_separator(s: &str) -> bool {
    s.ends_with('\\') || s.ends_with('/')
}

fn golden_dir() -> PathBuf {
    repo_root().join("docs/golden")
}

#[test]
fn the_contract_samples_on_disk_match_what_the_engine_serialises() {
    let dir = golden_dir();
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    if update {
        std::fs::create_dir_all(&dir).expect("create the golden folder");
    }

    // An empty list would make every comparison below pass by making none.
    let all = samples();
    assert!(all.len() >= 50, "only {} samples were built, the check proves nothing", all.len());

    let mut stale = Vec::new();
    for (name, value) in all {
        let file = dir.join(format!("{name}.json"));
        let mut text = serde_json::to_string_pretty(&value).expect("write sample");
        text.push('\n');

        if update {
            std::fs::write(&file, &text).expect("write the sample file");
            continue;
        }

        match std::fs::read_to_string(&file) {
            Ok(on_disk) if on_disk == text => {}
            Ok(_) => stale.push(format!("{name}: the file on disk is not what the engine sends")),
            Err(_) => stale.push(format!("{name}: docs/golden/{name}.json is missing")),
        }
    }

    assert!(
        stale.is_empty(),
        "The contract samples no longer match the engine.\n{}\n\n\
         If the change is meant, accept it and read the diff:\n    \
         UPDATE_GOLDEN=1 cargo test -p comfyvault-core golden\n\
         The interface builds its test doubles from these files, so a name that \
         moves here moves there too.",
        stale.join("\n")
    );
}

#[test]
fn every_payload_a_command_returns_has_a_sample() {
    // Reads the command layer's own source, so a new command without a sample
    // fails here rather than reaching the interface as an undocumented shape.
    let source = read_repo_file("src-tauri/src/commands.rs");

    let mut wanted: Vec<String> = Vec::new();
    // Only functions the interface can actually call. The crate also has a
    // generic helper returning `Reply<T>`, which is plumbing, not a payload.
    for block in source.split("#[tauri::command]").skip(1) {
        let Some(piece) = block.split("-> Reply<").nth(1) else { continue };
        let inner = piece.split('>').next().unwrap_or_default().trim();
        // Unwrap the containers a reply may be wrapped in.
        let inner = inner
            .trim_start_matches("Vec<")
            .trim_start_matches("Option<")
            .trim_start_matches("Vec<")
            .trim();
        if inner.is_empty() || inner == "()" {
            continue;
        }
        if !wanted.iter().any(|w| w == inner) {
            wanted.push(inner.to_string());
        }
    }
    assert!(wanted.len() > 20, "the command layer was not parsed, found {:?}", wanted);

    let have: Vec<&str> = samples().into_iter().map(|(n, _)| n).collect();
    let missing: Vec<&String> = wanted.iter().filter(|w| !have.contains(&w.as_str())).collect();
    assert!(
        missing.is_empty(),
        "these command replies have no contract sample: {missing:?}\n\
         Add one to samples() in this file, then run with UPDATE_GOLDEN=1."
    );
}

#[test]
fn the_contract_document_agrees_with_every_sample_it_declares() {
    // The document is a second copy of every shape, and two copies drift. It
    // said `huggingFaceCacheDirs` while the engine said `huggingfaceCacheDirs`,
    // and it promised `state` on every link, which the engine never sent.
    // Where the document lists fields for a payload that has a sample, the two
    // must name the same fields.
    let text = read_repo_file("docs/IPC-CONTRACT.md");

    let mut declared: Vec<(String, Vec<String>)> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let opens = line.starts_with("type ") && line.trim_end().ends_with("= {");
        if !opens {
            i += 1;
            continue;
        }
        let name = line["type ".len()..].split_whitespace().next().unwrap_or("").to_string();
        let mut fields = Vec::new();
        i += 1;
        while i < lines.len() && !lines[i].starts_with('}') {
            let l = lines[i];
            // `  fieldName: type` and nothing else. Comment and blank lines skip.
            if let Some(rest) = l.strip_prefix("  ") {
                if !rest.starts_with("//") && !rest.starts_with(' ') {
                    if let Some(field) = rest.split(':').next() {
                        let field = field.trim().trim_end_matches('?');
                        if !field.is_empty()
                            && field.chars().all(|c| c.is_ascii_alphanumeric())
                        {
                            fields.push(field.to_string());
                        }
                    }
                }
            }
            i += 1;
        }
        if !fields.is_empty() {
            declared.push((name, fields));
        }
        i += 1;
    }
    assert!(declared.len() > 20, "the contract was not parsed, found {}", declared.len());

    let mut wrong = Vec::new();
    let mut checked = 0;
    for (name, fields) in &declared {
        let Some((_, value)) = samples().into_iter().find(|(n, _)| n == name) else {
            continue;
        };
        let Some(object) = value.as_object() else { continue };
        checked += 1;

        let mut engine: Vec<&str> = object.keys().map(|k| k.as_str()).collect();
        engine.sort_unstable();
        let mut written: Vec<&str> = fields.iter().map(|f| f.as_str()).collect();
        written.sort_unstable();

        let missing: Vec<&&str> = engine.iter().filter(|k| !written.contains(k)).collect();
        let invented: Vec<&&str> = written.iter().filter(|k| !engine.contains(k)).collect();
        if !missing.is_empty() {
            wrong.push(format!("{name}: the engine sends {missing:?}, the contract does not list them"));
        }
        if !invented.is_empty() {
            wrong.push(format!("{name}: the contract promises {invented:?}, the engine does not send them"));
        }
    }
    assert!(checked > 15, "only {checked} payloads were compared, the parser is wrong");
    assert!(
        wrong.is_empty(),
        "docs/IPC-CONTRACT.md no longer matches the engine.\n{}\n\n\
         Correct the document, or correct the engine. The interface reads the \
         document, so a promise it cannot keep reaches a person as an empty panel.",
        wrong.join("\n")
    );
}

#[test]
fn a_flattened_payload_survives_the_real_wire_and_comes_back() {
    // `#[serde(flatten)]` buffers through an internal value type that does not
    // accept every Rust number. `ScanEntryWithCount` flattens a record holding
    // a 128 bit timestamp, so the combination is proved here rather than
    // assumed. Tauri sends a string, so the string is what is tested.
    for (name, value) in samples() {
        let text = serde_json::to_string(&value)
            .unwrap_or_else(|e| panic!("{name} could not be written as JSON: {e}"));
        let back: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name} could not be read back: {e}"));
        assert_eq!(back, value, "{name} changed on the way through JSON text");
    }

    // And the two flattened shapes specifically, from the real Rust value.
    let entry = scan_entry_with_count();
    let text = serde_json::to_string(&entry).expect("write the scan entry");
    let back: ScanEntryWithCount = serde_json::from_str(&text).expect("read the scan entry back");
    assert_eq!(back, entry);
    assert!(text.contains("\"mtimeNanos\":\"1758240000000000000\""), "the timestamp lost its value: {text}");
    assert!(text.contains("\"occurrenceCount\":2"), "the count is missing: {text}");

    let link = LinkWithState { link: link_record(), state: crate::store::LinkState::Dangling };
    let text = serde_json::to_string(&link).expect("write the link");
    let back: LinkWithState = serde_json::from_str(&text).expect("read the link back");
    assert_eq!(back, link);
}

#[test]
fn no_sample_carries_a_value_the_engine_would_never_send() {
    // The values in a sample are chosen by hand, so they can be wrong in a way
    // the shape checks cannot see. Written by hand the hashes were lowercase
    // and the engine writes uppercase, which would have taught the interface
    // to compare the wrong text.
    fn walk(name: &str, value: &serde_json::Value, wrong: &mut Vec<String>, seen: &mut usize) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == "sha256" {
                        if let Some(text) = v.as_str() {
                            *seen += 1;
                            if crate::scan::hash::normalize_sha256(text).as_deref() != Some(text) {
                                wrong.push(format!("{name}: {k} is {text:?}, which the engine would rewrite"));
                            }
                        }
                    }
                    walk(name, v, wrong, seen);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    walk(name, v, wrong, seen);
                }
            }
            _ => {}
        }
    }

    let mut wrong = Vec::new();
    let mut seen = 0usize;
    for (name, value) in samples() {
        walk(name, &value, &mut wrong, &mut seen);
    }
    // A walk that stops finding hashes would pass by checking nothing.
    assert!(seen >= 10, "only {seen} hashes were read, the check proves nothing");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_file_time_written_as_a_number_by_an_older_build_still_reads() {
    // Vaults written before file times became text hold a plain number. The
    // person's hash cache and plan history must survive the change, so both
    // spellings are accepted when reading.
    let as_text = serde_json::to_string(&scan_entry()).expect("write");
    let as_number = as_text.replace(
        "\"mtimeNanos\":\"1758240000000000000\"",
        "\"mtimeNanos\":1758240000000000000",
    );
    assert_ne!(as_text, as_number, "the replacement did not happen, the test proves nothing");

    let old: ScanEntryRecord = serde_json::from_str(&as_number).expect("read an older record");
    assert_eq!(old, scan_entry(), "an older record did not come back the same");
}

#[test]
fn no_example_in_the_contract_shows_a_path_the_engine_would_not_send() {
    // The document showed vaultRelPath as 'loras/lora1.safetensors'. The engine
    // sends 'loras\\lora1.safetensors'. An example is what a reader copies, so a
    // wrong one is read as the promise and built against. The interface had a
    // string replacement in its report working around exactly this.
    //
    // Only fields the engine actually sends with a backslash are checked. A
    // path sent to the engine may use either separator, and this must not
    // start policing arguments.
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if v.as_str().is_some_and(|s| s.contains('\\')) && !out.contains(k) {
                        out.push(k.clone());
                    }
                    collect(v, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| collect(v, out)),
            _ => {}
        }
    }

    let mut path_fields = Vec::new();
    for (_, value) in samples() {
        collect(&value, &mut path_fields);
    }
    assert!(
        path_fields.iter().any(|f| f == "vaultRelPath"),
        "no path fields were found, the check proves nothing: {path_fields:?}"
    );

    let doc = read_repo_file("docs/IPC-CONTRACT.md");
    let mut wrong = Vec::new();
    let mut examined = 0usize;
    for (number, line) in doc.lines().enumerate() {
        let Some((declaration, comment)) = line.split_once("//") else { continue };
        let Some(field) = declaration.trim().split(':').next() else { continue };
        let field = field.trim();
        if !path_fields.iter().any(|f| f == field) {
            continue;
        }
        // Only the quoted example, so prose about separators is left alone.
        for example in comment.split('\'').skip(1).step_by(2) {
            examined += 1;
            if example.contains('/') && !example.contains('\\') {
                wrong.push(format!(
                    "line {}: {field} is shown as {example:?}, and the engine sends a backslash",
                    number + 1
                ));
            }
        }
    }
    // Without this the check passes by reading nothing at all. If the way the
    // document writes an example ever changes, this fails instead of going
    // quiet, which is the failure this whole file exists to stop.
    assert!(examined >= 2, "only {examined} examples were read, the check proves nothing");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every value of an enum the interface compares against, with the exact text
/// it becomes on the wire.
///
/// The `match` is what keeps this honest. Adding a value to one of these enums
/// stops this file compiling, so the new value cannot reach the interface
/// without someone writing down what it is called.
fn every_enum_value() -> Vec<(&'static str, String, &'static str)> {
    use crate::plan::{BlockReason as B, SourceChoice as S};
    use crate::store::{ApplyState as A, Classification as C, LinkOrigin as O, LinkState as L};

    fn wire(value: &impl Serialize) -> String {
        serde_json::to_value(value)
            .expect("serialise the value")
            .as_str()
            .expect("an enum reaches the interface as a string")
            .to_string()
    }

    let mut out: Vec<(&str, String, &str)> = Vec::new();

    for c in [C::Movable, C::CustomNodes, C::HuggingFaceCache, C::AlreadyInVault, C::ExternalLink, C::Unreadable] {
        let expected = match c {
            C::Movable => "movable",
            C::CustomNodes => "customNodes",
            C::HuggingFaceCache => "huggingFaceCache",
            C::AlreadyInVault => "alreadyInVault",
            C::ExternalLink => "externalLink",
            C::Unreadable => "unreadable",
        };
        out.push(("Classification", wire(&c), expected));
    }

    for b in [
        B::FileLocked, B::FileChanged, B::FileMissing, B::PermissionDenied, B::InCustomNodes,
        B::InHuggingFaceCache, B::AlreadyInVault, B::ExternalLink, B::SymlinkUnsupported,
        B::VaultInsideInstall, B::TargetExistsNotLink, B::UnsafeVaultPath, B::NotEnoughSpace,
        B::ReadError,
    ] {
        let expected = match b {
            B::FileLocked => "fileLocked",
            B::FileChanged => "fileChanged",
            B::FileMissing => "fileMissing",
            B::PermissionDenied => "permissionDenied",
            B::InCustomNodes => "inCustomNodes",
            B::InHuggingFaceCache => "inHuggingFaceCache",
            B::AlreadyInVault => "alreadyInVault",
            B::ExternalLink => "externalLink",
            B::SymlinkUnsupported => "symlinkUnsupported",
            B::VaultInsideInstall => "vaultInsideInstall",
            B::TargetExistsNotLink => "targetExistsNotLink",
            B::UnsafeVaultPath => "unsafeVaultPath",
            B::NotEnoughSpace => "notEnoughSpace",
            B::ReadError => "readError",
        };
        out.push(("BlockReason", wire(&b), expected));
    }

    for l in [L::Ok, L::Dangling, L::Replaced, L::Missing] {
        let expected = match l {
            L::Ok => "ok",
            L::Dangling => "dangling",
            L::Replaced => "replaced",
            L::Missing => "missing",
        };
        out.push(("LinkState", wire(&l), expected));
    }

    for a in [
        A::Running, A::Completed, A::CompletedWithErrors, A::Cancelled, A::Interrupted,
        A::PartlyReverted, A::Reverted, A::SetAside,
    ] {
        let expected = match a {
            A::Running => "running",
            A::Completed => "completed",
            A::CompletedWithErrors => "completedWithErrors",
            A::Cancelled => "cancelled",
            A::Interrupted => "interrupted",
            A::PartlyReverted => "partlyReverted",
            A::Reverted => "reverted",
            A::SetAside => "setAside",
        };
        out.push(("ApplyState", wire(&a), expected));
    }

    for o in [O::Apply, O::Manual, O::Download] {
        let expected = match o {
            O::Apply => "apply",
            O::Manual => "manual",
            O::Download => "download",
        };
        out.push(("LinkOrigin", wire(&o), expected));
    }

    for s in [S::SameVolume, S::OnlyCopy, S::FirstByPath] {
        let expected = match s {
            S::SameVolume => "sameVolume",
            S::OnlyCopy => "onlyCopy",
            S::FirstByPath => "firstByPath",
        };
        out.push(("SourceChoice", wire(&s), expected));
    }

    for p in [RevertPhase::Restoring, RevertPhase::Finalizing] {
        let expected = match p {
            RevertPhase::Restoring => "restoring",
            RevertPhase::Finalizing => "finalizing",
        };
        out.push(("RevertPhase", wire(&p), expected));
    }

    for a in [
        RevertAction::RemovingLink, RevertAction::RenamingBack, RevertAction::CopyingBack,
        RevertAction::Tidying,
    ] {
        let expected = match a {
            RevertAction::RemovingLink => "removingLink",
            RevertAction::RenamingBack => "renamingBack",
            RevertAction::CopyingBack => "copyingBack",
            RevertAction::Tidying => "tidying",
        };
        out.push(("RevertAction", wire(&a), expected));
    }

    for e in crate::error::ErrorCode::every() {
        let text = serde_json::to_value(e).unwrap();
        out.push(("ErrorCode", text.as_str().unwrap().to_string(), ""));
    }
    out
}

#[test]
fn every_enum_reaches_the_interface_as_the_text_it_is_meant_to() {
    // The interface compares against these strings. A rename attribute beats
    // anything a reader of the Rust would predict, which is the shape of the
    // bug that shipped, so the text is asserted rather than derived.
    let values = every_enum_value();
    assert!(values.len() >= 40, "only {} values were read, the check proves nothing", values.len());

    let wrong: Vec<String> = values
        .iter()
        .filter(|(_, got, expected)| !expected.is_empty() && got != expected)
        .map(|(kind, got, expected)| format!("{kind}: the wire says {got:?}, it must say {expected:?}"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_contract_lists_the_same_enum_values_the_engine_sends() {
    // A value the engine can send and the document never names is a value the
    // interface has nothing to do with when it arrives.
    let doc = read_repo_file("docs/IPC-CONTRACT.md");
    let values = every_enum_value();

    let mut checked = 0;
    let mut wrong = Vec::new();
    for kind in ["Classification", "BlockReason", "LinkState", "ErrorCode", "RevertAction", "ApplyState"] {
        let engine: Vec<&str> = values
            .iter()
            .filter(|(k, _, _)| *k == kind)
            .map(|(_, got, _)| got.as_str())
            .collect();
        assert!(!engine.is_empty(), "{kind} produced no values");

        for value in &engine {
            checked += 1;
            // The document writes a value either as 'quoted' in a union or as
            // `code` in the error table.
            let quoted = format!("'{value}'");
            let ticked = format!("`{value}`");
            if !doc.contains(&quoted) && !doc.contains(&ticked) {
                wrong.push(format!("{kind}: the engine can send {value:?} and the contract never names it"));
            }
        }
    }
    assert!(checked >= 35, "only {checked} values were compared, the check proves nothing");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_program_icon_has_every_size_windows_draws() {
    // The icon was one flat 32 pixel square, so Explorer, the taskbar and the
    // title bar scaled that one image to every size. Windows picks the image
    // closest to the size it draws, so the file must carry each of them.
    let config = read_repo_file("src-tauri/tauri.conf.json");
    let config: serde_json::Value = serde_json::from_str(&config).unwrap();
    let listed = config["bundle"]["icon"].as_array().expect("no icons are listed");
    for icon in listed {
        let path = repo_root().join("src-tauri").join(icon.as_str().unwrap());
        assert!(path.is_file(), "{} is listed and missing", path.display());
    }

    let ico = std::fs::read(repo_root().join("src-tauri/icons/icon.ico")).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0], "icon.ico is not an icon file");
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    // Each entry's first byte is its width, where 0 means 256.
    let sizes: Vec<u32> = (0..count)
        .map(|i| match ico[6 + 16 * i] {
            0 => 256,
            w => w as u32,
        })
        .collect();
    for want in [16, 24, 32, 48, 64, 256] {
        assert!(sizes.contains(&want), "icon.ico has no {want} pixel image, only {sizes:?}");
    }
}

#[test]
fn only_one_copy_of_the_program_runs_at_a_time() {
    // A second copy opened a second window on the same vault. The single
    // instance plugin stops it, but only when it is the first plugin: Tauri
    // starts plugins in order, and one before it would run in the second copy
    // too. The real behavior is proven on Windows by
    // src-tauri/tests/single_instance.rs.
    let wiring = read_repo_file("src-tauri/src/lib.rs");
    let first = wiring.find(".plugin(").expect("no plugin is registered");
    assert!(
        wiring[first..].starts_with(".plugin(tauri_plugin_single_instance::init("),
        "the single instance plugin must be the first plugin registered"
    );
}

#[test]
fn every_command_is_wired_into_the_window() {
    // A command that is written but never listed in the handler does not exist
    // at run time. The build says nothing, and the interface gets "command not
    // found" from a function that is plainly there in the source.
    let commands = read_repo_file("src-tauri/src/commands.rs");
    let wiring = read_repo_file("src-tauri/src/lib.rs");

    let mut declared = Vec::new();
    for block in commands.split("#[tauri::command]").skip(1) {
        let Some(after) = block.split("fn ").nth(1) else { continue };
        // A command generic over the runtime reads `name<R: Runtime>(`.
        let Some(name) = after.split(['(', '<']).next() else { continue };
        let name = name.trim();
        if !name.is_empty() && !declared.contains(&name) {
            declared.push(name);
        }
    }
    assert!(declared.len() > 40, "only {} commands were parsed", declared.len());

    let missing: Vec<&&str> = declared
        .iter()
        .filter(|n| !wiring.contains(&format!("commands::{n},")))
        .collect();
    assert!(
        missing.is_empty(),
        "these commands are written but not listed in the handler, so the \
         interface cannot call them: {missing:?}"
    );
}

#[test]
fn the_sample_values_that_come_from_the_operating_system_have_the_shape_it_gives() {
    // Most sample values are the engine's own data, and a wrong one shows up
    // the moment anyone looks. A few are whatever the operating system hands
    // back, and for those the literal here says what somebody typed rather
    // than what the engine will send.
    //
    // It has already cost a person a wrong message. `volume` was written "C:"
    // and Windows returns "C:\\". The interface compared the two, decided the
    // vault was on a different drive from every install, and said their files
    // would be copied when they would in fact be renamed.
    //
    // Neither a check on names nor a check on kinds can see this: the name was
    // right and both are strings. So the value is compared against a live
    // answer from the platform this is running on.
    let platform = crate::platform::NativePlatform;
    let all = samples();
    let by_name = |name: &str| -> serde_json::Value {
        all.iter().find(|(n, _)| *n == name).expect("sample").1.clone()
    };

    // volume: the real shape, asked of the real operating system.
    let dir = tempfile::tempdir().expect("temp dir");
    let real = crate::platform::Platform::volume_id(&platform, dir.path())
        .expect("this machine must be able to name its own drive")
        .0;
    let info = by_name("VaultInfo");
    let sampled = info["volume"].as_str().expect("volume is text").to_string();
    assert!(!real.is_empty(), "the platform named no drive at all");
    // On Windows naming a drive gives its root, which ends with a separator.
    // On other systems it is an opaque device number and has no shape to
    // check, so only the live Windows answer is held to the rule. The sample
    // shows a Windows machine either way, so it is always held to it.
    if cfg!(windows) {
        assert!(
            ends_with_a_separator(&real),
            "this system named a drive as {real:?}, with no separator on the end"
        );
    }
    assert!(
        ends_with_a_separator(&sampled),
        "the sample says volume is {sampled:?}. A drive root ends with a \
         separator, and an interface that compares this against the start of a \
         path will decide the vault is somewhere it is not"
    );

    // os: the sample claims to show a Windows machine, and the name must be
    // one the engine really produces rather than a word chosen here.
    let report = by_name("PlatformReport");
    let os = report["os"].as_str().expect("os is text");
    assert!(
        ["windows", "linux", "macos"].contains(&os),
        "the sample's operating system name is {os:?}, which the engine never sends"
    );

    // drive roots: same rule as volume.
    let drives = crate::platform::Platform::drives(&platform);
    let first = drives.first().expect("a computer has a drive");
    let drive = by_name("DriveInfo");
    let sampled_root = drive["root"].as_str().expect("root is text");
    assert!(
        ends_with_a_separator(&first.root),
        "this system reported a drive root as {:?}, with no separator",
        first.root
    );
    let _ = &drives;
    assert!(
        ends_with_a_separator(sampled_root),
        "the sample says a drive root is {sampled_root:?}, with no separator"
    );
}

#[test]
fn the_window_loads_no_picture_from_anywhere_on_the_network() {
    // The app shows no remote picture, so neither content rule may name a
    // place on the network where an image can come from: not the one Tauri
    // applies, and not the one the page carries itself. A remote host in
    // either would let an address from a model's details reach a third party.
    let conf: serde_json::Value = serde_json::from_str(&read_repo_file("src-tauri/tauri.conf.json")).unwrap();
    let from_tauri = conf["app"]["security"]["csp"].as_str().expect("the window has content rules").to_string();
    let page = read_repo_file("index.html");
    let from_page = page
        .split("http-equiv=\"Content-Security-Policy\" content=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the page carries its own content rules")
        .to_string();

    for (whose, csp) in [("tauri.conf.json", from_tauri), ("index.html", from_page)] {
        let img_src = csp
            .split(';')
            .map(str::trim)
            .find_map(|d| d.strip_prefix("img-src "))
            .unwrap_or_else(|| panic!("{whose}: the rules do not say where images load from"));
        let remote = remote_sources(img_src);
        assert!(remote.is_empty(), "{whose}: img-src lets images load from {remote:?}");
    }
}

/// The sources in a content rule that reach the network. Only the app's own
/// sources are not: the page itself, inline data, and Tauri's local asset
/// protocol under its exact name. Everything else counts, however it is
/// written: a host, a host with no scheme, a scheme on its own, a wildcard.
fn remote_sources(sources: &str) -> Vec<&str> {
    const LOCAL: [&str; 4] = ["'self'", "data:", "asset:", "http://asset.localhost"];
    sources.split_whitespace().filter(|t| !LOCAL.contains(t)).collect()
}

#[test]
fn every_way_of_naming_a_remote_image_source_is_caught() {
    for remote in [
        "https://image.civitai.com",
        "image.civitai.com",
        "*",
        "https://*.civitai.com",
        "*.civitai.com",
        "https:",
        "http:",
        "blob:",
        "http://evil.localhost",
        "http://*.localhost",
    ] {
        assert_eq!(remote_sources(&format!("'self' data: {remote}")), vec![remote], "{remote}");
    }
    assert!(remote_sources("'self' asset: http://asset.localhost data:").is_empty());
}
