//! The whole download, end to end, against a server on this computer that
//! plays Hugging Face, Civitai and their storage.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::*;
use crate::download::http::UreqWeb;
use crate::download::test_server::{Canned, Req, Server};
use crate::download::tokens::MemoryTokens;
use crate::install::detect;
use crate::platform::FakePlatform;

fn content(tag: &str, len: usize) -> Vec<u8> {
    let seed = tag.bytes().fold(7u8, |a, b| a.wrapping_mul(31).wrapping_add(b));
    (0..len).map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed)).collect()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02X}")).collect()
}

/// What the fake sites serve, changeable while a test runs.
#[derive(Clone)]
struct Site {
    /// The file's bytes.
    bytes: Arc<Mutex<Vec<u8>>>,
    /// The SHA-256 the site claims. By default the true one.
    claimed: Arc<Mutex<Option<String>>>,
    /// Hugging Face answers every request for the file with this, when set.
    refuse: Arc<Mutex<Option<Canned>>>,
    /// The first storage answer stalls after this many bytes.
    stall_first: Arc<Mutex<Option<usize>>>,
    storage_hits: Arc<AtomicUsize>,
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    ctx: Context,
    fake: Arc<FakePlatform>,
    server: Server,
    storage: Server,
    site: Site,
    tokens: Arc<MemoryTokens>,
    dl: Arc<Downloader>,
    seen: Arc<Mutex<Vec<Download>>>,
}

struct Seen(Arc<Mutex<Vec<Download>>>);
impl ProgressSink<Download> for Seen {
    fn emit(&self, d: &Download) {
        self.0.lock().unwrap().push(d.clone());
    }
}

const HF: &str = "https://huggingface.co/o/r/blob/main/split_files/text_encoders/t5.safetensors";
const CIVITAI: &str = "https://civitai.com/models/4384";

