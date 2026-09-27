//! The code review's recheck tests, kept as they were written: a crash at
//! every step of a job and of its undo, an install unplugged through both,
//! two jobs in a row past an unplugged link, and a real file taking a link's
//! place as it is repointed.

use super::*;
use crate::apply::{ApplyRequest, Applier, VerifyModeArg};
use crate::install::Install;
use crate::platform::{
    DiskSpace, DriveInfo, FakePlatform, FileIdentity, LockState, Platform, ProcessInfo, RenameError,
    SymlinkCapability, VolumeId,
};
use crate::progress::{CancelToken, NullSink};
use crate::testkit::{weights, weights_hash, TestWorld};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const OLD: &str = "my-favourite.safetensors";
const KEPT: &str = "lora1.safetensors";

type Pred = Box<dyn Fn(&Path) -> bool + Send + Sync>;
type Hook<'a> = Box<dyn FnOnce() + Send + 'a>;

/// FakePlatform that can crash (panic) at the Nth call that changes the disk
/// (create link, remove link, rename), and run one action before the first
/// create whose path matches a test.
struct Crashy<'a> {
    inner: &'a FakePlatform,
    crash_at: usize,
    calls: AtomicUsize,
    before_create: Mutex<Option<(Pred, Hook<'a>)>>,
}

impl<'a> Crashy<'a> {
    fn new(inner: &'a FakePlatform, crash_at: usize) -> Self {
        Self { inner, crash_at, calls: AtomicUsize::new(0), before_create: Mutex::new(None) }
    }
    fn tick(&self, what: &str, p: &Path) {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if n == self.crash_at {
            panic!("simulated crash at step {n}: {what} {}", p.display());
        }
    }
}

impl<'a> Platform for Crashy<'a> {
    fn create_file_symlink(&self, link: &Path, target: &Path) -> Result<()> {
        let hook = {
            let mut g = self.before_create.lock().unwrap();
            if g.as_ref().is_some_and(|(p, _)| p(link)) {
                g.take().map(|(_, h)| h)
            } else {
                None
            }
        };
        if let Some(h) = hook {
            h();
        }
        self.tick("create", link);
        self.inner.create_file_symlink(link, target)
    }
    fn remove_symlink(&self, link: &Path) -> Result<()> {
        self.tick("remove", link);
        self.inner.remove_symlink(link)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::result::Result<(), RenameError> {
        self.tick("rename", from);
        self.inner.rename(from, to)
    }
    fn read_symlink(&self, link: &Path) -> Result<PathBuf> {
        self.inner.read_symlink(link)
    }
    fn symlink_capability(&self) -> SymlinkCapability {
        self.inner.symlink_capability()
    }
    fn lock_state(&self, path: &Path) -> LockState {
        self.inner.lock_state(path)
    }
    fn volume_id(&self, path: &Path) -> Result<VolumeId> {
        self.inner.volume_id(path)
    }
    fn file_identity(&self, path: &Path) -> Option<FileIdentity> {
        self.inner.file_identity(path)
    }
    fn disk_space(&self, path: &Path) -> Result<DiskSpace> {
        self.inner.disk_space(path)
    }
    fn list_processes(&self) -> Vec<ProcessInfo> {
        self.inner.list_processes()
    }
    fn listening_ports(&self, pids: &[u32]) -> HashMap<u32, Vec<u16>> {
        self.inner.listening_ports(pids)
    }
    fn processes_holding(&self, pids: &[u32], files: &[PathBuf]) -> HashMap<u32, bool> {
        self.inner.processes_holding(pids, files)
    }
    fn long_paths_enabled(&self) -> Option<bool> {
        self.inner.long_paths_enabled()
    }
    fn drive_roots(&self) -> Vec<PathBuf> {
        self.inner.drive_roots()
    }
    fn drives(&self) -> Vec<DriveInfo> {
        self.inner.drives()
    }
}

fn sha() -> String {
    weights_hash("same")
}

fn loras(i: &Install) -> PathBuf {
    i.root.join("models").join("loras")
}

fn consolidate(w: &TestWorld, installs: &[Install]) {
    let plan = w.plan(installs);
    Applier::new(&w.store, &w.platform)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();
}

/// A: KEPT, B: OLD, C: KEPT. The vault keeps KEPT, with OLD as a second name.
/// Choosing OLD renames two install links and the vault file, and repoints
/// install links.
fn world() -> (TestWorld, Vec<Install>) {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    w.write_model(&a, &format!("models/loras/{KEPT}"), &weights("same"));
    w.write_model(&b, &format!("models/loras/{OLD}"), &weights("same"));
    w.write_model(&c, &format!("models/loras/{KEPT}"), &weights("same"));
    consolidate(&w, &[a.clone(), b.clone(), c.clone()]);
    (w, vec![a, b, c])
}

fn reopen(w: &TestWorld) {
    Links::new(&w.store, &w.platform).finish_interrupted().unwrap();
    Unify::new(&w.store, &w.platform).finish_interrupted().unwrap();
}

/// Everything that must hold between any two operations, as sentences.
fn broken(w: &TestWorld, installs: &[Install]) -> Vec<String> {
    let mut bad = Vec::new();
    let mut models = 0;
    for i in installs {
        for e in std::fs::read_dir(loras(i)).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".cvtmp") {
                bad.push(format!("a hidden leftover: {}", p.display()));
                continue;
            }
            models += 1;
            if std::fs::read(&p).map(|b| b != weights("same")).unwrap_or(true) {
                bad.push(format!("loads nothing: {}", p.display()));
            }
        }
    }
    if models < installs.len() {
        bad.push(format!("an install has no name for the model ({models} names in {} installs)", installs.len()));
    }
    let rec = w.store.vault_file(&sha()).unwrap().unwrap();
    let file = w.vault_root.join(rec.vault_rel_path());
    let real = std::fs::symlink_metadata(&file).map(|m| m.file_type().is_file()).unwrap_or(false);
    if !real {
        bad.push(format!("the record's file is not a real file: {}", file.display()));
    }
    for a in &rec.aliases {
        let p = w.vault_root.join(rec.alias_rel_path(a));
        if std::fs::read(&p).map(|b| b != weights("same")).unwrap_or(true) {
            bad.push(format!("a recorded second name loads nothing: {}", p.display()));
        }
    }
    for l in w.store.links_for_hash(&sha()).unwrap() {
        if std::fs::read(&l.abs_path).map(|b| b != weights("same")).unwrap_or(true) {
            bad.push(format!("a link record names a place that loads nothing: {}", l.abs_path.display()));
        }
        if w.vault_root.join(&l.vault_rel_path).exists() == false {
            bad.push(format!("a link record names a vault place that is gone: {}", l.vault_rel_path.display()));
        }
    }
    bad
}

