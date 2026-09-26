//! Engine tests: the rules the front door itself enforces, and one run of the
//! whole flow from an empty vault to consolidated files.

use super::*;
use crate::platform::FakePlatform;
use crate::progress::NullSink;
use crate::testkit::{weights, weights_hash};

struct Fixture {
    dir: tempfile::TempDir,
    engine: Arc<Engine>,
    platform: Arc<FakePlatform>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let platform = Arc::new(FakePlatform::new());
        let engine =
            Engine::with_platform(dir.path().join("config/config.json"), platform.clone());
        Self { dir, engine, platform }
    }

    fn open_vault(&self) -> VaultInfo {
        self.engine
            .select_vault(&self.dir.path().join("ComfyVault"), true)
            .unwrap()
    }

    /// Builds a folder that passes the install test and registers it.
    fn add_install(&self, label: &str) -> Install {
        let root = self.dir.path().join(label);
        crate::install::detect::fixtures::make_install(&root);
        self.engine.register_install(&root, Some(label.into())).unwrap()
    }

    fn write_model(&self, install: &Install, rel: &str, content: &[u8]) -> PathBuf {
        let p = install.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    /// Runs a scan on this thread, the way a command line front end would.
    fn scan(&self) -> ScanRecord {
        let store = self.engine.store().unwrap();
        let installs = self.engine.installs().unwrap();
        let mut settings = store.settings().unwrap();
        settings.min_file_size_bytes = 0;
        // Never the machine's real Hugging Face cache: a test scans only what
        // the test put on disk.
        settings.huggingface_cache_dirs = Some(Vec::new());
        store.put_settings(&settings).unwrap();

        self.engine
            .run_scan(
                &store,
                &uuid::Uuid::new_v4().to_string(),
                &installs,
                &CancelToken::new(),
                &NullSink,
            )
            .unwrap()
    }
}

// --- nothing works before a vault is chosen --------------------------------

#[test]
fn every_command_that_needs_a_vault_says_so_before_one_is_chosen() {
    let f = Fixture::new();
    let e = &f.engine;

    let expect = |name: &str, r: Result<()>| {
        assert_eq!(
            r.expect_err(&format!("{name} answered when no vault is open")).code,
            ErrorCode::NotInitialized,
            "{name} answered something other than 'choose a vault first'"
        );
    };
    expect("list_installs", e.installs().map(|_| ()));
    expect("get_settings", e.settings().map(|_| ()));
    expect("build_plan", e.build_plan("x").map(|_| ()));
    expect("list_applies", e.applies().map(|_| ()));
    expect("list_orphans", e.orphans().map(|_| ()));
    expect("check_vault_health", e.vault_health().map(|_| ()));
    expect("list_name_groups", e.name_groups().map(|_| ()));
    expect("list_links", e.links(None, None, None).map(|_| ()));
    expect("check_model_usage", e.check_usage(&["m.safetensors".into()], None).map(|_| ()));
    expect("get_running_comfy", e.running_comfy().map(|_| ()));
    expect("register_install", e.register_install(f.dir.path(), None).map(|_| ()));
    // The five the interface calls the moment the window opens. All of them
    // refuse, so a boot sequence that calls them before a vault is chosen
    // greets a new person with a failure.
    expect("get_last_scan", e.last_scan().map(|_| ()));
    expect("get_interrupted_applies", e.interrupted_applies().map(|_| ()));
    expect("get_vault_info", e.vault_info().map(|_| ()));
    expect("list_vault_files", e.vault_files(0, 10, &Default::default(), crate::vault::VaultSort::Name, false).map(|_| ()));
    expect("list_contents", e.contents(0, 10, &Default::default(), crate::vault::VaultSort::Name, false).map(|_| ()));
    expect("get_scan_entries", e.scan_entries("x", 0, 10, &Default::default()).map(|_| ()));
    expect("update_settings", e.update_settings(&Default::default()).map(|_| ()));
    expect("get_metadata", e.metadata("x", false).map(|_| ()));
    expect("clear_metadata_cache", e.clear_metadata_cache().map(|_| ()));
}

#[test]
fn the_calls_a_first_run_can_make_all_answer_before_a_vault_is_chosen() {
    // The window opens on a machine with no vault. Whatever it may call then
    // has to answer, because a refusal on the first frame is what the person
    // sees instead of the screen that asks for a folder.
    //
    // This is the list the contract publishes. If a call is added to it, or
    // one of these starts refusing, this fails and the contract is wrong.
    let f = Fixture::new();
    let e = &f.engine;

    let state = e.app_state().expect("get_app_state must answer");
    assert!(!state.vault_initialized, "nothing is open yet");
    assert_eq!(state.vault_root, None);
    // Enough to draw the first screen without asking anything else.
    assert_eq!(state.settings, crate::settings::Settings::default());
    assert_eq!(state.install_count, 0);

    e.platform_report();
    e.validate_install_path(f.dir.path()).expect("validate_install_path must answer");
    assert!(e.locked_files(&[]).is_empty(), "check_locked_files must answer");
    let drives = e.drives();
    assert!(!drives.is_empty(), "list_drives must answer before a vault exists");
    assert!(e.busy().is_none(), "get_app_state reports nothing running");
}

#[test]
fn the_state_call_answers_before_a_vault_is_chosen() {
    // The window has to be able to draw itself and ask for a folder.
    let f = Fixture::new();
    let state = f.engine.app_state().unwrap();
    assert!(!state.vault_initialized);
    assert_eq!(state.vault_root, None);
    assert_eq!(state.install_count, 0);
    assert!(state.busy.is_none());
    // And it still reports what this computer can do, which is what decides
    // whether the interface can offer to consolidate at all.
    assert_eq!(state.platform.os, crate::platform::os_name());
}

