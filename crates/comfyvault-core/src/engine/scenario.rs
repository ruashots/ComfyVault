//! One person's whole session, through the engine's own commands, in the order
//! the screens call them: two installs, one with its loras folder on another
//! drive, a scan, a consolidation, a download, one name for a model and its
//! undo, a delete, and the undo of the consolidation.
//!
//! After every step it checks what the person would see, then opens the vault
//! again and checks once more. It downloads a small public file from Hugging
//! Face, so it is left out of the usual run:
//!
//! ```text
//! cargo test -p comfyvault-core engine::scenario -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;
use crate::download::{DownloadState, LinkChoice, StartDownload};
use crate::platform::NativePlatform;
use crate::progress::NullSink;
use crate::testkit::{weights, weights_hash};

const TINY: &str = "https://huggingface.co/hf-internal-testing/tiny-random-gpt2/blob/main/model.safetensors";
const TINY_SHA: &str = "8111D5AFB0715DBF5A31396D31432CB56370BA23F6650A035EA0FC8A20B4E500";

struct Session {
    dir: tempfile::TempDir,
    engine: Arc<Engine>,
    vault: PathBuf,
    a: Install,
    b: Install,
    /// Where B's loras folder really is.
    d_loras: PathBuf,
}

/// What the person sees at one moment, reduced to what the checks compare.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    vault_files: u64,
    links: u64,
    name_groups: usize,
    /// Every model path in both installs: its name and what it loads.
    installs: Vec<(String, &'static str)>,
}

impl Session {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_platform(dir.path().join("config").join("config.json"), Arc::new(NativePlatform::new()));
        let vault = dir.path().join("Vault");
        engine.select_vault(&vault, true).unwrap();
        let mut settings = engine.settings().unwrap();
        settings.min_file_size_bytes = 0;
        settings.huggingface_cache_dirs = Some(Vec::new());
        engine.store().unwrap().put_settings(&settings).unwrap();

        let a_root = dir.path().join("ComfyUI-A");
        let b_root = dir.path().join("ComfyUI-B");
        crate::install::detect::fixtures::make_install(&a_root);
        crate::install::detect::fixtures::make_install(&b_root);
        let d_loras = dir.path().join("D-drive").join("loras");
        std::fs::create_dir_all(&d_loras).unwrap();
        std::fs::create_dir_all(b_root.join("models")).unwrap();
        crate::links::tests::junction(&b_root.join("models").join("loras"), &d_loras);

