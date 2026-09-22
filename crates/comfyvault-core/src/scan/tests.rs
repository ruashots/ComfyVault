//! Scan tests. Every one builds its own tree in a temporary folder.

use super::*;
use crate::progress::{NullSink, RecordingSink};
use crate::testkit::{weights, weights_hash, TestWorld};

fn entry<'a>(out: &'a ScanOutcome, name: &str) -> &'a ScanEntryRecord {
    out.entries
        .iter()
        .find(|e| e.abs_path.file_name().unwrap().to_string_lossy() == name)
        .unwrap_or_else(|| panic!("no entry called {name} in {:?}", paths(out)))
}

fn paths(out: &ScanOutcome) -> Vec<String> {
    out.entries
        .iter()
        .map(|e| e.abs_path.file_name().unwrap().to_string_lossy().to_string())
        .collect()
}

fn find<'a>(out: &'a ScanOutcome, name: &str) -> Option<&'a ScanEntryRecord> {
    out.entries
        .iter()
        .find(|e| e.abs_path.file_name().unwrap().to_string_lossy() == name)
}

#[test]
fn a_scan_finds_a_model_and_identifies_it() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/lora1.safetensors", &weights("lora1"));

    let out = w.scan(&[i]);
    assert_eq!(out.entries.len(), 1);
    let e = entry(&out, "lora1.safetensors");
    assert_eq!(e.sha256.as_deref(), Some(weights_hash("lora1").as_str()));
    assert_eq!(e.category, "loras");
    assert_eq!(e.classification, Classification::Movable);
}

#[test]
fn the_same_content_in_two_installs_is_one_unique_content() {
    // The product's central case, stated by the person: the same weights sit in
    // two installs under different folder names.
    let w = TestWorld::new();
    let a = w.add_install("Production");
    let b = w.add_install("Normal");
    w.write_model(&a, "models/loras/awesomeloras/lora1.safetensors", &weights("lora1"));
    w.write_model(&b, "models/loras/newloras/lora1.safetensors", &weights("lora1"));

    let out = w.scan(&[a, b]);
    assert_eq!(out.record.totals.movable_files, 2);
    assert_eq!(out.record.totals.unique_contents, 1);
    assert_eq!(out.record.totals.duplicate_files, 1);
    assert_eq!(
        out.record.totals.reclaimable_bytes,
        weights("lora1").len() as u64,
        "one copy's worth of space is reclaimable"
    );
}

#[test]
fn the_same_content_under_two_different_names_is_still_one_content() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&a, "models/loras/my-favourite-lora.safetensors", &weights("same"));

    let out = w.scan(&[a]);
    assert_eq!(out.record.totals.unique_contents, 1);
    assert_eq!(out.record.totals.movable_files, 2);
}

#[test]
fn a_file_below_the_size_floor_is_ignored() {
    let w = TestWorld::new();
    let mut world_settings = w.settings.clone();
    world_settings.min_file_size_bytes = 1024;
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/tiny.safetensors", b"small");
    w.write_model(&i, "models/loras/big.safetensors", &weights("big"));

    let scanner = Scanner::new(&w.store, &w.platform, world_settings);
    let out = scanner
        .scan("s", &[i], &CancelToken::new(), &NullSink)
        .unwrap();

    assert!(find(&out, "big.safetensors").is_some());
    assert!(find(&out, "tiny.safetensors").is_none(), "a file under the floor must be skipped");
}

#[test]
fn a_file_whose_extension_is_not_a_weight_extension_is_ignored() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/notes.txt", &weights("notes"));
    w.write_model(&i, "models/loras/preview.png", &weights("png"));
    w.write_model(&i, "models/loras/real.safetensors", &weights("real"));

    let out = w.scan(&[i]);
    assert_eq!(out.entries.len(), 1, "found {:?}", paths(&out));
}

#[test]
fn every_default_weight_extension_is_found() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    for (n, ext) in crate::settings::DEFAULT_EXTENSIONS.iter().enumerate() {
        w.write_model(&i, &format!("models/loras/m{n}{ext}"), &weights(&format!("m{n}")));
    }
    let out = w.scan(&[i]);
    assert_eq!(out.entries.len(), crate::settings::DEFAULT_EXTENSIONS.len());
}

#[test]
fn the_category_comes_from_the_folder_under_models() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/checkpoints/sd.safetensors", &weights("ckpt"));
    w.write_model(&i, "models/loras/deep/nested/l.safetensors", &weights("lora"));
    w.write_model(&i, "models/loose.safetensors", &weights("loose"));

    let out = w.scan(&[i]);
    assert_eq!(entry(&out, "sd.safetensors").category, "checkpoints");
    assert_eq!(entry(&out, "l.safetensors").category, "loras");
    assert_eq!(
        entry(&out, "loose.safetensors").category, "misc",
        "a file lying loose in models belongs to no category"
    );
}