#[test]
fn a_first_run_has_to_open_a_vault_before_it_can_add_an_install() {
    // The order a new person's first minute depends on. Nothing exists yet:
    // no folder, no database, no installs.
    let f = Fixture::new();
    let e = &f.engine;
    let install_dir = f.dir.path().join("ComfyUI-Demo");
    crate::install::detect::fixtures::make_install(&install_dir);
    let vault = f.dir.path().join("ComfyVault");
    assert!(!vault.exists(), "nothing has been created yet");

    // Adding an install first is refused. There is nowhere to record it.
    let refused = e.register_install(&install_dir, None).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotInitialized);

    // Opening a folder that is not there is refused unless asked to create it.
    let missing = e.select_vault(&vault, false).unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    assert!(!vault.exists(), "a refusal must not leave a folder behind");

    // Asked to create it, the engine makes the folder and the database.
    let info = e.select_vault(&vault, true).expect("create the vault");
    assert!(vault.is_dir(), "the vault folder was not created");
    assert!(
        vault.join(".comfyvault").join("vault.redb").is_file(),
        "the vault database was not created"
    );
    assert_eq!(info.file_count, 0);
    assert!(e.app_state().unwrap().vault_initialized);

    // Only now does adding an install work.
    let install = e.register_install(&install_dir, None).expect("register the install");
    assert_eq!(e.installs().unwrap().len(), 1);
    assert_eq!(install.root, crate::paths::canonicalize_clean(&install_dir).unwrap_or(install_dir));
}

#[test]
fn a_vault_on_a_drive_that_stops_answering_says_nothing_rather_than_zero() {
    // Zero of zero reads as a completely full drive, with a tick beside it.
    // The number is the most-read one in the application, so it says nothing
    // when nothing is known.
    let f = Fixture::new();
    let vault = f.dir.path().join("ComfyVault");
    let info = f.engine.select_vault(&vault, true).expect("create the vault");
    assert!(info.free_bytes.is_some(), "a readable drive reports its space");

    f.platform.fail_disk_space(true);
    let info = f.engine.vault_info().expect("the vault is still open");
    assert_eq!(info.free_bytes, None, "free space must be unknown, not zero");
    assert_eq!(info.total_bytes, None, "total size must be unknown, not zero");

    let v = serde_json::to_value(&info).unwrap();
    assert!(v["freeBytes"].is_null());
    assert_ne!(v["freeBytes"], serde_json::json!(0));
    // The rest of the answer still arrives. One unreadable figure does not
    // take the screen down.
    assert!(!v["root"].as_str().unwrap().is_empty());
}

// --- the vault -------------------------------------------------------------

#[test]
fn choosing_a_vault_creates_it_and_remembers_it_for_next_time() {
    let f = Fixture::new();
    let info = f.open_vault();
    assert!(info.root.ends_with("ComfyVault"));
    assert_eq!(info.file_count, 0);
    assert_eq!(info.schema_version, crate::store::SCHEMA_VERSION);

    // A fresh engine, the same config file: the vault comes back by itself.
    // The vault database allows one holder at a time, so the first engine lets
    // go of it first, exactly as closing the app would.
    f.engine.close_vault().unwrap();
    let again = Engine::with_platform(
        f.dir.path().join("config/config.json"),
        Arc::new(FakePlatform::new()),
    );
    assert!(again.restore_last_vault().is_none());
    assert!(again.app_state().unwrap().vault_initialized);
}

#[test]
fn a_remembered_vault_that_is_gone_is_reported_and_the_app_still_starts() {
    // A vault on a drive that is not plugged in must not stop the window from
    // appearing.
    let f = Fixture::new();
    f.open_vault();
    f.engine.close_vault().unwrap();
    std::fs::remove_dir_all(f.dir.path().join("ComfyVault")).unwrap();

    let again = Engine::with_platform(
        f.dir.path().join("config/config.json"),
        Arc::new(FakePlatform::new()),
    );
    let problem = again.restore_last_vault();
    assert!(problem.is_some(), "the person has to be told");
    assert!(again.app_state().unwrap().vault_initialized == false);
}

#[test]
fn a_vault_inside_a_registered_install_is_refused() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");

    let err = f.engine.select_vault(&i.root.join("inner-vault"), true).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("outside every install"));
}

#[test]
fn registering_an_install_that_holds_the_vault_is_refused() {
    let f = Fixture::new();
    let root = f.dir.path().join("Outer");
    crate::install::detect::fixtures::make_install(&root);
    f.engine.select_vault(&root.join("TheVault"), true).unwrap();

    let err = f.engine.register_install(&root, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
}

// --- installs --------------------------------------------------------------

#[test]
fn an_install_is_registered_listed_renamed_and_forgotten() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("Production");

    assert_eq!(f.engine.installs().unwrap().len(), 1);
    assert_eq!(f.engine.install(&i.id).unwrap().label, "Production");

    let renamed = f.engine.rename_install(&i.id, "  My Main Install  ").unwrap();
    assert_eq!(renamed.label, "My Main Install", "the name is trimmed");

    assert_eq!(f.engine.unregister_install(&i.id).unwrap(), 0);
    assert!(f.engine.installs().unwrap().is_empty());
}

#[test]
fn forgetting_an_install_never_removes_a_file_or_a_link() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    let p = f.write_model(&i, "models/loras/m.safetensors", &weights("m"));

    f.engine.unregister_install(&i.id).unwrap();
    assert!(p.is_file(), "forgetting an install must never touch its files");
    assert_eq!(std::fs::read(&p).unwrap(), weights("m"));
}

#[test]
fn the_same_install_cannot_be_registered_twice() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    let err = f.engine.register_install(&i.root, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyRegistered);
}

#[test]
fn a_folder_that_is_not_a_comfyui_install_is_refused_with_a_reason() {
    let f = Fixture::new();
    f.open_vault();
    let empty = f.dir.path().join("just-a-folder");
    std::fs::create_dir_all(&empty).unwrap();

    let candidate = f.engine.validate_install_path(&empty).unwrap();
    assert!(!candidate.valid);
    assert!(candidate.reason.is_some());

    let err = f.engine.register_install(&empty, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAComfyInstall);
}