fn world(bytes: Vec<u8>, lfs: bool) -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = Arc::new(Store::open(&root.join("vault"), true).unwrap());
    let mut settings = store.settings().unwrap();
    settings.min_file_size_bytes = 0;
    settings.huggingface_cache_dirs = Some(Vec::new());
    store.put_settings(&settings).unwrap();

    let site = Site {
        bytes: Arc::new(Mutex::new(bytes)),
        claimed: Arc::new(Mutex::new(None)),
        refuse: Arc::new(Mutex::new(None)),
        stall_first: Arc::new(Mutex::new(None)),
        storage_hits: Arc::new(AtomicUsize::new(0)),
    };
    // Storage on a server of its own, as the real sites' storage is.
    let st = site.clone();
    let storage = Server::start(move |req: &Req| {
        let bytes = st.bytes.lock().unwrap().clone();
        let etag = format!("\"{}\"", &sha(&bytes)[..16]);
        if !req.path().starts_with("/store/") {
            return Canned::new(404);
        }
        let n = st.storage_hits.fetch_add(1, Ordering::SeqCst);
        let c = Canned::file(req, &bytes, &etag);
        match *st.stall_first.lock().unwrap() {
            Some(k) if n == 0 => Canned { stall_after: Some((k, Duration::from_secs(3))), ..c },
            _ => c,
        }
    });
    let store_base = storage.base.clone();
    let s = site.clone();
    let base = Arc::new(Mutex::new(String::new()));
    let b2 = base.clone();
    let server = Server::start(move |req: &Req| {
        let base = b2.lock().unwrap().clone();
        let bytes = s.bytes.lock().unwrap().clone();
        let claimed = s.claimed.lock().unwrap().clone().unwrap_or_else(|| sha(&bytes));
        match (req.method.as_str(), req.path()) {
            (_, "/o/r/resolve/main/split_files/text_encoders/t5.safetensors") => {
                if let Some(c) = s.refuse.lock().unwrap().clone() {
                    return c;
                }
                Canned::redirect(&format!("{store_base}/store/t5?X-Amz-Signature=sig"))
                    .with_header("x-linked-size", &bytes.len().to_string())
            }
            ("POST", "/api/models/o/r/paths-info/main") => {
                let lfs_block = if lfs {
                    format!(r#","lfs":{{"oid":"{}","size":{}}}"#, claimed.to_lowercase(), bytes.len())
                } else {
                    String::new()
                };
                Canned::json(
                    200,
                    &format!(r#"[{{"type":"file","path":"split_files/text_encoders/t5.safetensors","size":{}{lfs_block}}}]"#, bytes.len()),
                )
            }
            (_, "/api/v1/models/4384") => Canned::json(
                200,
                &format!(
                    r#"{{"name":"DreamShaper","type":"Checkpoint","modelVersions":[{{"id":8,"name":"8","files":[{{"id":1,"name":"dreamshaper_8.safetensors","sizeKB":{},"primary":true,"hashes":{{"SHA256":"{claimed}"}},"downloadUrl":"{base}/api/download/models/8"}}]}}]}}"#,
                    bytes.len() as f64 / 1024.0
                ),
            ),
            (_, "/api/download/models/8") => Canned::new(307).with_header("location", &format!("{store_base}/store/ds8?sig=1")),
            _ => Canned::new(404),
        }
    });
    *base.lock().unwrap() = server.base.clone();

    let tokens = Arc::new(MemoryTokens::default());
    let sites = Sites { hugging_face: server.base.clone(), civitai: server.base.clone() };
    let dl = Arc::new(Downloader::new(Arc::new(UreqWeb::with_idle(Duration::from_secs(1))), sites, tokens.clone()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    dl.set_sink(Arc::new(Seen(seen.clone())));
    let fake = Arc::new(FakePlatform::new());
    let ctx = Context { store, platform: fake.clone(), vault_writes: Arc::new(Mutex::new(())) };
    World { _dir: dir, root, ctx, fake, server, storage, site, tokens, dl, seen }
}

impl World {
    fn install(&self, label: &str) -> Install {
        let root = self.root.join(label);
        detect::fixtures::make_install(&root);
        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate(format!("id-{label}"), label.into(), root, &c).unwrap();
        self.ctx.store.put_install(&i).unwrap();
        i
    }

    fn read(&self, address: &str, category: Option<&str>) -> AddressReading {
        self.dl.read_address(&self.ctx, address, None, None, category).unwrap()
    }

    fn start(&self, address: &str, category: &str, installs: &[&Install]) -> Download {
        self.dl
            .start(
                &self.ctx,
                &StartDownload {
                    address: address.into(),
                    version_id: None,
                    file_id: None,
                    category: category.into(),
                    install_ids: installs.iter().map(|i| i.id.clone()).collect(),
                },
            )
            .unwrap()
    }

    /// Waits until the download reaches a state that is not moving.
    fn settle(&self, id: &str) -> DownloadRecord {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let d = self.ctx.store.download(id).unwrap().unwrap();
            if !d.state.is_active() && !self.dl.is_running(id) {
                return d;
            }
            assert!(Instant::now() < until, "the download never settled: {:?}", d.state);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Every request the site or its storage received.
    fn requests(&self) -> Vec<Req> {
        let mut all = self.server.requests();
        all.extend(self.storage.requests());
        all
    }

    fn vault(&self, rel: &str) -> PathBuf {
        self.ctx.store.vault_root().join(rel)
    }
}

/// A path as the engine shows it, compared the way Windows compares paths.
fn same(shown: Option<&str>, expected: &Path) -> bool {
    shown.map(|s| crate::paths::compare_key(Path::new(s)) == crate::paths::compare_key(expected)).unwrap_or(false)
}

fn read_link(p: &Path) -> Vec<u8> {
    assert!(std::fs::symlink_metadata(p).unwrap().file_type().is_symlink(), "{p:?} is not a link");
    std::fs::read(p).unwrap()
}

// --- reading --------------------------------------------------------------

#[test]
fn a_hugging_face_address_reads_into_a_plan_with_every_install() {
    let w = world(content("t5", 40_000), true);
    let a = w.install("A");
    let b = w.install("B");
    let r = w.read(HF, None);
    assert_eq!(r.refusal, None);
    let p = r.plan.unwrap();
    assert_eq!(p.file_name, "t5.safetensors");
    assert_eq!(p.category.as_deref(), Some("text_encoders"), "suggested from the repository folder");
    assert!(same(p.vault_rel_path.as_deref(), Path::new("text_encoders/t5.safetensors")), "{:?}", p.vault_rel_path);
    assert_eq!(p.sha256.as_deref(), Some(sha(&content("t5", 40_000)).as_str()));
    assert_eq!(p.space_needed_bytes, 40_000 + SPACE_MARGIN);
    assert!(p.categories.contains(&"checkpoints".to_string()));
    assert_eq!(p.installs.len(), 2);
    for (t, i) in p.installs.iter().zip([&a, &b]) {
        assert_eq!(t.state, InstallTargetState::Free);
        assert!(t.ticked, "every install is ticked the first time");
        assert!(same(t.link_path.as_deref(), &i.root.join("models/text_encoders/t5.safetensors")), "{:?}", t.link_path);
    }
}

#[test]
fn a_file_with_no_folder_hint_has_no_paths_until_one_is_chosen() {
    let w = world(content("x", 100), true);
    w.install("A");
    // A path with no category folder in it.
    *w.site.refuse.lock().unwrap() = None;
    let r = w.dl.read_address(&w.ctx, "https://huggingface.co/o/r/blob/main/split_files/text_encoders/t5.safetensors", None, None, Some("loras")).unwrap();
    let p = r.plan.unwrap();
    assert_eq!(p.category.as_deref(), Some("loras"), "the chosen folder wins over the suggestion");
    assert!(p.installs[0].link_path.as_deref().unwrap().contains("loras"));
}

#[test]
fn an_address_that_is_not_a_model_file_is_a_refusal_without_a_request() {
    let w = world(content("x", 10), true);
    let r = w.read("https://drive.google.com/file/d/1/view", None);
    assert_eq!(r.refusal.unwrap().kind, RefusalKind::BadAddress);
    let r = w.read("https://huggingface.co/o/r", None);
    assert_eq!(r.refusal.unwrap().kind, RefusalKind::HfRepoNotFile);
    assert!(w.requests().is_empty());
}

#[test]
fn an_install_with_a_different_file_of_that_name_cannot_be_ticked_and_is_never_touched() {
    let w = world(content("t5", 1000), true);
    let a = w.install("A");
    let b = w.install("B");
    let theirs = b.root.join("models/text_encoders/t5.safetensors");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"somebody's other t5").unwrap();

    let p = w.read(HF, None).plan.unwrap();
    let tb = p.installs.iter().find(|t| t.install_id == b.id).unwrap();
    assert_eq!(tb.state, InstallTargetState::NameTaken);
    assert!(!tb.ticked);

    // Ticked anyway, the install is left alone and named.
    let d = w.start(HF, "text_encoders", &[&a, &b]);
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Done);
    assert_eq!(d.linked_install_ids, vec![a.id.clone()]);
    assert_eq!(d.not_linked.len(), 1);
    assert_eq!(std::fs::read(&theirs).unwrap(), b"somebody's other t5");
}

#[test]
fn a_same_name_file_in_the_old_folder_name_also_counts_as_taken() {
    // ComfyUI searches models/clip for text encoders too. A file there with
    // this name would hide the new one, or be hidden by it.
    let w = world(content("t5", 1000), true);
    let a = w.install("A");
    let old = a.root.join("models/clip/t5.safetensors");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, b"another").unwrap();
    let p = w.read(HF, None).plan.unwrap();
    assert_eq!(p.installs[0].state, InstallTargetState::NameTaken);
}

#[test]
fn the_link_goes_where_the_yaml_tells_comfyui_to_save() {
    let w = world(content("ckpt", 2000), true);
    let a = w.install("A");
    let shared = w.root.join("shared/text_encoders");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(
        a.root.join("extra_model_paths.yaml"),
        format!("shared:\n    base_path: {}\n    is_default: true\n    text_encoders: text_encoders\n", w.root.join("shared").display()),
    )
    .unwrap();
    let c = detect::inspect(&a.root).unwrap();
    let a = Install::from_candidate(a.id.clone(), "A".into(), a.root.clone(), &c).unwrap();
    w.ctx.store.put_install(&a).unwrap();

    let p = w.read(HF, None).plan.unwrap();
    assert!(same(p.installs[0].link_path.as_deref(), &shared.join("t5.safetensors")), "{:?}", p.installs[0].link_path);
    let d = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert_eq!(read_link(&shared.join("t5.safetensors")), content("ckpt", 2000));
}

#[test]
fn the_installs_ticked_last_time_are_ticked_next_time() {
    let w = world(content("t5", 100), true);
    let a = w.install("A");
    let b = w.install("B");
    w.settle(&w.start(HF, "text_encoders", &[&b]).download_id);
    *w.site.bytes.lock().unwrap() = content("other", 100);
    let p = w.read(CIVITAI, None).plan.unwrap();
    let ticked: Vec<&str> = p.installs.iter().filter(|t| t.ticked).map(|t| t.install_id.as_str()).collect();
    assert_eq!(ticked, vec![b.id.as_str()]);
    let _ = a;
}

// --- a whole download -----------------------------------------------------

#[test]
fn a_hugging_face_file_downloads_checks_goes_into_the_vault_and_is_linked() {
    let bytes = content("t5", 300_000);
    let w = world(bytes.clone(), true);
    let a = w.install("A");
    let b = w.install("B");
    let d = w.start(HF, "text_encoders", &[&a, &b]);
    assert_eq!(d.state, DownloadState::Waiting);
    let d = w.settle(&d.download_id);

    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert!(!d.already_in_vault);
    assert_eq!(d.sha256.as_deref(), Some(sha(&bytes).as_str()), "the proven hash, for Show it in Library");
    assert_eq!(d.bytes_done, 300_000);
    let file = w.vault("text_encoders/t5.safetensors");
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    let rec = w.ctx.store.vault_file(&sha(&bytes)).unwrap().expect("a vault record");
    assert_eq!(rec.category, "text_encoders");
    for i in [&a, &b] {
        assert_eq!(read_link(&i.root.join("models/text_encoders/t5.safetensors")), bytes);
    }
    assert_eq!(w.ctx.store.links_for_hash(&sha(&bytes)).unwrap().len(), 2);
    assert!(!part_path(&w.ctx, &d.download_id).exists(), "the part is gone once it is in the vault");

    // Every step went into the journal first, and is done.
    let j = w.ctx.store.journal(&format!("{JOURNAL_PREFIX}{}", d.download_id)).unwrap();
    assert_eq!(j.len(), 3, "one move and two links");
    assert!(j.iter().all(|e| e.state == JournalState::Done));
    assert!(matches!(j[0].step, JournalStep::MoveToVault { .. }));

    // The list says each state it went through, in order.
    let states: Vec<DownloadState> = w.seen.lock().unwrap().iter().map(|x| x.state).collect();
    let mut order = states.clone();
    order.dedup();
    assert_eq!(order, vec![DownloadState::Waiting, DownloadState::Running, DownloadState::Checking, DownloadState::Done]);
}

#[test]
fn a_civitai_model_downloads_against_the_hash_civitai_gave() {
    let bytes = content("ds8", 50_000);
    let w = world(bytes.clone(), true);
    let a = w.install("A");
    let p = w.read(CIVITAI, None).plan.unwrap();
    assert_eq!(p.category.as_deref(), Some("checkpoints"));
    let d = w.settle(&w.start(CIVITAI, "checkpoints", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert_eq!(read_link(&a.root.join("models/checkpoints/dreamshaper_8.safetensors")), bytes);
}

#[test]
fn a_file_that_does_not_match_its_hash_is_deleted_and_nothing_is_linked() {
    let bytes = content("real", 20_000);
    let w = world(bytes.clone(), true);
    *w.site.claimed.lock().unwrap() = Some(sha(b"what the site said it was"));
    let a = w.install("A");
    let d = w.settle(&w.start(CIVITAI, "checkpoints", &[&a]).download_id);

    assert_eq!(d.state, DownloadState::Mismatch);
    assert_eq!(d.sha256, None, "nothing was proven");
    assert_eq!(d.error.as_ref().unwrap().kind, FailureKind::Mismatch);
    assert!(!part_path(&w.ctx, &d.download_id).exists(), "the wrong file is deleted");
    assert!(w.ctx.store.vault_files().unwrap().is_empty(), "nothing went into the vault");
    assert!(std::fs::symlink_metadata(a.root.join("models/checkpoints/dreamshaper_8.safetensors")).is_err());
    assert!(!w.vault("checkpoints").join("dreamshaper_8.safetensors").exists());
}

#[test]
fn a_model_already_in_the_vault_is_only_linked() {
    let bytes = content("have", 5_000);
    let w = world(bytes.clone(), true);
    let a = w.install("A");
    let b = w.install("B");
    w.settle(&w.start(CIVITAI, "checkpoints", &[&a]).download_id);
    let hits = w.site.storage_hits.load(Ordering::SeqCst);

    let p = w.read(CIVITAI, None).plan.unwrap();
    assert!(p.already_in_vault.is_some());
    assert_eq!(p.space_needed_bytes, 0);
    let ta = p.installs.iter().find(|t| t.install_id == a.id).unwrap();
    assert_eq!(ta.state, InstallTargetState::HasLink);

    let d = w.start(CIVITAI, "checkpoints", &[&a, &b]);
    assert_eq!(d.state, DownloadState::LinkedOnly);
    assert_eq!(d.sha256.as_deref(), Some(sha(&bytes).as_str()));
    assert!(d.already_in_vault);
    assert_eq!(w.site.storage_hits.load(Ordering::SeqCst), hits, "nothing was downloaded");
    assert_eq!(read_link(&b.root.join("models/checkpoints/dreamshaper_8.safetensors")), bytes);
    assert_eq!(d.linked_install_ids.len(), 2);
}

#[test]
fn a_model_the_vault_holds_is_planned_in_the_folder_it_is_in() {
    // The repository path suggests text_encoders, but the vault keeps this
    // model under loras. Its links come from there.
    let w = world(content("held", 2_000), true);
    let a = w.install("A");
    w.settle(&w.start(HF, "loras", &[&a]).download_id);
    let p = w.read(HF, None).plan.unwrap();
    assert!(p.already_in_vault.is_some());
    assert_eq!(p.category.as_deref(), Some("loras"));
    assert_eq!(p.suggested_category.as_deref(), Some("text_encoders"));
}

#[test]
fn a_file_with_no_hash_that_turns_out_to_be_in_the_vault_is_deleted_after_the_download() {
    let bytes = content("small", 3_000);
    let w = world(bytes.clone(), false);
    let a = w.install("A");
    let b = w.install("B");
    w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(w.ctx.store.vault_files().unwrap().len(), 1);

    let d = w.settle(&w.start(HF, "text_encoders", &[&b]).download_id);
    assert_eq!(d.state, DownloadState::Done);
    assert!(d.already_in_vault, "found in the vault only after the download");
    assert_eq!(w.ctx.store.vault_files().unwrap().len(), 1, "no second copy");
    assert_eq!(read_link(&b.root.join("models/text_encoders/t5.safetensors")), bytes);

    // A different small file under the same name: no hash to tag with yet,
    // so the plan shows the plain name and says it is taken.
    *w.site.bytes.lock().unwrap() = content("other small", 3_000);
    let p = w.read(HF, None).plan.unwrap();
    assert!(same(p.vault_rel_path.as_deref(), Path::new("text_encoders/t5.safetensors")), "{:?}", p.vault_rel_path);
    assert!(p.vault_name_taken);
    let d = w.settle(&w.start(HF, "text_encoders", &[]).download_id);
    let tag = &sha(&content("other small", 3_000))[..8];
    assert!(d.vault_rel_path.ends_with(&format!("t5__{tag}.safetensors")), "{}", d.vault_rel_path);
    assert_eq!(w.ctx.store.vault_files().unwrap().len(), 2);
}

#[test]
fn a_name_another_model_has_in_the_vault_gets_the_hash_tag() {
    let w = world(content("one", 1_000), true);
    let a = w.install("A");
    w.settle(&w.start(HF, "text_encoders", &[]).download_id);
    let second = content("two", 1_000);
    *w.site.bytes.lock().unwrap() = second.clone();
    let p = w.read(HF, None).plan.unwrap();
    let tag = &sha(&second)[..8];
    assert!(p.vault_rel_path.as_deref().unwrap().ends_with(&format!("t5__{tag}.safetensors")));
    assert!(p.vault_name_taken);
    let d = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Done);
    assert_eq!(std::fs::read(w.vault(&format!("text_encoders/t5__{tag}.safetensors"))).unwrap(), second);
    // The link keeps the name the person knows.
    assert_eq!(read_link(&a.root.join("models/text_encoders/t5.safetensors")), second);
}

#[test]
fn a_download_that_needs_more_room_than_the_drive_has_is_refused() {
    let w = world(content("big", 1_000), true);
    w.fake.set_free_bytes(SPACE_MARGIN + 500);
    let a = w.install("A");
    let p = w.read(HF, None).plan.unwrap();
    assert_eq!(p.vault_free_bytes, Some(SPACE_MARGIN + 500));
    assert!(p.space_needed_bytes > p.vault_free_bytes.unwrap());
    let err = w
        .dl
        .start(
            &w.ctx,
            &StartDownload {
                address: HF.into(),
                version_id: None,
                file_id: None,
                category: "text_encoders".into(),
                install_ids: vec![a.id.clone()],
            },
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::IoError);
    assert!(w.ctx.store.downloads().unwrap().is_empty());
}

// --- stopping, failing, continuing ----------------------------------------

#[test]
fn stop_then_continue_carries_on_from_the_kept_part() {
    let bytes = content("long", 400_000);
    let w = world(bytes.clone(), true);
    *w.site.stall_first.lock().unwrap() = Some(150_000);
    let a = w.install("A");
    let d = w.start(HF, "text_encoders", &[&a]);

    // Wait for the first bytes, then stop.
    let until = Instant::now() + Duration::from_secs(10);
    while part_len(&w.ctx, &d.download_id) < 150_000 {
        assert!(Instant::now() < until, "no bytes arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped = w.dl.stop(&w.ctx, &d.download_id).unwrap();
    assert_eq!(stopped.state, DownloadState::Stopped);
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Stopped, "stopped wins over the dropped line");
    assert_eq!(part_len(&w.ctx, &d.download_id), 150_000, "the part is kept");

    w.dl.resume(&w.ctx, &d.download_id).unwrap();
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert_eq!(std::fs::read(w.vault("text_encoders/t5.safetensors")).unwrap(), bytes, "the SHA-256 matched");
    let storage: Vec<Req> = w.requests().into_iter().filter(|r| r.path().starts_with("/store/")).collect();
    assert_eq!(storage.last().unwrap().header("range"), Some("bytes=150000-"));
    // Continue asked the site again, never the old storage address alone.
    let site_asks = w.requests().iter().filter(|r| r.method == "GET" && r.path().starts_with("/o/r/resolve")).count();
    assert_eq!(site_asks, 2);
}

#[test]
fn a_dropped_line_fails_with_the_part_kept_and_no_retry() {
    let bytes = content("drop", 200_000);
    let w = world(bytes, true);
    *w.site.stall_first.lock().unwrap() = Some(60_000);
    let a = w.install("A");
    let d = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Failed);
    assert_eq!(d.error.as_ref().unwrap().kind, FailureKind::Connection);
    assert_eq!(d.bytes_done, 60_000);
    assert_eq!(w.site.storage_hits.load(Ordering::SeqCst), 1, "it did not try again by itself");
    assert!(d.part_version.is_some(), "the kept part names its version, so it can continue");
}

#[test]
fn a_refusal_part_way_comes_back_in_the_sites_words() {
    let w = world(content("gated", 100_000), true);
    *w.site.stall_first.lock().unwrap() = Some(10);
    let a = w.install("A");
    let d = w.start(HF, "text_encoders", &[&a]);
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Failed, "the first attempt dropped");
    *w.site.refuse.lock().unwrap() = Some(
        Canned::new(401)
            .with_header("x-error-code", "GatedRepo")
            .with_header("x-error-message", "Access to model o/r is restricted."),
    );
    std::fs::write(part_path(&w.ctx, &d.download_id), b"").ok();
    w.dl.resume(&w.ctx, &d.download_id).unwrap();
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Failed);
    let e = d.error.clone().unwrap();
    assert_eq!(e.kind, FailureKind::Refused);
    assert_eq!(e.service_message.as_deref(), Some("Access to model o/r is restricted."));
}