#[test]
fn weights_under_custom_nodes_are_counted_and_never_movable() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "custom_nodes/SomePack/ckpts/bundled.safetensors", &weights("bundled"));
    w.write_model(&i, "models/loras/normal.safetensors", &weights("normal"));

    let out = w.scan(&[i]);
    let bundled = entry(&out, "bundled.safetensors");
    assert_eq!(bundled.classification, Classification::CustomNodes);
    assert!(bundled.sha256.is_none(), "counted files are not read, only measured");

    assert_eq!(out.record.totals.custom_node_files, 1);
    assert_eq!(out.record.totals.custom_node_bytes, weights("bundled").len() as u64);
    assert_eq!(out.record.totals.movable_files, 1);
}

#[test]
fn an_extra_model_path_pointing_into_custom_nodes_is_still_counted_only() {
    // Some packs register their own weights folder through
    // extra_model_paths.yaml. Reaching a file by that route must not make it
    // movable: the pack also opens it by its own relative path, and moving it
    // breaks the pack.
    let w = TestWorld::new();
    let i = w.add_install("A");
    let inside = i.root.join("custom_nodes/AnimateDiff/models");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(inside.join("motion.safetensors"), weights("motion")).unwrap();
    let i = w.add_extra_model_path(&i, "animatediff_models", &inside);

    let out = w.scan(&[i]);
    let e = entry(&out, "motion.safetensors");
    assert_eq!(
        e.classification,
        Classification::CustomNodes,
        "a file inside custom_nodes must never be movable, whichever root found it"
    );
}

#[test]
fn a_folder_from_extra_model_paths_is_scanned_with_its_category() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    let shared = w.path().join("shared-loras");
    let i = w.add_extra_model_path(&i, "loras", &shared);
    std::fs::write(shared.join("shared.safetensors"), weights("shared")).unwrap();

    let out = w.scan(&[i]);
    let e = entry(&out, "shared.safetensors");
    assert_eq!(e.category, "loras");
    assert_eq!(e.classification, Classification::Movable);
}

#[test]
fn the_five_output_model_folders_are_scanned() {
    // ComfyUI registers these at startup, so weights saved by a workflow land
    // in a real model path. A scan of models/ alone would miss them.
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "output/loras/saved.safetensors", &weights("saved"));
    w.write_model(&i, "output/checkpoints/merged.safetensors", &weights("merged"));
    w.write_model(&i, "output/images/not-a-model.safetensors", &weights("notmodel"));

    let out = w.scan(&[i]);
    assert_eq!(entry(&out, "saved.safetensors").category, "loras");
    assert_eq!(entry(&out, "merged.safetensors").category, "checkpoints");
    assert!(
        find(&out, "not-a-model.safetensors").is_none(),
        "only the five registered output folders are model paths"
    );
}

#[test]
fn output_clip_is_reported_as_the_text_encoders_category() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "output/clip/t5.safetensors", &weights("t5"));
    let out = w.scan(&[i]);
    assert_eq!(entry(&out, "t5.safetensors").category, "text_encoders");
}

#[test]
fn output_folders_can_be_switched_off() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "output/loras/saved.safetensors", &weights("saved"));

    let settings = Settings { scan_output_model_dirs: false, min_file_size_bytes: 0, ..Default::default() };
    let out = Scanner::new(&w.store, &w.platform, settings)
        .scan("s", &[i], &CancelToken::new(), &NullSink)
        .unwrap();
    assert!(out.entries.is_empty());
}

#[test]
fn one_physical_file_reached_by_two_routes_is_counted_once() {
    // Without this, a plan could hold two rows that are one file, and applying
    // it would move the file and then try to link it to itself.
    let w = TestWorld::new();
    let i = w.add_install("A");
    let shared = w.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("one.safetensors"), weights("one")).unwrap();

    // Reach the same folder twice: once as an extra path, once through a
    // directory link inside models.
    let i = w.add_extra_model_path(&i, "loras", &shared);
    let link_dir = i.root.join("models/loras/linked");
    std::fs::create_dir_all(link_dir.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&shared, &link_dir).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&shared, &link_dir).unwrap();

    let out = w.scan(&[i]);
    let seen: Vec<&ScanEntryRecord> = out
        .entries
        .iter()
        .filter(|e| e.abs_path.file_name().unwrap() == "one.safetensors")
        .collect();
    assert_eq!(seen.len(), 1, "the same physical file was counted more than once");
}