#[test]
fn renaming_an_install_to_nothing_is_refused() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    assert_eq!(
        f.engine.rename_install(&i.id, "   ").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(f.engine.install(&i.id).unwrap().label, "A");
}

// --- one long operation at a time ------------------------------------------

#[test]
fn a_second_long_operation_is_refused_while_one_runs() {
    // Two of them touching the same files at once is the one thing that could
    // still lose data after everything else is right.
    let f = Fixture::new();
    f.open_vault();
    f.add_install("A");

    let cancel = f.engine.take_slot(BusyKind::Scan, "scan-1").unwrap();
    assert!(f.engine.busy().is_some());

    let err = f.engine.take_slot(BusyKind::Apply, "ap-1").unwrap_err();
    assert_eq!(err.code, ErrorCode::VaultBusy);
    assert!(err.message.contains("scan"), "the person needs to know what is running");

    // Cancelling reaches the operation that is running.
    f.engine.cancel("scan-1").unwrap();
    assert!(cancel.is_cancelled());

    f.engine.clear_slot();
    assert!(f.engine.busy().is_none());
    assert!(f.engine.take_slot(BusyKind::Apply, "ap-1").is_ok());
}

#[test]
fn two_launcher_installs_get_two_different_names() {
    // Both real roots are folders called ComfyUI. Named after the root alone,
    // every screen showed two installs called ComfyUI.
    let f = Fixture::new();
    f.open_vault();
    let mut labels = Vec::new();
    for launcher in ["Easy-Install", "Portable"] {
        let picked = f.dir.path().join(launcher);
        crate::install::detect::fixtures::make_install(&picked.join("ComfyUI"));
        labels.push(f.engine.register_install(&picked, None).unwrap().label);
    }
    assert_eq!(labels, vec!["Easy-Install", "Portable"]);

    // The root picked directly, when another install already has its name.
    let other = f.dir.path().join("Standalone");
    crate::install::detect::fixtures::make_install(&other.join("ComfyUI"));
    f.engine.rename_install(&f.engine.installs().unwrap()[0].id, "ComfyUI").unwrap();
    let i = f.engine.register_install(&other.join("ComfyUI"), None).unwrap();
    assert_eq!(i.label, "Standalone");

    // A name the person gives always wins.
    let named = f.dir.path().join("Named");
    crate::install::detect::fixtures::make_install(&named.join("ComfyUI"));
    assert_eq!(f.engine.register_install(&named, Some("Mine".into())).unwrap().label, "Mine");
}

#[test]
fn link_and_name_changes_wait_for_a_delete_in_progress() {
    // A link made between a delete's checks and its removals would be left
    // pointing at nothing. The delete holds this lock, and so does every
    // command that changes a link or a name, so one waits for the other.
    let f = Fixture::new();
    f.open_vault();
    let held = f.engine.write_lock().unwrap();

    // One thread per command, so each one is shown to wait on its own.
    let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
    let sha = weights_hash("m");
    type Call = fn(&Engine, &str) -> bool;
    let calls: Vec<(&'static str, Call)> = vec![
        ("create_link", |e, sha| {
            e.create_link(&CreateLinkRequest {
                install_id: "none".into(),
                sha256: sha.into(),
                relative_dir: "models/loras".into(),
                link_name: None,
                create_dir: false,
            })
            .is_err()
        }),
        ("remove_link", |e, _| e.remove_link("none").is_err()),
        ("set_canonical_name", |e, sha| e.set_canonical_name(sha, "x.safetensors").is_err()),
        ("remove_alias", |e, sha| e.remove_alias(sha, "x.safetensors").is_err()),
        ("delete_vault_file", |e, sha| e.delete_vault_file(sha, sha).is_err()),
        ("remove_dangling_links", |e, _| e.remove_dangling_links().is_ok()),
        ("delete_vault_file_and_links", |e, sha| e.delete_vault_file_and_links(sha, sha).is_err()),
    ];
    let count = calls.len();
    for (name, call) in calls {
        let (engine, tx, sha) = (Arc::clone(&f.engine), tx.clone(), sha.clone());
        std::thread::spawn(move || {
            assert!(call(&engine, &sha), "{name} answered unexpectedly");
            tx.send(name).unwrap();
        });
    }

    std::thread::sleep(std::time::Duration::from_millis(300));
    let early: Vec<&str> = rx.try_iter().collect();
    assert!(early.is_empty(), "these went ahead while a delete held the lock: {early:?}");
    drop(held);
    for _ in 0..count {
        rx.recv_timeout(std::time::Duration::from_secs(10)).expect("a command never finished");
    }
}

#[test]
fn a_model_is_not_deleted_with_its_links_while_a_long_operation_runs() {
    // A consolidation could be linking new copies to this very file.
    let f = Fixture::new();
    f.open_vault();
    let sha = weights_hash("m");

    f.engine.take_slot(BusyKind::Apply, "ap-1").unwrap();
    let err = f.engine.delete_vault_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::VaultBusy);
    assert!(err.message.contains("consolidation"), "{}", err.message);

    // The control: with nothing running, the same call reaches the vault,
    // which does not hold that model.
    f.engine.clear_slot();
    let err = f.engine.delete_vault_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn cancelling_something_that_is_not_running_says_so() {
    let f = Fixture::new();
    assert_eq!(f.engine.cancel("nothing").unwrap_err().code, ErrorCode::NotFound);

    f.engine.take_slot(BusyKind::Scan, "scan-1").unwrap();
    assert_eq!(f.engine.cancel("a-different-id").unwrap_err().code, ErrorCode::NotFound);
    f.engine.clear_slot();
}

#[test]
fn a_scan_started_on_a_thread_finishes_and_frees_the_slot() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    f.write_model(&i, "models/loras/m.safetensors", &weights("m"));

    let done = Arc::new(Mutex::new(None));
    let sink: Arc<dyn ProgressSink<ScanProgress>> = Arc::new(crate::progress::NullSink);
    let captured = Arc::clone(&done);

    let scan_id = f
        .engine
        .start_scan(
            None,
            sink,
            Arc::new(move |r: Result<ScanRecord>| {
                *captured.lock().unwrap() = Some(r.map(|rec| rec.scan_id));
            }),
        )
        .unwrap();

    // Wait for the worker, without a fixed sleep.
    for _ in 0..600 {
        if done.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let result = done.lock().unwrap().take().expect("the scan never reported back");
    assert_eq!(result.unwrap(), scan_id);
    assert!(f.engine.busy().is_none(), "the slot must be free again");
}