        let write = |p: PathBuf, tag: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, weights(tag)).unwrap();
        };
        // One model under two names, one under the same name, and one only
        // in A.
        write(a_root.join("models/loras/style.safetensors"), "style");
        write(d_loras.join("style-v2.safetensors"), "style");
        write(a_root.join("models/checkpoints/base.safetensors"), "base");
        write(b_root.join("models/checkpoints/base.safetensors"), "base");
        write(a_root.join("models/vae/only.safetensors"), "vae");

        let a = engine.register_install(&a_root, Some("A".into())).unwrap();
        let b = engine.register_install(&b_root, Some("B".into())).unwrap();
        Self { dir, engine, vault, a, b, d_loras }
    }

    fn model_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for folder in [self.a.root.join("models"), self.b.root.join("models")] {
            for e in walkdir::WalkDir::new(&folder).follow_links(true) {
                match e {
                    Ok(e) if !e.file_type().is_dir() => out.push(e.path().to_path_buf()),
                    Ok(_) => {}
                    // A link that leads nowhere is an error to the walk. It is
                    // kept, so the check below reports it rather than miss it.
                    Err(err) => out.extend(err.path().map(Path::to_path_buf)),
                }
            }
        }
        out.sort();
        out
    }

    /// What the person sees, after checking what must hold at every moment:
    /// every path holds a file or a working link, every record matches the
    /// disk, the health check is clean, and nothing is left behind.
    fn look(&self, step: &str) -> Seen {
        let e = &self.engine;
        let mut installs = Vec::new();
        for p in self.model_paths() {
            let name = p.strip_prefix(self.dir.path()).unwrap().display().to_string().replace('\\', "/");
            assert!(!name.contains(".cvtmp") && !name.contains(crate::apply::fsops::STASH_SUFFIX), "{step}: left behind: {name}");
            let bytes = std::fs::read(&p).unwrap_or_else(|_| panic!("{step}: {name} loads nothing"));
            let what = ["style", "base", "vae"]
                .into_iter()
                .find(|t| bytes == weights(t))
                .unwrap_or(if crate::scan::hash::hash_bytes(&bytes) == TINY_SHA { "tiny" } else { "?" });
            assert_ne!(what, "?", "{step}: {name} holds something unexpected");
            installs.push((name, what));
        }

        let health = e.vault_health().unwrap();
        assert!(health.ok, "{step}: the health check is not clean: {health:?}");
        let links = e.links(None, None, None).unwrap();
        for l in &links {
            assert_eq!(l.state, LinkState::Ok, "{step}: {:?}", l.link);
        }
        let store = e.store().unwrap();
        for f in store.vault_files().unwrap() {
            let p = store.vault_root().join(f.vault_rel_path());
            assert!(std::fs::symlink_metadata(&p).map(|m| m.is_file()).unwrap_or(false), "{step}: {p:?} missing");
            for alias in &f.aliases {
                let a = store.vault_root().join(&f.category).join(alias);
                assert!(std::fs::read(&a).is_ok(), "{step}: the vault name {alias} loads nothing");
            }
        }
        let info = e.vault_info().unwrap();
        let contents = e.contents(0, 1000, &Default::default(), crate::vault::VaultSort::Name, false).unwrap();
        assert!(contents.rows.len() as u64 >= info.file_count, "{step}: the Library shows fewer rows than the vault holds");
        Seen {
            vault_files: info.file_count,
            links: links.len() as u64,
            name_groups: e.name_groups().unwrap().len(),
            installs,
        }
    }

    /// Looks, opens the vault again, and looks once more.
    fn look_and_reopen(&self, step: &str) -> Seen {
        let before = self.look(step);
        self.engine.close_vault().unwrap();
        self.engine.select_vault(&self.vault, false).unwrap();
        let after = self.look(&format!("{step}, after reopening"));
        assert_eq!(before, after, "{step}: reopening the vault changed what the person sees");
        before
    }

    fn apply_everything(&self) -> String {
        let scan = self.engine.last_scan().unwrap().expect("a scan");
        let plan = self.engine.build_plan(&scan.scan_id).unwrap();
        let (tx, rx) = mpsc::channel();
        let id = self
            .engine
            .start_apply(
                ApplyRequest {
                    plan_id: plan.plan_id.clone(),
                    group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
                    verify: crate::apply::VerifyModeArg::SizeAndMtime,
                    stop_on_error: false,
                },
                Arc::new(NullSink),
                Arc::new(move |r| tx.send(r.map(|r| (r.state, r.failures.len()))).unwrap()),
            )
            .unwrap();
        let (state, failures) = rx.recv_timeout(Duration::from_secs(120)).unwrap().unwrap();
        assert_eq!(state, crate::store::ApplyState::Completed);
        assert_eq!(failures, 0);
        id
    }

    fn scan(&self) {
        let store = self.engine.store().unwrap();
        let installs = self.engine.installs().unwrap();
        let rec = self
            .engine
            .run_scan(&store, &uuid::Uuid::new_v4().to_string(), &installs, &CancelToken::new(), &NullSink)
            .unwrap();
        assert!(!rec.cancelled);
    }
}

fn row(name: &str, what: &'static str) -> (String, &'static str) {
    (name.to_string(), what)
}