// R1. A crash at EVERY disk-changing step of a unify that renames the vault
// file, then a reopen, then an undo. Nothing may be left broken at any point.
#[test]
fn r1_a_crash_at_every_step_then_reopen_then_undo_leaves_nothing_broken() {
    // How many steps a whole job takes.
    let total = {
        let (w, _) = world();
        let p = Crashy::new(&w.platform, usize::MAX);
        Unify::new(&w.store, &p).unify(&sha(), OLD).unwrap();
        p.calls.load(Ordering::SeqCst)
    };
    assert!(total >= 6, "setup: the job takes {total} steps");

    let mut failures: Vec<String> = Vec::new();
    for n in 1..=total {
        let (w, installs) = world();
        let p = Crashy::new(&w.platform, n);
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Unify::new(&w.store, &p).unify(&sha(), OLD)
        }));
        let at = match &crashed {
            Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_default(),
            Ok(_) => "no crash".into(),
        };
        reopen(&w);
        for b in broken(&w, &installs) {
            failures.push(format!("[{at}] after reopen: {b}"));
        }
        let id = w.store.journal_ids().unwrap().into_iter().find(|i| i.starts_with(UNIFY_JOURNAL_PREFIX));
        if let Some(id) = id {
            if let Err(e) = Unify::new(&w.store, &w.platform).undo(&id) {
                failures.push(format!("[{at}] undo refused: {}", e.message));
            }
            for b in broken(&w, &installs) {
                failures.push(format!("[{at}] after undo: {b}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// R2. A crash at every step of the UNDO of a finished job, then a reopen.
#[test]
fn r2_a_crash_at_every_step_of_the_undo_then_reopen_leaves_nothing_broken() {
    let total = {
        let (w, _) = world();
        let r = Unify::new(&w.store, &w.platform).unify(&sha(), OLD).unwrap();
        let p = Crashy::new(&w.platform, usize::MAX);
        Unify::new(&w.store, &p).undo(&r.unify_id).unwrap();
        p.calls.load(Ordering::SeqCst)
    };
    let mut failures: Vec<String> = Vec::new();
    for n in 1..=total {
        let (w, installs) = world();
        let r = Unify::new(&w.store, &w.platform).unify(&sha(), OLD).unwrap();
        let p = Crashy::new(&w.platform, n);
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Unify::new(&w.store, &p).undo(&r.unify_id)
        }));
        let at = match &crashed {
            Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_default(),
            Ok(_) => "no crash".into(),
        };
        reopen(&w);
        for b in broken(&w, &installs) {
            failures.push(format!("[{at}] after reopen: {b}"));
        }
        // The person tries the undo again.
        if let Err(e) = Unify::new(&w.store, &w.platform).undo(&r.unify_id) {
            failures.push(format!("[{at}] the second undo refused: {}", e.message));
        }
        for b in broken(&w, &installs) {
            failures.push(format!("[{at}] after the second undo: {b}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// R3. The hidden-name swap. While the vault rename points an install link at
// the new name, the person's own file takes that link's place. The hidden
// link is then renamed over the path, and a rename replaces a file.
#[test]
fn r3_the_hidden_link_is_never_renamed_over_a_real_file() {
    let (w, installs) = world();
    let c_link = loras(&installs[2]).join(OLD);
    let p = Crashy::new(&w.platform, usize::MAX);
    let target = c_link.clone();
    let c_dir = loras(&installs[2]);
    *p.before_create.lock().unwrap() = Some((
        Box::new(move |l: &Path| l.to_string_lossy().ends_with(".cvtmp") && l.parent() == Some(c_dir.as_path())),
        Box::new(move || {
            // Whichever install link is being repointed first, C's place gets
            // the person's file. C's link was renamed to OLD by the job.
            if std::fs::symlink_metadata(&target).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                std::fs::remove_file(&target).unwrap();
                std::fs::write(&target, b"the person's own file").unwrap();
            }
        }),
    ));
    let r = Unify::new(&w.store, &p).unify(&sha(), OLD);
    assert_eq!(
        std::fs::read(&c_link).unwrap_or_default(),
        b"the person's own file",
        "the person's file was replaced by the hidden link; unify said {:?}",
        r.map(|x| x.skipped).map_err(|e| e.message)
    );
}

// R4. The "unreachable" link, new inputs: B is unplugged during the job AND
// during its undo, and plugged back only after both.
#[test]
fn r4_an_install_unplugged_through_the_job_and_its_undo_works_when_it_comes_back() {
    let (w, installs) = world();
    let c = &installs[2];
    let away = c.root.with_extension("unplugged");
    std::fs::rename(&c.root, &away).unwrap();
    let r = Unify::new(&w.store, &w.platform).unify(&sha(), OLD).unwrap();
    let after_job = {
        std::fs::rename(&away, &c.root).unwrap();
        let b = broken(&w, &installs);
        std::fs::rename(&c.root, &away).unwrap();
        b
    };
    let undo = Unify::new(&w.store, &w.platform).undo(&r.unify_id);
    std::fs::rename(&away, &c.root).unwrap();
    let after_undo = broken(&w, &installs);
    assert!(
        after_job.is_empty() && after_undo.is_empty() && undo.is_ok(),
        "after the job: {after_job:#?}\nundo: {:?}\nafter the undo: {after_undo:#?}",
        undo.map_err(|e| e.message)
    );
}

// R5. The same, with a SECOND job in between: C's recorded link keeps the
// vault's first name while two renames go past it.
#[test]
fn r5_an_unplugged_link_survives_two_name_jobs() {
    let (w, installs) = world();
    let (a, c) = (&installs[0], &installs[2]);
    let away = c.root.with_extension("unplugged");
    std::fs::rename(&c.root, &away).unwrap();
    let j1 = Unify::new(&w.store, &w.platform).unify(&sha(), OLD).unwrap();
    assert!(j1.stopped.is_none(), "{j1:?}");
    let vault_dir = |w: &TestWorld| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(w.vault_root.join("loras")).unwrap().flatten()
            .map(|e| format!("{} -> {:?}", e.file_name().to_string_lossy(), std::fs::read_link(e.path()).ok().map(|t| t.file_name().unwrap().to_string_lossy().to_string())))
            .collect();
        v.sort();
        v
    };
    eprintln!("R5 vault after job 1: {:?}; record {:?}", vault_dir(&w), w.store.vault_file(&sha()).unwrap().map(|r| (r.canonical_name, r.aliases)));
    // A third name appears in A, and a second job gives it to everyone.
    Links::new(&w.store, &w.platform)
        .create(&crate::links::CreateLinkRequest {
            install_id: a.id.clone(),
            sha256: sha(),
            relative_dir: "models/loras".into(),
            dir: None,
            link_name: Some("third.safetensors".into()),
            create_dir: true,
        })
        .unwrap();
    let j2 = Unify::new(&w.store, &w.platform).unify(&sha(), "third.safetensors").unwrap();
    eprintln!("R5 vault after job 2: {:?}; record {:?}", vault_dir(&w), w.store.vault_file(&sha()).unwrap().map(|r| (r.canonical_name, r.aliases)));
    std::fs::rename(&away, &c.root).unwrap();
    let b = broken(&w, &installs);
    assert!(b.is_empty(), "{b:#?}\njob 2: {j2:?}");
}