// --- the whole flow --------------------------------------------------------

#[test]
fn an_empty_vault_becomes_a_consolidated_one() {
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("Production");
    let b = f.add_install("Normal");

    let pa = f.write_model(&a, "models/loras/awesomeloras/lora1.safetensors", &weights("lora1"));
    let pb = f.write_model(&b, "models/loras/newloras/lora1.safetensors", &weights("lora1"));
    f.write_model(&a, "models/checkpoints/only.safetensors", &weights("only"));

    // Scan.
    let record = f.scan();
    assert_eq!(record.totals.movable_files, 3);
    assert_eq!(record.totals.unique_contents, 2);
    assert_eq!(record.totals.reclaimable_bytes, weights("lora1").len() as u64);

    // Plan.
    let plan = f.engine.build_plan(&record.scan_id).unwrap();
    assert_eq!(plan.groups.len(), 2);
    assert_eq!(plan.totals.bytes_freed, weights("lora1").len() as u64);

    // Apply, on this thread.
    let store = f.engine.store().unwrap();
    let applied = Applier::new(&store, f.engine.platform())
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
                verify: crate::apply::VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();
    assert_eq!(applied.groups_applied, 2);

    // Both old places still read the weights, which is the whole promise.
    assert_eq!(std::fs::read(&pa).unwrap(), weights("lora1"));
    assert_eq!(std::fs::read(&pb).unwrap(), weights("lora1"));

    // The vault lists what it holds, and the health check is happy.
    let page = f
        .engine
        .vault_files(0, 100, &VaultFilter::default(), VaultSort::Name, false)
        .unwrap();
    assert_eq!(page.total, 2);
    assert!(f.engine.vault_health().unwrap().ok);
    assert!(f.engine.orphans().unwrap().is_empty());

    // The install's record carries what the scan found in it.
    let after = f.engine.install(&a.id).unwrap();
    assert!(after.last_scan_at.is_some());
    assert_eq!(after.last_scan_totals.unwrap().totals.movable_files, 2);

    // And the plan built from a fresh scan has nothing left to do.
    let second = f.engine.build_plan(&f.scan().scan_id).unwrap();
    assert!(second.groups.is_empty());
}

#[test]
fn a_scan_page_can_be_filtered_and_paged() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    for n in 0..7 {
        f.write_model(&i, &format!("models/loras/m{n}.safetensors"), &weights(&format!("m{n}")));
    }
    f.write_model(&i, "models/checkpoints/c.safetensors", &weights("c"));

    let record = f.scan();
    let all = f
        .engine
        .scan_entries(&record.scan_id, 0, 100, &ScanEntryFilter::default())
        .unwrap();
    assert_eq!(all.total, 8);

    let page = f
        .engine
        .scan_entries(&record.scan_id, 2, 3, &ScanEntryFilter::default())
        .unwrap();
    assert_eq!(page.entries.len(), 3);
    assert_eq!(page.total, 8);

    let loras = f
        .engine
        .scan_entries(
            &record.scan_id,
            0,
            100,
            &ScanEntryFilter { category: Some("loras".into()), ..Default::default() },
        )
        .unwrap();
    assert_eq!(loras.total, 7);
}