#[test]
fn a_link_into_the_vault_is_recognized_as_already_consolidated() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    let vault_file = w.vault_root.join("loras/already.safetensors");
    std::fs::create_dir_all(vault_file.parent().unwrap()).unwrap();
    std::fs::write(&vault_file, weights("already")).unwrap();

    let at = i.root.join("models/loras/already.safetensors");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&vault_file, &at).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&vault_file, &at).unwrap();

    let out = w.scan(&[i]);
    let e = entry(&out, "already.safetensors");
    assert_eq!(e.classification, Classification::AlreadyInVault);
    assert!(e.link_target.is_some());
    assert_eq!(out.record.totals.already_linked_files, 1);
    assert_eq!(out.record.totals.movable_files, 0);
}

#[test]
fn a_link_pointing_outside_the_vault_is_reported_as_an_external_link() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    let elsewhere = w.write_file("elsewhere/e.safetensors", &weights("e"));

    let at = i.root.join("models/loras/e.safetensors");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, &at).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&elsewhere, &at).unwrap();

    let out = w.scan(&[i]);
    assert_eq!(entry(&out, "e.safetensors").classification, Classification::ExternalLink);
}

#[test]
fn the_vaults_own_folder_is_never_walked() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    // Make the vault's internal folder reachable from the install.
    let at = i.root.join("models/loras/internal");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(w.store.internal_dir(), &at).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(w.store.internal_dir(), &at).unwrap();

    let out = w.scan(&[i]);
    assert!(
        out.entries.iter().all(|e| !w.store.is_internal(&e.abs_path)),
        "the engine's own database folder must never be scanned"
    );
}

#[test]
fn the_hash_cache_prevents_a_second_read_of_an_unchanged_file() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/a.safetensors", &weights("a"));

    let first = w.scan(&[i.clone()]);
    assert!(first.record.totals.bytes_read > 0, "the first scan must read the file");
    assert_eq!(first.record.totals.bytes_from_cache, 0);

    let second = w.scan(&[i]);
    assert_eq!(second.record.totals.bytes_read, 0, "the second scan must read nothing");
    assert_eq!(second.record.totals.bytes_from_cache, weights("a").len() as u64);
    assert_eq!(
        second.entries[0].sha256, first.entries[0].sha256,
        "the cached hash must equal the computed one"
    );
}

#[test]
fn a_changed_file_is_read_again_and_gets_a_new_hash() {
    // The dangerous case: a stale cache row would make the engine move bytes it
    // has not looked at.
    let w = TestWorld::new();
    let i = w.add_install("A");
    let p = w.write_model(&i, "models/loras/a.safetensors", &weights("before"));

    let first = w.scan(&[i.clone()]);
    assert_eq!(first.entries[0].sha256.as_deref(), Some(weights_hash("before").as_str()));

    std::fs::write(&p, weights("after")).unwrap();
    // Force a different modification time, in case the two writes land inside
    // one filesystem tick.
    let f = std::fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3)).unwrap();
    drop(f);

    let second = w.scan(&[i]);
    assert_eq!(second.entries[0].sha256.as_deref(), Some(weights_hash("after").as_str()));
    assert!(second.record.totals.bytes_read > 0, "a changed file must be read again");
}

#[test]
fn a_file_whose_size_changed_is_read_again() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    let p = w.write_model(&i, "models/loras/a.safetensors", &weights("x"));
    w.scan(&[i.clone()]);

    let mut longer = weights("x");
    longer.extend_from_slice(b"more");
    std::fs::write(&p, &longer).unwrap();

    let second = w.scan(&[i]);
    assert!(second.record.totals.bytes_read > 0);
    assert_eq!(second.entries[0].sha256.as_deref(), Some(crate::scan::hash::hash_bytes(&longer).as_str()));
}

#[test]
fn switching_the_cache_off_forces_a_full_read() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/a.safetensors", &weights("a"));
    w.scan(&[i.clone()]);

    let settings = Settings { hash_cache_enabled: false, min_file_size_bytes: 0, ..Default::default() };
    let out = Scanner::new(&w.store, &w.platform, settings)
        .scan("s2", &[i], &CancelToken::new(), &NullSink)
        .unwrap();
    assert!(out.record.totals.bytes_read > 0);
    assert_eq!(out.record.totals.bytes_from_cache, 0);
}

#[test]
fn an_unreadable_file_is_recorded_and_the_scan_carries_on() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/good.safetensors", &weights("good"));
    let bad = w.write_model(&i, "models/loras/bad.safetensors", &weights("bad"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    // Running as root can read anything, so there would be nothing to prove.
    if std::fs::File::open(&bad).is_ok() {
        return;
    }

    let out = w.scan(&[i]);
    assert_eq!(entry(&out, "good.safetensors").classification, Classification::Movable);
    assert_eq!(
        entry(&out, "bad.safetensors").classification,
        Classification::Unreadable,
        "a file that cannot be read must not be planned as movable"
    );
    assert!(!out.record.errors.is_empty(), "the failure must be reported");
    assert!(out.record.totals.error_count > 0);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o644));
    }
}