#[test]
fn a_token_goes_to_the_site_only() {
    let w = world(content("t", 1_000), true);
    w.tokens.set(Host::HuggingFace, "hf_secret").unwrap();
    let a = w.install("A");
    let d = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Done);
    for r in w.requests() {
        let site = !r.path().starts_with("/store/");
        assert_eq!(r.header("authorization").is_some(), site, "{} {}", r.method, r.target);
        assert!(!r.target.contains("hf_secret"));
    }
    let listed = serde_json::to_string(&w.dl.list(&w.ctx).unwrap()).unwrap();
    assert!(!listed.contains("hf_secret"));
}

#[test]
fn a_transfer_the_app_closed_on_is_cut_off_and_continues_from_its_part() {
    let bytes = content("cut", 100_000);
    let w = world(bytes.clone(), true);
    *w.site.stall_first.lock().unwrap() = Some(10);
    let a = w.install("A");
    // What a closed app leaves: a running row and a part, with the version
    // the part came from.
    let d = w.start(HF, "text_encoders", &[&a]);
    let mut d = w.settle(&d.download_id);
    std::fs::write(part_path(&w.ctx, &d.download_id), &bytes[..30_000]).unwrap();
    d.part_version = Some(format!("\"{}\"", &sha(&bytes)[..16]));
    d.state = DownloadState::Running;
    w.ctx.store.put_download(&d).unwrap();

    w.dl.on_open(&w.ctx).unwrap();
    let d = w.ctx.store.download(&d.download_id).unwrap().unwrap();
    assert_eq!(d.state, DownloadState::CutOff);
    assert_eq!(d.bytes_done, 30_000);

    w.dl.resume(&w.ctx, &d.download_id).unwrap();
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert_eq!(std::fs::read(w.vault("text_encoders/t5.safetensors")).unwrap(), bytes);
    let last = w.requests().into_iter().filter(|r| r.path().starts_with("/store/")).last().unwrap();
    assert_eq!(last.header("range"), Some("bytes=30000-"));
}