#[test]
fn asking_about_a_scan_that_never_happened_says_so() {
    let f = Fixture::new();
    f.open_vault();
    assert_eq!(
        f.engine
            .scan_entries("never", 0, 10, &ScanEntryFilter::default())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(f.engine.build_plan("never").unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(f.engine.plan("never").unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(f.engine.apply_record("never").unwrap_err().code, ErrorCode::NotFound);
}

// --- settings --------------------------------------------------------------

#[test]
fn nothing_the_engine_hands_out_carries_a_credential() {
    // There is no Civitai key, because looking a model up by hash needs none
    // and this version does not download. A field storing a credential for a
    // feature nobody can reach, in a file the person is told to carry on a
    // portable drive, is a liability with nothing on the other side of it.
    let f = Fixture::new();
    f.open_vault();

    for (what, json) in [
        ("settings", serde_json::to_string(&f.engine.settings().unwrap()).unwrap()),
        ("app_state", serde_json::to_string(&f.engine.app_state().unwrap()).unwrap()),
    ] {
        let lower = json.to_lowercase();
        for word in ["apikey", "api_key", "civitai", "token", "secret", "password"] {
            assert!(!lower.contains(word), "{what} carries {word}: {json}");
        }
    }
}

#[test]
fn settings_survive_a_restart() {
    let f = Fixture::new();
    f.open_vault();
    f.engine
        .update_settings(&SettingsPatch {
            metadata_lookups_enabled: Some(false),
            min_file_size_bytes: Some(4096),
            ..Default::default()
        })
        .unwrap();

    f.engine.close_vault().unwrap();
    let again = Engine::with_platform(
        f.dir.path().join("config/config.json"),
        Arc::new(FakePlatform::new()),
    );
    assert!(again.restore_last_vault().is_none(), "the vault did not reopen");
    let s = again.settings().unwrap();
    assert!(!s.metadata_lookups_enabled);
    assert_eq!(s.min_file_size_bytes, 4096);
}

// --- metadata offline ------------------------------------------------------

#[test]
fn a_metadata_lookup_with_no_network_never_breaks_anything() {
    // There is no network in a test. The call must answer, not fail.
    let f = Fixture::new();
    f.open_vault();
    f.engine
        .update_settings(&SettingsPatch {
            metadata_lookups_enabled: Some(false),
            ..Default::default()
        })
        .unwrap();

    assert!(f.engine.metadata(&weights_hash("m"), false).unwrap().is_none());
    let batch = f.engine.metadata_batch(&[weights_hash("m")], false).unwrap();
    assert_eq!(batch.len(), 1);
    assert!(!batch[0].found, "not found is a normal answer");
}

// --- the shapes the interface reads ----------------------------------------

#[test]
fn the_state_call_serializes_with_the_names_the_contract_promises() {
    let f = Fixture::new();
    f.open_vault();
    f.add_install("A");

    let v = serde_json::to_value(f.engine.app_state().unwrap()).unwrap();
    for key in [
        "vaultRoot",
        "vaultInitialized",
        "installCount",
        "platform",
        "settings",
        "lastScanId",
        "lastPlanId",
        "interruptedApplies",
        "busy",
    ] {
        assert!(v.get(key).is_some(), "the contract promises {key}");
    }
    assert!(v["platform"].get("longPathsEnabled").is_some());
    assert!(v["platform"]["symlinks"].get("supported").is_some());
}

#[test]
fn locked_file_answers_carry_whether_the_answer_means_anything() {
    // On Linux the answer is unknowable, and it must say so rather than report
    // a confident "not locked".
    let f = Fixture::new();
    let states = f.engine.locked_files(&[f.dir.path().join("anything")]);
    assert_eq!(states.len(), 1);
    let v = serde_json::to_value(&states[0]).unwrap();
    assert!(v.get("checkable").is_some());
    assert!(v.get("locked").is_some());
}

// --- the drive the vault sits on -------------------------------------------

#[test]
fn vault_facts_can_be_read_without_reopening_the_vault() {
    // The free space is the most-read number in the application. Asking for it
    // must not mean opening the vault again.
    let f = Fixture::new();
    let opened = f.open_vault();
    let again = f.engine.vault_info().unwrap();

    assert_eq!(again.root, opened.root);
    assert_eq!(again.created_at, opened.created_at);
    // A readable drive reports both figures, and null would mean it was not.
    let total = again.total_bytes.expect("the drive the vault is on is readable");
    let free = again.free_bytes.expect("the drive the vault is on is readable");
    assert!(total > 0, "the drive has a size");
    assert!(free <= total);
    assert_eq!(again.file_count, 0);
}

#[test]
fn vault_facts_follow_what_the_vault_holds() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    f.write_model(&i, "models/loras/m.safetensors", &weights("m"));

    let record = f.scan();
    let plan = f.engine.build_plan(&record.scan_id).unwrap();
    let store = f.engine.store().unwrap();
    Applier::new(&store, f.engine.platform())
        .apply("ap-1", &plan, &ApplyRequest {
            plan_id: plan.plan_id.clone(),
            group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
            verify: crate::apply::VerifyModeArg::SizeAndMtime,
            stop_on_error: false,
        }, &CancelToken::new(), &NullSink)
        .unwrap();

    let info = f.engine.vault_info().unwrap();
    assert_eq!(info.file_count, 1);
    assert_eq!(info.total_stored_bytes, weights("m").len() as u64);
}

#[test]
fn asking_for_vault_facts_before_a_vault_is_chosen_says_so() {
    let f = Fixture::new();
    assert_eq!(f.engine.vault_info().unwrap_err().code, ErrorCode::NotInitialized);
}

#[test]
fn a_folder_picker_starts_at_every_drive_not_inside_one() {
    // On Windows a person's models can be on D:. A picker that starts inside
    // C: can never reach them, and nothing there could be registered.
    let platform = FakePlatform::new();
    platform.set_drive_roots(vec![PathBuf::from("C:\\"), PathBuf::from("D:\\")]);
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::with_platform(dir.path().join("config.json"), Arc::new(platform));

    let roots = engine.platform().drive_roots();
    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&PathBuf::from("D:\\")), "the second drive must be reachable");
}

#[test]
fn the_real_platform_reports_at_least_one_root() {
    let f = Fixture::new();
    let roots = comfyvault_core_platform_roots(&f);
    assert!(!roots.is_empty(), "a computer always has somewhere to start browsing");
    for r in &roots {
        assert!(r.is_absolute(), "a starting point must be an absolute path: {r:?}");
    }
}

fn comfyvault_core_platform_roots(f: &Fixture) -> Vec<PathBuf> {
    f.engine.platform().drive_roots()
}

// --- no Windows verbatim prefix ever escapes the engine --------------------

/// Every path in a value the interface receives, so one test can sweep them.
fn every_path_in(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains(":\\") || s.starts_with('/') || s.contains(r"\\") {
                out.push(s.clone());
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|v| every_path_in(v, out)),
        serde_json::Value::Object(o) => o.values().for_each(|v| every_path_in(v, out)),
        _ => {}
    }
}

