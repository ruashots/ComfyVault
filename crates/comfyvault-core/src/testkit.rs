//! Fabricated worlds for tests.
//!
//! **Every test builds its own tree in a temporary folder and deletes it
//! afterwards. The engine's tests never read, list or touch a real ComfyUI
//! install.** That is not a convenience, it is a rule of this project: a
//! machine running these tests may hold real installs with a terabyte of
//! weights in them, and they are off limits.
//!
//! A world holds a vault and however many fabricated installs a test asks for,
//! all under one temporary folder, with a [`FakePlatform`] so a test can make a
//! file look locked or make a drive look separate.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::install::{detect, Install};
use crate::platform::FakePlatform;
use crate::settings::Settings;
use crate::store::Store;

/// A vault, some installs, and a platform a test can lie to.
pub struct TestWorld {
    pub dir: TempDir,
    pub vault_root: PathBuf,
    pub store: Store,
    pub platform: FakePlatform,
    pub settings: Settings,
    next_id: std::cell::Cell<u32>,
}

impl TestWorld {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let vault_root = dir.path().join("ComfyVault");
        let store = Store::open(&vault_root, true).expect("open vault");
        let settings = Settings {
            min_file_size_bytes: 0,
            // A test must never walk the machine's real Hugging Face cache.
            // On a machine that runs ComfyUI that folder holds real models,
            // and a scan would count files no test put there.
            huggingface_cache_dirs: Some(Vec::new()),
            ..Default::default()
        };
        store.put_settings(&settings).expect("store settings");
        Self {
            vault_root,
            store,
            platform: FakePlatform::new(),
            // Tests use real files, but small ones. The one megabyte floor has
            // its own test rather than forcing every other test to write a
            // megabyte per file.
            settings,
            next_id: std::cell::Cell::new(1),
            dir,
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Builds a folder that passes the install test, and registers it.
    pub fn add_install(&self, label: &str) -> Install {
        let root = self.dir.path().join(label);
        detect::fixtures::make_install(&root);
        self.register(label, &root)
    }

    /// Builds an install with a version recorded, for the thumbnail warning.
    pub fn add_install_with_version(&self, label: &str, version: &str) -> Install {
        let root = self.dir.path().join(label);
        detect::fixtures::make_install(&root);
        detect::fixtures::set_version(&root, version);
        self.register(label, &root)
    }

    fn register(&self, label: &str, root: &Path) -> Install {
        let id = format!("inst-{}", self.next_id.get());
        self.next_id.set(self.next_id.get() + 1);
        let c = detect::inspect(root).expect("inspect");
        assert!(c.valid, "the fabricated install did not pass the install test");
        let install =
            Install::from_candidate(id, label.to_string(), root.to_path_buf(), &c).expect("install");
        self.store.put_install(&install).expect("store install");
        install
    }

    /// Re-reads an install after its folders changed on disk.
    pub fn refresh(&self, install: &Install) -> Install {
        let c = detect::inspect(&install.root).expect("inspect");
        let mut updated =
            Install::from_candidate(install.id.clone(), install.label.clone(), install.registered_path.clone(), &c)
                .expect("install");
        updated.added_at = install.added_at;
        self.store.put_install(&updated).expect("store install");
        updated
    }

    /// Writes a model file inside an install, creating folders as needed.
    ///
    /// `rel` is relative to the install root, for example
    /// `models/loras/awesomeloras/lora1.safetensors`.
    pub fn write_model(&self, install: &Install, rel: &str, content: &[u8]) -> PathBuf {
        let p = install.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).expect("create model folder");
        std::fs::write(&p, content).expect("write model");
        p
    }

