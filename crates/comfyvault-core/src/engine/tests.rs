//! Engine tests: the rules the front door itself enforces, and one run of the
//! whole flow from an empty vault to consolidated files.

use super::*;
use crate::platform::FakePlatform;
use crate::progress::NullSink;
use crate::testkit::{weights, weights_hash};

struct Fixture {
    dir: tempfile::TempDir,
    engine: Arc<Engine>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_platform(
            dir.path().join("config/config.json"),
            Arc::new(FakePlatform::new()),
        );
        Self { dir, engine }
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

    let expect = |r: Result<()>| {
        assert_eq!(
            r.unwrap_err().code,
            ErrorCode::NotInitialized,
            "this command answered something other than 'choose a vault first'"
        );
    };
    expect(e.installs().map(|_| ()));
    expect(e.settings().map(|_| ()));
    expect(e.build_plan("x").map(|_| ()));
    expect(e.applies().map(|_| ()));
    expect(e.orphans().map(|_| ()));
    expect(e.vault_health().map(|_| ()));
    expect(e.name_groups().map(|_| ()));
    expect(e.links(None, None, None).map(|_| ()));
    expect(e.check_usage(&["m.safetensors".into()], None).map(|_| ()));
    expect(e.running_comfy().map(|_| ()));
    expect(e.register_install(f.dir.path(), None).map(|_| ()));
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
fn the_api_key_never_comes_back_out_of_the_engine() {
    // It is a credential. It goes in and it is used, and nothing above the
    // engine can read it back out of a window or a log.
    let f = Fixture::new();
    f.open_vault();

    f.engine
        .update_settings(&SettingsPatch {
            civitai_api_key: Some("the-real-key".into()),
            ..Default::default()
        })
        .unwrap();

    let s = f.engine.settings().unwrap();
    assert_eq!(s.civitai_api_key.as_deref(), Some("***"));
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("the-real-key"));

    let state_json = serde_json::to_string(&f.engine.app_state().unwrap()).unwrap();
    assert!(!state_json.contains("the-real-key"), "the key leaked into the state call");
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
    assert!(again.total_bytes > 0, "the drive has a size");
    assert!(again.free_bytes <= again.total_bytes);
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
    // On Windows the person's models can be on D:. A picker that starts inside
    // C: can never reach them, and they could not register anything there.
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
    let as_walked = stored[0].abs_path.clone();

    // The same file, spelled the way a path from the interface arrives.
    let as_sent = PathBuf::from(
        crate::paths::display_path(&as_walked).replace('\\', "/"),
    );
    assert!(
        store.link_at_path(&as_sent).unwrap().is_some(),
        "the link vanished when its path was spelled with forward slashes: {as_sent:?}"
    );
}