#[test]
fn no_path_the_interface_receives_carries_the_windows_verbatim_prefix() {
    // Canonicalizing on Windows returns \\?\C:\... and that path becomes the
    // install root, every scan entry, every link and the vault itself. All of
    // those reach the screen, so without cleaning, the person is shown
    // \\?\C:\Users\... everywhere a path appears. It also breaks every
    // comparison that mixes a canonicalized path with a plain one.
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("Production");
    let b = f.add_install("Normal");
    f.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    f.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let record = f.scan();
    let plan = f.engine.build_plan(&record.scan_id).unwrap();
    let store = f.engine.store().unwrap();
    Applier::new(&store, f.engine.platform())
        .apply("ap-1", &plan, &ApplyRequest {
            plan_id: plan.plan_id.clone(),
            group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
            verify: crate::apply::VerifyModeArg::SizeAndMtime,
            stop_on_error: false,
        }, &CancelToken::new(), &NullSink)
        .unwrap();

    // Only the payloads that really carry absolute paths. A payload with none
    // would pass this test while proving nothing, so the sweep refuses one.
    let payloads = vec![
        ("app_state", serde_json::to_value(f.engine.app_state().unwrap()).unwrap()),
        ("vault_info", serde_json::to_value(f.engine.vault_info().unwrap()).unwrap()),
        ("installs", serde_json::to_value(f.engine.installs().unwrap()).unwrap()),
        ("plan", serde_json::to_value(&plan).unwrap()),
        ("links", serde_json::to_value(f.engine.links(None, None, None).unwrap()).unwrap()),
        (
            "scan_entries",
            serde_json::to_value(
                f.engine
                    .scan_entries(&record.scan_id, 0, 100, &ScanEntryFilter::default())
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            "model_dirs",
            serde_json::to_value(f.engine.install_model_dirs(&a.id).unwrap()).unwrap(),
        ),
    ];

    let mut checked = 0;
    for (name, payload) in payloads {
        let mut paths = Vec::new();
        every_path_in(&payload, &mut paths);
        assert!(!paths.is_empty(), "{name} carried no paths, so this test proved nothing");
        for p in paths {
            assert!(
                !p.starts_with(r"\\?\"),
                "{name} handed the interface a verbatim path: {p}"
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "only {checked} paths were swept, which is too few to trust");
}

#[test]
fn a_path_spelled_the_way_the_interface_sends_it_finds_the_same_link() {
    // The contract's examples use forward slashes. On Windows that names the
    // same folder as a backslash, and the engine has to agree with itself.
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("A");
    f.write_model(&a, "models/loras/m.safetensors", &weights("m"));

    let record = f.scan();
    let plan = f.engine.build_plan(&record.scan_id).unwrap();
    let store = f.engine.store().unwrap();
    Applier::new(&store, f.engine.platform())
        .apply("ap-1", &plan, &ApplyRequest {
            plan_id: plan.plan_id.clone(),
            group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
            verify: crate::apply::VerifyModeArg::SizeAndMtime,
            stop_on_error: false,
        }, &CancelToken::new(), &NullSink)
        .unwrap();

    let stored = f.engine.links(None, None, None).unwrap();
    assert_eq!(stored.len(), 1);
    let as_walked = stored[0].link.abs_path.clone();

    // The same file, spelled the way a path from the interface arrives.
    let as_sent = PathBuf::from(
        crate::paths::display_path(&as_walked).replace('\\', "/"),
    );
    assert!(
        store.link_at_path(&as_sent).unwrap().is_some(),
        "the link vanished when its path was spelled with forward slashes: {as_sent:?}"
    );
}

#[test]
fn a_stored_install_row_is_proved_on_the_disk_before_it_is_scanned_or_linked_into() {
    // The vault's database travels with the vault. A row naming a folder that
    // is not a ComfyUI install on this disk must not decide where the engine
    // reads or writes.
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    let store = f.engine.store().unwrap();
    let mut settings = store.settings().unwrap();
    settings.min_file_size_bytes = 0;
    settings.huggingface_cache_dirs = Some(Vec::new());
    store.put_settings(&settings).unwrap();

    let elsewhere = f.dir.path().join("Elsewhere");
    let secret = elsewhere.join("models/loras/private.safetensors");
    std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
    std::fs::write(&secret, weights("private")).unwrap();
    store
        .put_install(&Install {
            root: elsewhere.clone(),
            registered_path: elsewhere.clone(),
            models_dir: elsewhere.join("models"),
            ..i.clone()
        })
        .unwrap();

    let done = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&done);
    f.engine
        .start_scan(
            None,
            Arc::new(crate::progress::NullSink),
            Arc::new(move |r: Result<ScanRecord>| *captured.lock().unwrap() = Some(r)),
        )
        .unwrap();
    for _ in 0..600 {
        if done.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let record = done.lock().unwrap().take().expect("the scan never reported back").unwrap();
    let page = f.engine.scan_entries(&record.scan_id, 0, 100, &Default::default()).unwrap();
    assert_eq!(page.total, 0, "the scan read a folder only a database row named");

    let err = f.engine.create_model_folder(&i.id, "loras/new").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAComfyInstall);
    assert!(!elsewhere.join("models/loras/new").exists(), "a folder was made where only a row pointed");
}

#[test]
fn choosing_the_vault_that_is_already_open_answers_with_it() {
    let f = Fixture::new();
    let first = f.open_vault();
    let again = f.engine.select_vault(&f.dir.path().join("ComfyVault"), true).unwrap();
    assert_eq!(again.root, first.root);
    assert!(f.engine.vault_info().is_ok(), "the vault was closed by choosing it again");
}

#[test]
fn a_vault_refused_inside_an_install_leaves_nothing_behind() {
    let f = Fixture::new();
    f.open_vault();
    let i = f.add_install("A");
    let inside = i.root.join("models").join("vault");
    let err = f.engine.select_vault(&inside, true).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(!inside.exists(), "a refused vault still made a folder and a database inside the install");
}

#[cfg(windows)]
#[test]
fn the_folder_picker_leaves_out_hidden_and_system_folders() {
    let f = Fixture::new();
    let root = f.dir.path().join("Pick");
    for name in ["Shown", "Hidden", "System"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    for (name, flag) in [("Hidden", "+h"), ("System", "+s")] {
        let out = std::process::Command::new("attrib").arg(flag).arg(root.join(name)).output().unwrap();
        assert!(out.status.success(), "attrib failed: {out:?}");
    }
    let listing = f.engine.list_directory(&root.to_string_lossy()).unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Shown"]);
}

#[cfg(windows)]
#[test]
fn a_folder_windows_refuses_is_reported_as_a_refused_permission() {
    let f = Fixture::new();
    let locked = f.dir.path().join("Refused");
    std::fs::create_dir_all(&locked).unwrap();
    let who = std::env::var("USERNAME").unwrap();
    let deny = std::process::Command::new("icacls")
        .arg(&locked)
        .args(["/deny", &format!("{who}:(RD)")])
        .output()
        .unwrap();
    assert!(deny.status.success(), "icacls could not deny: {deny:?}");
    let result = f.engine.list_directory(&locked.to_string_lossy());
    let _ = std::process::Command::new("icacls").arg(&locked).args(["/remove:d", &who]).output();
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert!(!err.message.contains("Another program"), "a cause was guessed: {}", err.message);
}

#[test]
fn a_vault_is_made_only_in_an_empty_folder_or_opened_where_one_already_is() {
    let f = Fixture::new();

    // A folder that already holds models: refused, and nothing in it changes.
    let busy = f.dir.path().join("MyModels");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("kept.safetensors"), weights("kept")).unwrap();
    let err = f.engine.select_vault(&busy, true).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("empty folder"), "{}", err.message);
    assert!(!busy.join(crate::store::INTERNAL_DIR).exists(), "the refused folder was changed");
    assert_eq!(std::fs::read(busy.join("kept.safetensors")).unwrap(), weights("kept"));
    let names: Vec<_> = std::fs::read_dir(&busy).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names.len(), 1);

    // An empty folder and a folder not made yet both become vaults.
    let empty = f.dir.path().join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    f.engine.select_vault(&empty, true).unwrap();
    f.engine.select_vault(&f.dir.path().join("New"), true).unwrap();

    // A vault that holds models opens again.
    let vault = f.dir.path().join("New");
    std::fs::create_dir_all(vault.join("loras")).unwrap();
    std::fs::write(vault.join("loras/m.safetensors"), weights("m")).unwrap();
    f.engine.select_vault(&empty, false).unwrap();
    f.engine.select_vault(&vault, false).unwrap();
}

// --- a ComfyUI that serves nothing, and one that holds a file ---------------

fn comfy_process(pid: u32, root: &Path) -> crate::platform::ProcessInfo {
    crate::platform::ProcessInfo {
        pid,
        name: "python.exe".into(),
        exe_path: Some(root.join("python_embeded").join("python.exe")),
        cwd: Some(root.to_path_buf()),
        command_line: vec!["python.exe".into(), "ComfyUI/main.py".into()],
        started_at: Some(crate::time_util::Timestamp::from_millis(1_758_412_800_000)),
    }
}

#[test]
fn each_running_comfyui_says_what_it_serves_and_what_it_holds() {
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("ComfyUI-A");
    f.write_model(&a, "models/checkpoints/base.safetensors", &weights("base"));
    f.scan();

    // 7 serves a page, 8 is the leftover that serves nothing, and the system
    // could not answer anything about 9.
    f.platform.set_processes(vec![
        comfy_process(7, &a.root),
        comfy_process(8, &a.root),
        comfy_process(9, &a.root),
    ]);
    f.platform.set_listening([(7, vec![8188, 8188, 80]), (8, vec![])].into());
    f.platform.set_holding([(7, false), (8, true)].into());

    let got = f.engine.running_comfy().unwrap();
    let by = |pid: u32| got.iter().find(|r| r.pid == pid).unwrap();

    assert_eq!(by(7).listening_ports, Some(vec![80, 8188]), "sorted, each port once");
    assert_eq!(by(7).holds_model_files, Some(false));
    assert_eq!(by(8).listening_ports, Some(vec![]), "a leftover listens on nothing");
    assert_eq!(by(8).holds_model_files, Some(true));
    // Unknown stays unknown. Reporting "not listening" here would tell a
    // person that a working ComfyUI is safe to end.
    assert_eq!(by(9).listening_ports, None);
    assert_eq!(by(9).holds_model_files, None);
    assert_eq!(
        by(9).started_at,
        Some(crate::time_util::Timestamp::from_millis(1_758_412_800_000)),
        "when it started reaches the interface"
    );
}

#[test]
fn the_files_asked_about_are_every_file_an_apply_or_an_undo_could_move() {
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("ComfyUI-A");
    let found = f.write_model(&a, "models/loras/detail.safetensors", &weights("detail"));

    // A link the scan finds, pointing at a file elsewhere on the disk.
    let elsewhere = f.dir.path().join("elsewhere").join("style.safetensors");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, weights("style")).unwrap();
    let link = a.root.join("models/loras/style.safetensors");
    crate::platform::NativePlatform::new().create_file_symlink(&link, &elsewhere).unwrap();

    // A file already in the vault.
    let store = f.engine.store().unwrap();
    let spare = f.dir.path().join("spare.safetensors");
    std::fs::write(&spare, weights("spare")).unwrap();
    let placed = crate::vault::place_file(
        &store,
        f.platform.as_ref(),
        &spare,
        "checkpoints",
        "spare.safetensors",
        &weights_hash("spare"),
        &CancelToken::new(),
    )
    .unwrap();
    let in_vault = store.vault_root().join(placed.vault_rel_path());

    f.scan();
    f.platform.set_processes(vec![comfy_process(7, &a.root)]);
    f.engine.running_comfy().unwrap();

    let asked = f.platform.files_asked_about().expect("the engine asked no question");
    for want in [&found, &link, &elsewhere, &in_vault] {
        assert!(asked.contains(want), "{} was not asked about: {asked:?}", want.display());
    }
}