#[test]
fn progress_is_reported_and_ends_with_the_final_totals() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    for n in 0..5 {
        w.write_model(&i, &format!("models/loras/m{n}.safetensors"), &weights(&format!("m{n}")));
    }

    let sink: RecordingSink<ScanProgress> = RecordingSink::new();
    let out = w
        .scanner()
        .scan("scan-1", &[i], &CancelToken::new(), &sink)
        .unwrap();

    let updates = sink.snapshot();
    assert!(!updates.is_empty(), "a scan must report progress");
    let last = updates.last().unwrap();
    assert_eq!(last.phase, ScanPhase::Finalizing, "the final update must not be dropped");
    assert_eq!(last.files_hashed, 5);
    assert_eq!(last.scan_id, "scan-1");
    assert_eq!(out.entries.len(), 5);
}

#[test]
fn a_cancelled_scan_stops_and_says_so() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    for n in 0..10 {
        w.write_model(&i, &format!("models/loras/m{n}.safetensors"), &weights(&format!("m{n}")));
    }

    let cancel = CancelToken::new();
    cancel.cancel();
    let out = w.scanner().scan("s", &[i], &cancel, &NullSink).unwrap();

    assert!(out.record.cancelled, "a cancelled scan must report that it was cancelled");
}

#[test]
fn a_cancelled_scan_changes_nothing_on_disk() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    let p = w.write_model(&i, "models/loras/m.safetensors", &weights("m"));
    let before = std::fs::read(&p).unwrap();

    let cancel = CancelToken::new();
    cancel.cancel();
    w.scanner().scan("s", &[i], &cancel, &NullSink).unwrap();

    assert_eq!(std::fs::read(&p).unwrap(), before);
    assert!(p.exists());
}

#[test]
fn per_install_totals_add_up_to_the_whole() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/one.safetensors", &weights("one"));
    w.write_model(&a, "models/loras/two.safetensors", &weights("two"));
    w.write_model(&b, "models/loras/three.safetensors", &weights("three"));

    let out = w.scan(&[a.clone(), b.clone()]);
    assert_eq!(out.record.per_install.len(), 2);
    let for_a = out.record.per_install.iter().find(|p| p.install_id == a.id).unwrap();
    let for_b = out.record.per_install.iter().find(|p| p.install_id == b.id).unwrap();
    assert_eq!(for_a.totals.movable_files, 2);
    assert_eq!(for_b.totals.movable_files, 1);
    assert_eq!(
        for_a.totals.movable_files + for_b.totals.movable_files,
        out.record.totals.movable_files
    );
}

#[test]
fn reclaimable_bytes_is_what_a_full_apply_would_return() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    // Three copies of one content, plus one file that exists once.
    for i in [&a, &b, &c] {
        w.write_model(i, "models/loras/shared.safetensors", &weights("shared"));
    }
    w.write_model(&a, "models/loras/unique.safetensors", &weights("unique"));

    let out = w.scan(&[a, b, c]);
    let size = weights("shared").len() as u64;
    assert_eq!(
        out.record.totals.reclaimable_bytes,
        size * 2,
        "three copies means two copies' worth of space comes back"
    );
    assert_eq!(out.record.totals.unique_contents, 2);
    assert_eq!(out.record.totals.duplicate_files, 2);
}

#[test]
fn an_empty_install_scans_to_nothing_without_failing() {
    let w = TestWorld::new();
    let i = w.add_install("Empty");
    let out = w.scan(&[i]);
    assert_eq!(out.entries.len(), 0);
    assert_eq!(out.record.totals.reclaimable_bytes, 0);
    assert!(out.record.errors.is_empty());
}

#[test]
fn a_missing_models_folder_does_not_stop_the_scan() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    std::fs::remove_dir_all(i.root.join("models")).unwrap();
    let out = w.scan(&[i]);
    assert_eq!(out.entries.len(), 0);
}

#[test]
fn the_category_helper_handles_the_shapes_it_meets() {
    use std::path::Path;
    assert_eq!(category_from_rel(Path::new("loras/x.safetensors")), "loras");
    assert_eq!(category_from_rel(Path::new("loras/deep/x.safetensors")), "loras");
    assert_eq!(category_from_rel(Path::new("x.safetensors")), "misc");
    assert_eq!(category_from_rel(Path::new("")), "misc");
}