#[test]
fn a_crash_after_the_file_went_into_the_vault_is_finished_on_continue() {
    let bytes = content("moved", 8_000);
    let w = world(bytes.clone(), true);
    *w.site.stall_first.lock().unwrap() = Some(10);
    let a = w.install("A");
    let d = w.start(HF, "text_encoders", &[&a]);
    let mut d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Failed);

    // The move happened and was journaled; the record and links did not.
    let dest = w.vault("text_encoders/t5.safetensors");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, &bytes).unwrap();
    let _ = std::fs::remove_file(part_path(&w.ctx, &d.download_id));
    w.ctx
        .store
        .append_journal(&JournalEntry {
            apply_id: d.journal_id(),
            seq: 0,
            group_id: d.download_id.clone(),
            step: JournalStep::MoveToVault {
                from: part_path(&w.ctx, &d.download_id),
                to: dest.clone(),
                copied: false,
                sha256: sha(&bytes),
                size_bytes: bytes.len() as u64,
            },
            state: JournalState::Pending,
            started_at: Timestamp::now(),
            finished_at: None,
            error: None,
        })
        .unwrap();
    d.state = DownloadState::Checking;
    w.ctx.store.put_download(&d).unwrap();
    let hits = w.site.storage_hits.load(Ordering::SeqCst);

    w.dl.on_open(&w.ctx).unwrap();
    w.dl.resume(&w.ctx, &d.download_id).unwrap();
    let d = w.settle(&d.download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert!(w.ctx.store.vault_file(&sha(&bytes)).unwrap().is_some(), "the record was written");
    assert_eq!(read_link(&a.root.join("models/text_encoders/t5.safetensors")), bytes);
    assert_eq!(w.site.storage_hits.load(Ordering::SeqCst), hits, "nothing was downloaded again");
}