#[test]
fn with_nothing_scanned_and_an_empty_vault_whether_a_file_is_held_is_unknown() {
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("ComfyUI-A");
    f.platform.set_processes(vec![comfy_process(7, &a.root)]);
    f.platform.set_holding([(7, false)].into());

    let got = f.engine.running_comfy().unwrap();
    // "Holds none of no files" is not an answer, so no question is asked.
    assert_eq!(f.platform.files_asked_about(), None);
    assert_eq!(got[0].holds_model_files, None);
}

#[test]
fn a_running_comfyui_sent_by_an_older_build_still_reads() {
    let old = serde_json::json!({
        "pid": 18244,
        "name": "python.exe",
        "exePath": "C:\\ComfyUI-Main\\python_embeded\\python.exe",
        "cwd": "C:\\ComfyUI-Main",
        "commandLine": ["python.exe", "main.py"],
        "matchedInstallIds": ["inst-1"],
        "matchReason": "exeUnderRoot"
    });
    let r: RunningComfy = serde_json::from_value(old).unwrap();
    assert_eq!(r.started_at, None);
    assert_eq!(r.listening_ports, None);
    assert_eq!(r.holds_model_files, None);
}

// --- a cancelled scan does not erase the last one that finished ------------

fn cancelled_scan(f: &Fixture) -> ScanRecord {
    let store = f.engine.store().unwrap();
    let installs = f.engine.installs().unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    f.engine
        .run_scan(&store, &uuid::Uuid::new_v4().to_string(), &installs, &cancel, &NullSink)
        .unwrap()
}