    /// Writes a file anywhere under the world's folder.
    pub fn write_file(&self, rel: &str, content: &[u8]) -> PathBuf {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).expect("create folder");
        std::fs::write(&p, content).expect("write file");
        p
    }

    /// Points an install's `extra_model_paths.yaml` at a folder.
    pub fn add_extra_model_path(&self, install: &Install, category: &str, dir: &Path) -> Install {
        std::fs::create_dir_all(dir).expect("create extra folder");
        let yaml = install.root.join("extra_model_paths.yaml");
        let existing = std::fs::read_to_string(&yaml).unwrap_or_default();
        let section = format!(
            "extra{}:\n    base_path: {}\n    {}: .\n",
            self.next_id.get(),
            dir.display(),
            category
        );
        self.next_id.set(self.next_id.get() + 1);
        std::fs::write(&yaml, format!("{existing}{section}")).expect("write yaml");
        self.refresh(install)
    }

    pub fn scanner(&self) -> crate::scan::Scanner<'_> {
        crate::scan::Scanner::new(&self.store, &self.platform, self.settings.clone())
    }

    pub fn planner(&self) -> crate::plan::Planner<'_> {
        crate::plan::Planner::new(&self.store, &self.platform)
    }

    /// Runs a scan and stores the result, the way the command layer does.
    ///
    /// Each scan gets its own identifier, as the engine gives it, so a test
    /// that scans twice is really looking at two scans.
    pub fn scan(&self, installs: &[Install]) -> crate::scan::ScanOutcome {
        let id = format!("scan-{}", uuid::Uuid::new_v4().simple());
        let out = self
            .scanner()
            .scan(
                &id,
                installs,
                &crate::progress::CancelToken::new(),
                &crate::progress::NullSink,
            )
            .expect("scan");
        self.store.put_scan(&out.record).expect("store scan");
        self.store
            .put_scan_entries(&out.record.scan_id, &out.entries)
            .expect("store entries");
        out
    }

    /// Scans, then plans.
    pub fn plan(&self, installs: &[Install]) -> crate::plan::ConsolidationPlan {
        let out = self.scan(installs);
        let plan = self
            .planner()
            .build("plan-test", &out.record.scan_id, &out.entries, installs)
            .expect("plan");
        self.store.put_plan(&plan).expect("store plan");
        plan
    }

    /// Where a link points, or `None` when the path is not a link.
    pub fn link_target(&self, p: &Path) -> Option<PathBuf> {
        std::fs::read_link(p).ok()
    }

    pub fn is_link(&self, p: &Path) -> bool {
        std::fs::symlink_metadata(p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    /// Reads a file through whatever link chain reaches it.
    pub fn read(&self, p: &Path) -> Vec<u8> {
        std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }
}

impl Default for TestWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes a sparse file: a short header naming `tag`, then `len` bytes in all,
/// the rest a hole that occupies nothing on the drive.
///
/// On Windows the file is marked sparse by `fsutil`, the tool a person would
/// use, not by the engine's own code, so a test built on it cannot agree with
/// the engine by sharing a mistake.
pub fn write_sparse(path: &Path, tag: &str, len: u64) {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().unwrap()).expect("create folder");
    let mut f = std::fs::File::create(path).expect("create sparse file");
    #[cfg(windows)]
    {
        let out = std::process::Command::new("fsutil")
            .args(["sparse", "setflag"])
            .arg(path)
            .output()
            .expect("run fsutil");
        assert!(out.status.success(), "fsutil could not mark the file sparse: {out:?}");
    }
    f.write_all(format!("SPARSE:{tag}:").as_bytes()).expect("write header");
    f.set_len(len).expect("extend with a hole");
    f.sync_all().expect("flush");
}

/// Bytes that stand in for model weights.
///
/// Distinct tags give distinct content, and therefore distinct hashes. The
/// padding keeps the files big enough to look real without being big enough to
/// slow the tests down.
pub fn weights(tag: &str) -> Vec<u8> {
    let mut v = format!("WEIGHTS:{tag}:").into_bytes();
    v.resize(4096, b'\0');
    // Fill the tail deterministically so two tags never collide by accident.
    let seed = tag.bytes().fold(7u8, |a, b| a.wrapping_mul(31).wrapping_add(b));
    for (i, byte) in v.iter_mut().enumerate().skip(64) {
        *byte = seed.wrapping_add((i % 251) as u8);
    }
    v
}

/// The hash of [`weights`] with the same tag.
pub fn weights_hash(tag: &str) -> String {
    crate::scan::hash::hash_bytes(&weights(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_builds_a_vault_and_an_install() {
        let w = TestWorld::new();
        let i = w.add_install("Production");
        assert!(i.root.join("folder_paths.py").is_file());
        assert!(w.vault_root.is_dir());
        assert_eq!(w.store.installs().unwrap().len(), 1);
    }

    #[test]
    fn two_tags_give_two_different_contents() {
        assert_ne!(weights("a"), weights("b"));
        assert_ne!(weights_hash("a"), weights_hash("b"));
        assert_eq!(weights_hash("a"), weights_hash("a"));
    }

    #[test]
    fn a_written_model_hashes_to_its_tags_hash() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let p = w.write_model(&i, "models/loras/x.safetensors", &weights("lora1"));
        assert_eq!(crate::scan::hash::hash_file(&p).unwrap(), weights_hash("lora1"));
    }

    #[test]
    fn an_extra_model_path_is_read_back_from_the_install() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let shared = w.path().join("shared-loras");
        let i = w.add_extra_model_path(&i, "loras", &shared);

        assert_eq!(i.extra_paths.len(), 1);
        assert_eq!(i.extra_paths[0].category, "loras");
        assert!(i.extra_paths[0].exists);
    }

    #[test]
    fn the_world_is_deleted_when_it_goes_out_of_scope() {
        let path = {
            let w = TestWorld::new();
            w.add_install("A");
            w.path().to_path_buf()
        };
        assert!(!path.exists(), "a test must not leave its tree behind");
    }
}