#[test]
fn a_crash_between_links_is_finished_without_a_second_link() {
    let bytes = content("half", 4_000);
    let w = world(bytes.clone(), true);
    let a = w.install("A");
    let b = w.install("B");
    let d = w.settle(&w.start(HF, "text_encoders", &[&a, &b]).download_id);
    // Take the second link's record away, as if the crash came between the
    // link and its record, and run the links again.
    let lb = b.root.join("models/text_encoders/t5.safetensors");
    let rec = w.ctx.store.link_at_path(&lb).unwrap().unwrap();
    w.ctx.store.delete_link(&rec.id).unwrap();
    let mut d = d;
    let record = w.ctx.store.vault_file(&sha(&bytes)).unwrap().unwrap();
    w.dl.link_all(&w.ctx, &mut d, &record).unwrap();
    assert_eq!(d.linked_install_ids.len(), 2);
    assert!(d.not_linked.is_empty(), "{:?}", d.not_linked);
    assert!(w.ctx.store.link_at_path(&lb).unwrap().is_some(), "the record is back");
    assert_eq!(w.ctx.store.links_for_hash(&sha(&bytes)).unwrap().len(), 2);
}

#[test]
fn discard_deletes_the_part_and_remove_refuses_while_there_is_one() {
    let w = world(content("d", 200_000), true);
    *w.site.stall_first.lock().unwrap() = Some(50_000);
    let a = w.install("A");
    let d = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Failed);
    let part = part_path(&w.ctx, &d.download_id);
    assert!(part.exists());

    assert_eq!(w.dl.remove(&w.ctx, &d.download_id).unwrap_err().code, ErrorCode::Conflict);
    w.dl.discard(&w.ctx, &d.download_id).unwrap();
    assert!(!part.exists());
    assert!(w.ctx.store.download(&d.download_id).unwrap().is_none());
}