#[test]
fn a_cancelled_scan_leaves_the_last_finished_scan_as_the_one_that_counts() {
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("ComfyUI-A");
    f.write_model(&a, "models/checkpoints/base.safetensors", &weights("base"));
    f.write_model(&a, "models/loras/base-copy.safetensors", &weights("base"));
    let full = f.scan();
    assert!(!full.cancelled);
    assert!(full.totals.files_seen > 0, "the fixture scan found nothing");
    let totals_before = f.engine.installs().unwrap()[0].last_scan_totals.clone();
    assert!(totals_before.is_some());

    let stopped = cancelled_scan(&f);
    assert!(stopped.cancelled, "the scan was not cancelled");

    // The results still come from the scan that finished.
    let last = f.engine.last_scan().unwrap().unwrap();
    assert_eq!(last.scan_id, full.scan_id, "the cancelled scan replaced the finished one");
    assert_eq!(last.totals, full.totals);
    let state = f.engine.app_state().unwrap();
    assert_eq!(state.last_scan_id.as_deref(), Some(full.scan_id.as_str()));
    // And the cancellation is still a fact the person can be told.
    assert_eq!(state.last_cancelled_scan_at, Some(stopped.finished_at));
    // The install keeps the totals of the scan that finished.
    assert_eq!(f.engine.installs().unwrap()[0].last_scan_totals, totals_before);
    // The plan is built from the finished scan, and finds the copy.
    assert!(!f.engine.build_plan(&last.scan_id).unwrap().groups.is_empty());

    // A scan that finishes afterwards clears the cancelled fact.
    let again = f.scan();
    assert_eq!(f.engine.last_scan().unwrap().unwrap().scan_id, again.scan_id);
    assert_eq!(f.engine.app_state().unwrap().last_cancelled_scan_at, None);
}

#[test]
fn when_no_scan_has_finished_the_cancelled_one_is_still_the_last_scan() {
    let f = Fixture::new();
    f.open_vault();
    f.add_install("ComfyUI-A");
    let stopped = cancelled_scan(&f);

    let last = f.engine.last_scan().unwrap().expect("a scan ran, so there is a last scan");
    assert_eq!(last.scan_id, stopped.scan_id);
    assert!(last.cancelled);
    assert_eq!(f.engine.app_state().unwrap().last_cancelled_scan_at, Some(stopped.finished_at));
    // Nothing was read, so the install says so rather than showing zero.
    assert_eq!(f.engine.installs().unwrap()[0].last_scan_totals, None);
}

#[test]
fn a_vault_from_an_older_build_finds_its_last_finished_scan() {
    // Before the engine kept a pointer to the last finished scan, a cancelled
    // scan moved the only pointer. Such a vault must still read correctly.
    let f = Fixture::new();
    f.open_vault();
    let a = f.add_install("ComfyUI-A");
    f.write_model(&a, "models/checkpoints/base.safetensors", &weights("base"));
    let full = f.scan();
    let stopped = cancelled_scan(&f);

    let store = f.engine.store().unwrap();
    store.forget_finished_scan_pointer_for_tests();
    assert_eq!(store.latest_scan_id().unwrap(), Some(stopped.scan_id.clone()));
    assert_eq!(f.engine.last_scan().unwrap().unwrap().scan_id, full.scan_id);
}

#[test]
fn opening_another_vault_stops_a_download_and_keeps_its_part() {
    // A download takes no long-operation slot, so nothing else would stop it
    // writing into a vault that is no longer the open one.
    use crate::download::test_server::{Canned, Server};
    let body: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
    let b2 = body.clone();
    let server = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let base = server.clone();
    let s = Server::start(move |req| {
        let base = base.lock().unwrap().clone();
        match (req.method.as_str(), req.path()) {
            (_, "/o/r/resolve/main/m.safetensors") => Canned::redirect(&format!("{base}/store/m")),
            ("POST", _) => Canned::json(200, &format!(r#"[{{"path":"m.safetensors","size":{}}}]"#, b2.len())),
            (_, "/store/m") => Canned {
                stall_after: Some((100_000, std::time::Duration::from_secs(5))),
                ..Canned::file(req, &b2, "\"v\"")
            },
            _ => Canned::new(404),
        }
    });
    *server.lock().unwrap() = s.base.clone();

    let dir = tempfile::tempdir().unwrap();
    let platform = Arc::new(FakePlatform::new());
    let downloads = crate::download::Downloader::new(
        Arc::new(crate::download::http::UreqWeb::new()),
        crate::download::sites::Sites { hugging_face: s.base.clone(), civitai: s.base.clone() },
        Arc::new(crate::download::tokens::MemoryTokens::default()),
    );
    let e = Engine::with_parts(dir.path().join("config.json"), platform, downloads);
    let first = dir.path().join("VaultOne");
    e.select_vault(&first, true).unwrap();
    let d = e
        .start_download(&crate::download::StartDownload {
            address: "https://huggingface.co/o/r/blob/main/m.safetensors".into(),
            version_id: None,
            file_id: None,
            category: "loras".into(),
            install_ids: vec![],
        })
        .unwrap();
    let part = first.join(".comfyvault/downloads").join(format!("{}.part", d.download_id));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) < 100_000 {
        assert!(std::time::Instant::now() < until, "no bytes arrived");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    e.select_vault(&dir.path().join("VaultTwo"), true).unwrap();
    assert!(e.list_downloads().unwrap().is_empty(), "the other vault has its own list");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 100_000, "the part is kept, and nothing more was written");

    e.select_vault(&first, false).unwrap();
    let back = e.list_downloads().unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].state, crate::download::DownloadState::Stopped);
    assert_eq!(back[0].bytes_done, 100_000);
}