#[test]
#[ignore]
fn a_whole_session_leaves_every_install_working_at_every_step() {
    let s = Session::start();
    let e = &s.engine;
    let style = weights_hash("style");

    // 1. Two installs, B's loras on another drive, and a scan.
    s.scan();
    let original = s.look_and_reopen("after the scan");
    assert_eq!(original.vault_files, 0);
    assert_eq!(original.links, 0);
    let expected_files = vec![
        row("ComfyUI-A/models/checkpoints/base.safetensors", "base"),
        row("ComfyUI-A/models/loras/style.safetensors", "style"),
        row("ComfyUI-A/models/vae/only.safetensors", "vae"),
        row("ComfyUI-B/models/checkpoints/base.safetensors", "base"),
        row("ComfyUI-B/models/loras/style-v2.safetensors", "style"),
    ];
    assert_eq!(original.installs, expected_files);

    // 2. Consolidate. The same paths load the same models, now through links.
    let apply_id = s.apply_everything();
    let seen = s.look_and_reopen("after consolidating");
    assert_eq!(seen.installs, expected_files);
    assert_eq!(seen.vault_files, 3);
    assert_eq!(seen.links, 5);
    assert_eq!(seen.name_groups, 1, "style is called by two names");
    assert!(s.d_loras.join("style-v2.safetensors").is_symlink(), "B's loras on the other drive were consolidated");

    // 3. A download, linked into both installs in a folder of its own.
    let reading = e.read_model_address(TINY, None, None, Some("loras")).unwrap();
    let plan = reading.plan.expect("Hugging Face answered");
    assert!(plan.size_bytes < 5_000_000);
    let links = vec![
        LinkChoice { install_id: s.a.id.clone(), dir: s.a.root.join("models").join("loras").join("tiny") },
        LinkChoice { install_id: s.b.id.clone(), dir: s.b.root.join("models").join("loras").join("tiny") },
    ];
    let d = e
        .start_download(&StartDownload {
            address: TINY.into(),
            version_id: None,
            file_id: None,
            category: "loras".into(),
            install_ids: Vec::new(),
            links: Some(links),
        })
        .unwrap();
    let until = Instant::now() + Duration::from_secs(180);
    let d = loop {
        let now = e.list_downloads().unwrap().into_iter().find(|x| x.download_id == d.download_id).unwrap();
        if now.state == DownloadState::Done || now.state == DownloadState::Failed || now.state == DownloadState::Mismatch {
            break now;
        }
        assert!(Instant::now() < until, "the download never finished: {:?}", now.state);
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    let mut with_tiny = expected_files.clone();
    with_tiny.push(row("ComfyUI-A/models/loras/tiny/model.safetensors", "tiny"));
    with_tiny.push(row("ComfyUI-B/models/loras/tiny/model.safetensors", "tiny"));
    with_tiny.sort();
    let seen = s.look_and_reopen("after the download");
    assert_eq!(seen.installs, with_tiny);
    assert_eq!(seen.vault_files, 4);
    assert_eq!(seen.links, 7);
    assert!(s.d_loras.join("tiny").join("model.safetensors").is_symlink(), "B's link sits on the other drive");

    // 4. One name for style everywhere, then undo it.
    let unify_plan = e.plan_unify_name(&style, "style.safetensors").unwrap();
    assert!(unify_plan.running.is_empty());
    let done = e.unify_name(&style, "style.safetensors").unwrap();
    assert_eq!(done.renamed.len(), 1, "{done:?}");
    assert!(done.skipped.is_empty() && done.stopped.is_none(), "{done:?}");
    let mut unified: Vec<(String, &str)> = with_tiny
        .iter()
        .map(|(p, w)| (p.replace("style-v2", "style"), *w))
        .collect();
    unified.sort();
    let seen = s.look_and_reopen("after one name everywhere");
    assert_eq!(seen.installs, unified);
    assert_eq!(seen.name_groups, 0);
    assert_eq!(seen.links, 7);

    e.undo_unify_name(&done.unify_id).unwrap();
    let seen = s.look_and_reopen("after undoing the name");
    assert_eq!(seen.installs, with_tiny);
    assert_eq!(seen.name_groups, 1);

    // 5. Delete the downloaded model with its links.
    let tiny = e.store().unwrap().vault_files().unwrap().into_iter().find(|f| f.sha256 == TINY_SHA).unwrap();
    let deleted = e.delete_vault_file_and_links(&tiny.sha256, &tiny.sha256).unwrap();
    assert_eq!(deleted.links_removed.len(), 2);
    let seen = s.look_and_reopen("after the delete");
    assert_eq!(seen.installs, expected_files);
    assert_eq!(seen.vault_files, 3);
    assert_eq!(seen.links, 5);

    // 6. Undo the consolidation: every install as it was at the start.
    let (tx, rx) = mpsc::channel();
    e.start_revert(apply_id, Arc::new(NullSink), Arc::new(move |r| tx.send(r.map(|r| r.state)).unwrap()))
        .unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(120)).unwrap().unwrap(), crate::store::ApplyState::Reverted);
    let seen = s.look_and_reopen("after undoing the consolidation");
    assert_eq!(seen, original, "every install is as it was, and the vault is empty");
    for p in s.model_paths() {
        assert!(!p.is_symlink(), "{p:?} is a real file again");
    }
    let left: Vec<PathBuf> = walkdir::WalkDir::new(&s.vault)
        .into_iter()
        .flatten()
        .filter(|x| x.file_type().is_file() || x.path_is_symlink())
        .map(|x| x.path().to_path_buf())
        .filter(|p| !p.starts_with(s.vault.join(".comfyvault")))
        .collect();
    assert!(left.is_empty(), "the vault still holds files: {left:?}");
}