#[test]
fn one_transfer_runs_at_a_time_in_the_order_they_were_started() {
    let w = world(content("q", 150_000), true);
    *w.site.stall_first.lock().unwrap() = Some(100);
    let a = w.install("A");
    let first = w.start(HF, "text_encoders", &[&a]);
    let second = w.start(CIVITAI, "checkpoints", &[&a]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(w.ctx.store.download(&second.download_id).unwrap().unwrap().state, DownloadState::Waiting);
    w.settle(&first.download_id);
    let s = w.settle(&second.download_id);
    assert_eq!(s.state, DownloadState::Done, "{:?}", s.error);
}

#[test]
fn closing_the_vault_stops_the_transfer_and_keeps_the_part() {
    let w = world(content("c", 300_000), true);
    *w.site.stall_first.lock().unwrap() = Some(100_000);
    let a = w.install("A");
    let d = w.start(HF, "text_encoders", &[&a]);
    let until = Instant::now() + Duration::from_secs(10);
    while part_len(&w.ctx, &d.download_id) < 100_000 {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    w.dl.close(Some(&w.ctx));
    let d = w.ctx.store.download(&d.download_id).unwrap().unwrap();
    assert_eq!(d.state, DownloadState::Stopped);
    assert!(!w.dl.is_running(&d.download_id));
    assert!(part_path(&w.ctx, &d.download_id).exists());
}

#[test]
fn a_finished_download_is_gone_from_the_list_after_a_restart_and_an_unfinished_one_stays() {
    let w = world(content("r", 1_000), true);
    let a = w.install("A");
    let done = w.settle(&w.start(HF, "text_encoders", &[&a]).download_id);
    let mut stopped = done.clone();
    let kept = uuid::Uuid::new_v4().to_string();
    stopped.download_id = kept.clone();
    stopped.state = DownloadState::Stopped;
    w.ctx.store.put_download(&stopped).unwrap();
    w.dl.on_open(&w.ctx).unwrap();
    let ids: Vec<String> = w.ctx.store.downloads().unwrap().into_iter().map(|d| d.download_id.clone()).collect();
    assert_eq!(ids, vec![kept]);
}

#[test]
fn a_token_is_saved_only_when_the_site_accepts_it() {
    let w = world(content("x", 10), true);
    let s = Server::start(|req| match (req.path(), req.header("authorization")) {
        ("/api/whoami-v2", Some("Bearer good")) => Canned::json(200, r#"{"name":"someone"}"#),
        _ => Canned::json(401, r#"{"error":"Invalid username or password."}"#),
    });
    let tokens = Arc::new(MemoryTokens::default());
    let dl = Downloader::new(
        Arc::new(UreqWeb::new()),
        Sites { hugging_face: s.base.clone(), civitai: s.base.clone() },
        tokens.clone(),
    );
    let e = dl.set_token(Host::HuggingFace, "bad").unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(e.detail.as_deref(), Some("Invalid username or password."));
    assert!(!format!("{e:?}").contains("bad\""));
    assert_eq!(tokens.get(Host::HuggingFace).unwrap(), None, "a refused token is not saved");

    assert_eq!(dl.set_token(Host::HuggingFace, " good ").unwrap(), Some("someone".into()));
    assert_eq!(tokens.get(Host::HuggingFace).unwrap().as_deref(), Some("good"));
    let st = dl.token_status(Host::HuggingFace).unwrap();
    assert_eq!(st, TokenStatus { saved: true, ok: Some(true), account: Some("someone".into()), message: None });
    assert!(dl.remove_token(Host::HuggingFace).unwrap());
    assert_eq!(dl.token_status(Host::HuggingFace).unwrap().saved, false);
    let _ = w;
}

#[test]
fn a_download_does_not_stand_in_the_way_of_undoing_an_earlier_run() {
    // A download's journal belongs to no run. It must only matter to an undo
    // that needs what the download touched, and a download never touches
    // what a run made.
    let w = world(content("new", 2_000), true);
    let a = w.install("A");
    let b = w.install("B");
    let pa = a.root.join("models/loras/m.safetensors");
    let pb = b.root.join("models/loras/m.safetensors");
    for p in [&pa, &pb] {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content("m", 3_000)).unwrap();
    }
    let settings = w.ctx.store.settings().unwrap();
    let installs = vec![a.clone(), b.clone()];
    let scan = crate::scan::Scanner::new(&w.ctx.store, &*w.fake, settings)
        .scan("scan-1", &installs, &CancelToken::new(), &crate::progress::NullSink)
        .unwrap();
    w.ctx.store.put_scan(&scan.record).unwrap();
    w.ctx.store.put_scan_entries(&scan.record.scan_id, &scan.entries).unwrap();
    let plan = crate::plan::Planner::new(&w.ctx.store, &*w.fake)
        .build("plan-1", &scan.record.scan_id, &scan.entries, &installs)
        .unwrap();
    w.ctx.store.put_plan(&plan).unwrap();
    crate::apply::Applier::new(&w.ctx.store, &*w.fake)
        .apply(
            "ap-1",
            &plan,
            &crate::apply::ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
                verify: crate::apply::VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &crate::progress::NullSink,
        )
        .unwrap();
    assert!(std::fs::symlink_metadata(&pa).unwrap().file_type().is_symlink());

    let d = w.settle(&w.start(HF, "text_encoders", &[&a, &b]).download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);

    crate::apply::Applier::new(&w.ctx.store, &*w.fake)
        .revert("ap-1", &CancelToken::new(), &crate::progress::NullSink)
        .expect("the earlier run is undone");
    assert!(std::fs::symlink_metadata(&pa).unwrap().file_type().is_file(), "the file is back");
    // And the download's links are untouched by that undo.
    assert_eq!(read_link(&a.root.join("models/text_encoders/t5.safetensors")), content("new", 2_000));
}

/// Every file under `dir` whose bytes contain `needle`.
fn files_containing(dir: &Path, needle: &[u8]) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| std::fs::read(e.path()).map(|b| b.windows(needle.len()).any(|w| w == needle)).unwrap_or(false))
        .map(|e| e.path().to_path_buf())
        .collect()
}

#[test]
fn a_key_pasted_in_the_address_is_never_written_to_the_vault() {
    // Civitai's own instructions give the address with the key in it.
    let w = world(content("keyed", 4_000), true);
    let a = w.install("A");
    let secret = "PASTEDKEY9f3a1c";
    let d = w.settle(&w.start(&format!("{CIVITAI}?token={secret}"), "checkpoints", &[&a]).download_id);
    assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
    assert_eq!(d.address, "https://civitai.com/models/4384");
    assert!(files_containing(&w.root.join("vault"), secret.as_bytes()).is_empty(), "the key is in the vault");
    for r in w.requests() {
        assert!(!r.target.contains(secret));
    }
}

fn plain_download() -> Download {
    Download {
        download_id: String::new(),
        host: Host::Civitai,
        title: "DreamShaper".into(),
        file_name: "dreamshaper_8.safetensors".into(),
        category: "checkpoints".into(),
        vault_rel_path: String::new(),
        install_ids: Vec::new(),
        linked_install_ids: Vec::new(),
        not_linked: Vec::new(),
        already_in_vault: false,
        sha256: None,
        state: DownloadState::Stopped,
        bytes_done: 0,
        bytes_total: 0,
        bytes_per_second: None,
        error: None,
        started_at: None,
        finished_at: None,
    }
}

#[test]
fn a_key_an_earlier_build_kept_is_gone_from_the_database_file() {
    // Two ways an earlier build left a key in the file: a row that still
    // holds it, and a finished row the list already forgot, whose bytes stay
    // in pages the database freed.
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let (kept, forgotten) = ("KEPTROWKEY7e21", "FORGOTTENKEY4b9");
    {
        let store = Store::open(&vault, true).unwrap();
        let mut rec = DownloadRecord {
            download: plain_download(),
            address: format!("https://civitai.com/api/download/models/8?token={kept}"),
            version_id: Some(8),
            file_id: None,
            expected_sha256: None,
            part_version: None,
            seq: 0,
        };
        rec.download.download_id = uuid::Uuid::new_v4().to_string();
        let mut gone = rec.clone();
        gone.download.download_id = uuid::Uuid::new_v4().to_string();
        gone.address = format!("https://civitai.com/api/download/models/9?token={forgotten}");
        store.put_download(&gone).unwrap();
        // The work a download does in between: its row saved again and
        // again as its bytes move.
        for n in 0..20u64 {
            gone.download.bytes_done = n;
            store.put_download(&gone).unwrap();
        }
        store.put_download(&rec).unwrap();
        store.delete_download(&gone.download_id).unwrap();
        // The app ended without closing the database, as a crash or a kill
        // does. A clean close gives the freed pages back; this does not.
        // Its lock stays with this process, so the file as the crash left it
        // is copied to a vault of its own.
        std::mem::forget(store);
    }
    let crashed = dir.path().join("crashed");
    std::fs::create_dir_all(crashed.join(".comfyvault")).unwrap();
    std::fs::copy(vault.join(".comfyvault/vault.redb"), crashed.join(".comfyvault/vault.redb")).unwrap();
    let vault = crashed;
    for key in [kept, forgotten] {
        assert!(!files_containing(&vault, key.as_bytes()).is_empty(), "the setup did not leave {key}");
    }

    let store = Store::open(&vault, false).unwrap();
    let rows = store.downloads().unwrap();
    assert_eq!(rows[0].address, "https://civitai.com/api/download/models/8");
    drop(store);
    for key in [kept, forgotten] {
        assert!(files_containing(&vault, key.as_bytes()).is_empty(), "{key} is still in the database file");
    }
}

#[test]
fn a_download_id_that_names_a_path_never_reaches_a_file() {
    // The vault's database is a file anyone could have prepared.
    let w = world(content("x", 10), true);
    let id = "../../../outside/victim";
    let mut rec = DownloadRecord {
        download: plain_download(),
        address: CIVITAI.into(),
        version_id: None,
        file_id: None,
        expected_sha256: None,
        part_version: None,
        seq: 0,
    };
    rec.download.download_id = id.into();
    w.ctx.store.put_download(&rec).unwrap();
    std::fs::create_dir_all(w.ctx.store.downloads_dir()).unwrap();
    let victim = w.root.join("outside/victim.part");
    std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
    std::fs::write(&victim, b"the person's own file").unwrap();

    for e in [w.dl.discard(&w.ctx, id).unwrap_err(), w.dl.resume(&w.ctx, id).unwrap_err().clone()] {
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{e:?}");
    }
    assert_eq!(std::fs::read(&victim).unwrap(), b"the person's own file");
    // And the row is gone the next time the vault opens.
    w.dl.on_open(&w.ctx).unwrap();
    assert!(w.ctx.store.download(id).unwrap().is_none());
    assert!(victim.exists());
}

/// Against the real sites. They run only when asked (`--ignored`), and each
/// one reads the address first and stops if the file is bigger than a few
/// megabytes, so a wrong address can never fill a disk.
mod real_sites {
    use super::*;

    const CEILING: u64 = 5_000_000;

    fn real() -> (tempfile::TempDir, Context, Arc<Downloader>, Install) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("vault"), true).unwrap());
        let mut settings = store.settings().unwrap();
        settings.min_file_size_bytes = 0;
        settings.huggingface_cache_dirs = Some(Vec::new());
        store.put_settings(&settings).unwrap();
        let root = dir.path().join("ComfyUI");
        detect::fixtures::make_install(&root);
        let c = detect::inspect(&root).unwrap();
        let install = Install::from_candidate("real".into(), "A".into(), root, &c).unwrap();
        store.put_install(&install).unwrap();
        let ctx = Context {
            store,
            platform: Arc::new(crate::platform::NativePlatform::new()),
            vault_writes: Arc::new(Mutex::new(())),
        };
        let dl = Arc::new(Downloader::new(Arc::new(UreqWeb::new()), Sites::default(), Arc::new(MemoryTokens::default())));
        (dir, ctx, dl, install)
    }

    fn fetch(address: &str, want_sha: Option<&str>) {
        let (_dir, ctx, dl, install) = real();
        let plan = dl.read_address(&ctx, address, None, None, None).unwrap().plan.expect("a plan");
        assert!(plan.size_bytes <= CEILING, "{} is {} bytes, over the ceiling; nothing was downloaded", address, plan.size_bytes);
        let category = plan.category.clone().unwrap_or_else(|| "loras".into());
        let d = dl
            .start(
                &ctx,
                &StartDownload { address: address.into(), version_id: None, file_id: None, category: category.clone(), install_ids: vec![install.id.clone()] },
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(120);
        let d = loop {
            let d = ctx.store.download(&d.download_id).unwrap().unwrap();
            if !d.state.is_active() && !dl.is_running(&d.download_id) {
                break d;
            }
            assert!(Instant::now() < until, "still {:?}", d.state);
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(d.state, DownloadState::Done, "{:?}", d.error);
        let rec = ctx.store.vault_files().unwrap().pop().unwrap();
        if let Some(want) = want_sha {
            assert_eq!(rec.sha256, want.to_ascii_uppercase());
        }
        let file = ctx.store.vault_root().join(rec.vault_rel_path());
        assert_eq!(sha(&std::fs::read(&file).unwrap()), rec.sha256);
        let link = folders::link_folder(&install, &category).join(&d.file_name);
        assert_eq!(read_link(&link), std::fs::read(&file).unwrap());
        println!("{address}: {} bytes, SHA-256 {}", rec.size_bytes, rec.sha256);
    }

    #[test]
    #[ignore]
    fn a_small_hugging_face_file_in_lfs_downloads_and_matches() {
        fetch(
            "https://huggingface.co/hf-internal-testing/tiny-random-gpt2/blob/main/model.safetensors",
            Some("8111d5afb0715dbf5a31396d31432cb56370ba23f6650a035ea0fc8a20b4e500"),
        );
    }

    #[test]
    #[ignore]
    fn a_small_hugging_face_file_with_no_hash_downloads() {
        fetch("https://huggingface.co/hf-internal-testing/tiny-random-bert/blob/main/model.safetensors", None);
    }

    #[test]
    #[ignore]
    fn a_small_civitai_model_downloads_and_matches() {
        fetch(
            "https://civitai.com/models/7808/easynegative",
            Some("C74B4E810B030F6B75FDE959E2DB678C268D07115B85356D3C0138BA5EB42340"),
        );
    }

    #[test]
    #[ignore]
    fn a_gated_model_without_a_token_is_refused_in_hugging_faces_words() {
        let (_dir, ctx, dl, _) = real();
        let r = dl
            .read_address(&ctx, "https://huggingface.co/black-forest-labs/FLUX.1-dev/blob/main/flux1-dev.safetensors", None, None, None)
            .unwrap();
        let refusal = r.refusal.expect("a refusal");
        assert_eq!(refusal.kind, RefusalKind::TokenMissing);
        println!("Hugging Face said: {:?}", refusal.service_message);
        assert!(refusal.service_message.unwrap_or_default().contains("restricted"));
    }

    #[test]
    #[ignore]
    fn a_made_up_token_is_refused_by_both_sites_and_not_saved() {
        let (_dir, _ctx, dl, _) = real();
        for host in [Host::HuggingFace, Host::Civitai] {
            let e = dl.set_token(host, "not_a_real_token_123").unwrap_err();
            println!("{} said: {:?}", host.name(), e.detail);
            assert_eq!(e.code, ErrorCode::Conflict, "{e:?}");
            assert!(!dl.token_status(host).unwrap().saved);
        }
    }
}
