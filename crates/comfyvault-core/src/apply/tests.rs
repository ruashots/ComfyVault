//! Apply, revert and crash-recovery tests.
//!
//! Every one builds its own tree in a temporary folder. Several deliberately
//! break an apply halfway and then check that the disk holds exactly what it
//! held before, because that is the promise the whole product rests on.

use super::*;
use crate::progress::{NullSink, RecordingSink};
use crate::store::{JournalState, JournalStep};
use crate::testkit::{weights, weights_hash, TestWorld};

fn applier<'a>(w: &'a TestWorld) -> Applier<'a> {
    Applier::new(&w.store, &w.platform)
}

fn all_groups(plan: &ConsolidationPlan) -> Vec<String> {
    plan.groups.iter().map(|g| g.group_id.clone()).collect()
}

fn request(plan: &ConsolidationPlan) -> ApplyRequest {
    ApplyRequest {
        plan_id: plan.plan_id.clone(),
        group_ids: all_groups(plan),
        verify: VerifyModeArg::SizeAndMtime,
        stop_on_error: false,
    }
}

/// The installs on drive C:, the vault on drive D:. A rename between the two
/// is refused, as Windows refuses one.
fn vault_on_another_drive(w: &TestWorld) {
    w.platform.set_volume(w.path(), "C:\\");
    w.platform.set_volume(&w.vault_root, "D:\\");
}

fn run_apply(w: &TestWorld, plan: &ConsolidationPlan) -> ApplyRecord {
    applier(w)
        .apply("ap-1", plan, &request(plan), &CancelToken::new(), &NullSink)
        .expect("apply")
}

/// Counts the bytes of real model files, ignoring links.
///
/// This is the number the person's drive actually shows. The engine's own
/// folder is skipped: the vault database grows and shrinks as it is written,
/// and counting it would measure bookkeeping rather than the person's files.
fn real_bytes(w: &TestWorld) -> u64 {
    let internal = w.store.internal_dir();
    let mut total = 0;
    for e in walkdir::WalkDir::new(w.path())
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.path().starts_with(&internal))
        .flatten()
    {
        if e.file_type().is_file() {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

fn group_for<'a>(plan: &'a ConsolidationPlan, tag: &str) -> &'a PlanGroup {
    let sha = weights_hash(tag);
    plan.groups.iter().find(|g| g.sha256 == sha).expect("a group for that content")
}

// ---------------------------------------------------------------------------
// The happy path, checked against the disk rather than against the record
// ---------------------------------------------------------------------------

#[test]
fn two_copies_become_one_file_and_two_links_that_still_read() {
    let w = TestWorld::new();
    let a = w.add_install("Production");
    let b = w.add_install("Normal");
    let pa = w.write_model(&a, "models/loras/awesomeloras/lora1.safetensors", &weights("lora1"));
    let pb = w.write_model(&b, "models/loras/newloras/lora1.safetensors", &weights("lora1"));

    let plan = w.plan(&[a, b]);
    let result = run_apply(&w, &plan);

    assert_eq!(result.state, ApplyState::Completed);
    assert_eq!(result.groups_applied, 1);
    assert_eq!(result.files_moved, 1);
    assert_eq!(result.links_created, 2);

    let vault_file = w.vault_root.join("loras/lora1.safetensors");
    assert!(vault_file.is_file());
    assert!(!w.is_link(&vault_file), "the vault holds the real file");

    // Both old places are now links, and both still read the weights. This is
    // the whole promise: ComfyUI keeps working.
    assert!(w.is_link(&pa));
    assert!(w.is_link(&pb));
    assert_eq!(w.read(&pa), weights("lora1"));
    assert_eq!(w.read(&pb), weights("lora1"));
    assert_eq!(w.link_target(&pa).unwrap(), vault_file);
    assert_eq!(w.link_target(&pb).unwrap(), vault_file);
}

#[test]
fn the_space_actually_comes_back_on_the_disk() {
    // Not the reported number: the bytes a person's drive really holds.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    for i in [&a, &b, &c] {
        w.write_model(i, "models/loras/shared.safetensors", &weights("shared"));
    }
    let size = weights("shared").len() as u64;

    let before = real_bytes(&w);
    let plan = w.plan(&[a, b, c]);
    let result = run_apply(&w, &plan);
    let after = real_bytes(&w);

    assert_eq!(result.bytes_freed, size * 2);
    assert!(
        before - after >= size * 2,
        "expected at least {} bytes to come back, got {}",
        size * 2,
        before - after
    );
}

#[test]
fn a_link_keeps_its_own_name_when_the_vault_name_differs() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    let pb = w.write_model(&b, "models/loras/my-favourite.safetensors", &weights("same"));

    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    assert!(w.is_link(&pb));
    assert_eq!(
        pb.file_name().unwrap(),
        "my-favourite.safetensors",
        "the install's own name must survive"
    );
    assert_eq!(w.read(&pb), weights("same"));
}

#[test]
fn a_second_name_appears_inside_the_vault_as_a_link() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/my-favourite.safetensors", &weights("same"));

    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    let alias = w.vault_root.join("loras/my-favourite.safetensors");
    assert!(w.is_link(&alias), "the vault must show every name the content was known by");
    assert_eq!(w.read(&alias), weights("same"));

    let record = w.store.vault_file(&weights_hash("same")).unwrap().unwrap();
    assert_eq!(record.canonical_name, "lora1.safetensors");
    assert_eq!(record.aliases, vec!["my-favourite.safetensors"]);
}

#[test]
fn two_different_contents_with_one_name_both_end_up_in_the_vault() {
    let w = TestWorld::new();
    let installs: Vec<_> = (0..3).map(|n| w.add_install(&format!("I{n}"))).collect();
    w.write_model(&installs[0], "models/loras/lora1.safetensors", &weights("rare"));
    let common_paths: Vec<_> = installs[1..3]
        .iter()
        .map(|i| w.write_model(i, "models/loras/lora1.safetensors", &weights("common")))
        .collect();

    let plan = w.plan(&installs);
    run_apply(&w, &plan);

    let plain = w.vault_root.join("loras/lora1.safetensors");
    assert!(plain.is_file());
    assert_eq!(std::fs::read(&plain).unwrap(), weights("common"));

    // The rarer content kept its bytes under an adjusted name. Nothing was
    // overwritten and nothing was lost.
    let rare_group = group_for(&plan, "rare");
    let rare_path = w.vault_root.join(&rare_group.vault_rel_path);
    assert!(rare_path.is_file(), "the second content must survive too");
    assert_eq!(std::fs::read(&rare_path).unwrap(), weights("rare"));
    assert_ne!(rare_path, plain);

    for p in common_paths {
        assert_eq!(w.read(&p), weights("common"));
    }
}

#[test]
fn a_single_copy_moves_into_the_vault_and_leaves_a_link() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/only.safetensors", &weights("only"));

    let plan = w.plan(&[a]);
    let result = run_apply(&w, &plan);

    assert_eq!(result.bytes_freed, 0, "one copy frees nothing");
    assert_eq!(result.files_moved, 1);
    assert!(w.is_link(&p));
    assert_eq!(w.read(&p), weights("only"));
    assert!(w.vault_root.join("loras/only.safetensors").is_file());
}

#[test]
fn only_the_groups_the_caller_named_are_applied() {
    // Apply must never do more than the person ticked.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["keep", "leave"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }

    let plan = w.plan(&[a.clone(), b]);
    let keep = group_for(&plan, "keep").group_id.clone();

    let result = applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: vec![keep],
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();

    assert_eq!(result.groups_applied, 1);
    assert!(w.vault_root.join("loras/keep.safetensors").is_file());
    assert!(
        !w.vault_root.join("loras/leave.safetensors").exists(),
        "an unticked row must not be touched"
    );
    let untouched = a.root.join("models/loras/leave.safetensors");
    assert!(!w.is_link(&untouched), "an unticked file must still be a real file");
}

#[test]
fn an_empty_selection_does_nothing_at_all() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    let result = applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: vec![],
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();

    assert_eq!(result.groups_applied, 0);
    assert!(!w.is_link(&p));
    assert_eq!(w.read(&p), weights("m"));
}

// ---------------------------------------------------------------------------
// Checks that stop a row
// ---------------------------------------------------------------------------

#[test]
fn a_file_changed_between_the_plan_and_the_apply_stops_that_row() {
    // The engine must never move a file it has not just looked at.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);

    // Somebody replaces one copy after the plan was read.
    std::fs::write(&pb, weights("replaced")).unwrap();
    let f = std::fs::File::options().write(true).open(&pb).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    drop(f);

    let result = run_apply(&w, &plan);

    assert_eq!(result.state, ApplyState::CompletedWithErrors);
    assert_eq!(result.groups_applied, 0);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].reason, BlockReason::FileChanged);

    // Nothing moved at all: the group is all or nothing.
    assert_eq!(std::fs::read(&pb).unwrap(), weights("replaced"));
    assert!(!w.vault_root.join("loras/m.safetensors").exists());
}

#[test]
fn a_locked_file_stops_that_row_and_nothing_moves() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    // ComfyUI starts up between the plan and the apply and loads the model.
    w.platform.lock_file(pb.clone());

    let result = run_apply(&w, &plan);

    assert_eq!(result.groups_applied, 0);
    assert_eq!(result.failures[0].reason, BlockReason::FileLocked);
    assert!(!w.is_link(&pa), "the other copy must not be touched either");
    assert_eq!(w.read(&pa), weights("m"));
    assert_eq!(w.read(&pb), weights("m"));
}

#[test]
fn a_file_deleted_between_the_plan_and_the_apply_stops_that_row() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    std::fs::remove_file(&pb).unwrap();

    let result = run_apply(&w, &plan);
    assert_eq!(result.failures[0].reason, BlockReason::FileMissing);
    assert_eq!(w.read(&pa), weights("m"), "the surviving copy is untouched");
}

#[test]
fn rehash_catches_an_edit_that_kept_the_size_and_the_time() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    let (size, mtime) = {
        let m = std::fs::metadata(&pb).unwrap();
        (m.len(), crate::time_util::Timestamp::mtime_nanos(&m))
    };

    let mut tampered = weights("m");
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    std::fs::write(&pb, &tampered).unwrap();
    let f = std::fs::File::options().write(true).open(&pb).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(mtime as u64)).unwrap();
    drop(f);
    assert_eq!(std::fs::metadata(&pb).unwrap().len(), size);

    let result = applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: all_groups(&plan),
                verify: VerifyModeArg::Rehash,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();

    assert_eq!(result.groups_applied, 0, "rehash must catch what size and time cannot");
    assert_eq!(result.failures[0].reason, BlockReason::FileChanged);
}

#[test]
fn an_apply_without_symlink_support_refuses_before_it_touches_anything() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    w.platform.set_symlinks_unsupported(true);
    let err = applier(&w)
        .apply("ap-1", &plan, &request(&plan), &CancelToken::new(), &NullSink)
        .unwrap_err();

    assert_eq!(err.code, ErrorCode::SymlinkUnsupported);
    assert!(err.message.contains("Developer Mode"));
    assert_eq!(w.read(&p), weights("m"));
}

#[test]
fn something_already_sitting_at_the_vault_path_stops_the_row_without_overwriting() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    let occupied = w.vault_root.join("loras/m.safetensors");
    std::fs::create_dir_all(occupied.parent().unwrap()).unwrap();
    std::fs::write(&occupied, b"someone else's file").unwrap();

    let result = run_apply(&w, &plan);
    assert_eq!(result.groups_applied, 0);
    assert_eq!(
        std::fs::read(&occupied).unwrap(),
        b"someone else's file",
        "a file already in the vault must never be overwritten"
    );
}

// ---------------------------------------------------------------------------
// A group is all or nothing
// ---------------------------------------------------------------------------

#[test]
fn a_failure_halfway_through_a_group_puts_everything_back() {
    // The link for the second copy fails. The first copy has already moved into
    // the vault and been linked, so all of that has to be undone.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "m");
    let failing = g.links[0].abs_path.clone();
    w.platform.fail_symlink_at(
        failing.clone(),
        VaultError::new(ErrorCode::IoError, "the drive refused this link"),
    );

    let result = run_apply(&w, &plan);
    assert_eq!(result.groups_applied, 0);
    assert_eq!(result.groups_failed, 1);

    // Both files are real files again, with their original bytes, in their
    // original places.
    for p in [&pa, &pb] {
        assert!(!w.is_link(p), "{} is still a link", p.display());
        assert_eq!(w.read(p), weights("m"), "{} lost its bytes", p.display());
    }
    assert!(
        !w.vault_root.join("loras/m.safetensors").exists(),
        "the vault must not keep a file from a group that failed"
    );
    assert!(w.store.vault_file(&weights_hash("m")).unwrap().is_none());
}

#[test]
fn a_failure_on_the_very_first_link_puts_the_moved_file_back() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    w.platform.fail_symlink_at(p.clone(), VaultError::new(ErrorCode::IoError, "refused"));

    let result = run_apply(&w, &plan);
    assert_eq!(result.groups_applied, 0);
    assert!(p.is_file(), "the file must be back where it started");
    assert!(!w.is_link(&p));
    assert_eq!(w.read(&p), weights("m"));
    assert!(!w.vault_root.join("loras/m.safetensors").exists());
}

#[test]
fn one_group_failing_does_not_disturb_another() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let good_a = w.write_model(&a, "models/loras/good.safetensors", &weights("good"));
    let good_b = w.write_model(&b, "models/loras/good.safetensors", &weights("good"));
    w.write_model(&a, "models/loras/bad.safetensors", &weights("bad"));
    let bad_b = w.write_model(&b, "models/loras/bad.safetensors", &weights("bad"));

    let plan = w.plan(&[a, b]);
    w.platform.fail_symlink_at(bad_b.clone(), VaultError::new(ErrorCode::IoError, "refused"));

    let result = run_apply(&w, &plan);
    assert_eq!(result.groups_applied, 1);
    assert_eq!(result.groups_failed, 1);
    assert_eq!(result.state, ApplyState::CompletedWithErrors);

    assert!(w.is_link(&good_a) && w.is_link(&good_b));
    assert_eq!(w.read(&good_a), weights("good"));
    assert!(!w.is_link(&bad_b));
    assert_eq!(w.read(&bad_b), weights("bad"));
}

#[test]
fn stopping_on_the_first_error_leaves_the_later_groups_untouched() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two", "three"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);

    // Break the first group the run will reach.
    let first = plan.groups[0].links[0].abs_path.clone();
    w.platform.fail_symlink_at(first, VaultError::new(ErrorCode::IoError, "refused"));

    let result = applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: all_groups(&plan),
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: true,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();

    assert_eq!(result.groups_applied, 0);
    assert_eq!(result.groups_failed, 1);
}

// ---------------------------------------------------------------------------
// A vault on another drive
// ---------------------------------------------------------------------------

#[test]
fn a_vault_on_another_drive_copies_checks_then_removes_the_original() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    // Every rename across the boundary is refused, exactly as Windows does
    // between two drives.
    vault_on_another_drive(&w);

    let result = run_apply(&w, &plan);
    assert_eq!(result.state, ApplyState::Completed);

    let vault_file = w.vault_root.join("loras/m.safetensors");
    assert!(vault_file.is_file());
    assert_eq!(std::fs::read(&vault_file).unwrap(), weights("m"));
    assert!(w.is_link(&pa) && w.is_link(&pb));
    assert_eq!(w.read(&pa), weights("m"));

    // No half-written copy left in the vault's own folder.
    let leftovers: Vec<_> = std::fs::read_dir(w.store.temp_dir()).unwrap().flatten().collect();
    assert!(leftovers.is_empty(), "a part file was left behind: {leftovers:?}");
}

#[test]
fn a_finished_run_reports_the_drive_twice_and_never_the_arithmetic() {
    // The screen where a person checks whether the product did what it said.
    // Both figures are read off the drive, one before the first file moved and
    // one after the last one. Neither is the other plus bytesFreed. The two
    // can honestly disagree with that sum: something else on the computer may
    // write during the run, and sparse files never occupied what they claimed.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);

    // Two readings that no arithmetic on bytesFreed could connect.
    const BEFORE: u64 = 22_000_000_000;
    const AFTER: u64 = 92_000_000_000;
    w.platform.set_free_bytes(BEFORE);

    // The drive changes while the run is going, the way a real one does.
    let platform = &w.platform;
    let sink = move |_: &ApplyProgress| {
        platform.set_free_bytes(AFTER);
    };

    let record = applier(&w)
        .apply("ap-1", &plan, &request(&plan), &CancelToken::new(), &sink)
        .unwrap();

    assert_eq!(record.vault_free_bytes_before, Some(BEFORE), "the first reading was not kept");
    assert_eq!(record.vault_free_bytes_after, Some(AFTER), "the second reading was not taken");
    assert!(record.bytes_freed > 0, "this run did free something");
    assert_ne!(
        record.vault_free_bytes_after,
        record.vault_free_bytes_before.map(|b| b + record.bytes_freed),
        "the second figure is the first plus bytesFreed, which means it was computed"
    );
}

#[test]
fn a_resumed_run_keeps_the_reading_from_before_the_work_began() {
    // "Before" means before the work, not before this attempt at it. A resumed
    // pass that read the drive again would report the space its own first pass
    // had already freed as if it had always been there.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);
    let req = request(&plan);

    const BEFORE: u64 = 10_000_000_000;
    w.platform.set_free_bytes(BEFORE);

    // Stop after the first group.
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let stop = move |p: &ApplyProgress| {
        if p.group_index >= 1 {
            trigger.cancel();
        }
    };
    let first = applier(&w).apply("ap-1", &plan, &req, &cancel, &stop).unwrap();
    assert_eq!(first.vault_free_bytes_before, Some(BEFORE));
    // Then the power goes before the record is closed: only a run cut off
    // part way can be finished.
    w.store.put_apply(&ApplyRecord { state: ApplyState::Running, finished_at: None, ..first }).unwrap();

    // The drive now reads differently, because the first pass freed something.
    w.platform.set_free_bytes(30_000_000_000);
    let resumed = applier(&w)
        .resume("ap-1", &CancelToken::new(), &NullSink)
        .unwrap();

    assert_eq!(
        resumed.vault_free_bytes_before,
        Some(BEFORE),
        "the resumed run replaced the original reading with a later one"
    );
    assert_eq!(resumed.vault_free_bytes_after, Some(30_000_000_000));
}

// ---------------------------------------------------------------------------
// Cancelling
// ---------------------------------------------------------------------------

#[test]
fn cancelling_stops_between_groups_and_keeps_what_is_done() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);

    let cancel = CancelToken::new();
    cancel.cancel();
    let result = applier(&w)
        .apply("ap-1", &plan, &request(&plan), &cancel, &NullSink)
        .unwrap();

    assert_eq!(result.state, ApplyState::Cancelled);
    assert_eq!(result.groups_applied, 0, "cancelled before the first group started");
}

#[test]
fn a_failure_that_really_happened_survives_the_person_pressing_stop() {
    // A group the cancel interrupted is not a failure, because the person
    // stopped it. A group that broke on its own, earlier in the same run, is
    // one, and it must still be named. Otherwise a file that was left alone
    // is reported nowhere, which is the one thing the result screen exists
    // to prevent.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two", "three"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);

    // Break the first group for a real reason, not a cancel.
    let doomed = group_for(&plan, "one");
    let victim = doomed.links.iter().find(|l| !l.is_source).expect("a link to make");
    w.platform.fail_symlink_at(
        victim.abs_path.clone(),
        VaultError::new(ErrorCode::PermissionDenied, "Windows refused to create this link."),
    );

    // The order is the order asked for, so "one" runs first.
    let req = ApplyRequest {
        plan_id: plan.plan_id.clone(),
        group_ids: vec![
            doomed.group_id.clone(),
            group_for(&plan, "two").group_id.clone(),
            group_for(&plan, "three").group_id.clone(),
        ],
        verify: VerifyModeArg::SizeAndMtime,
        stop_on_error: false,
    };

    // Stop once the run reaches the third group. The first has already failed
    // and the second has already finished.
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let sink = move |p: &ApplyProgress| {
        if p.group_index >= 2 {
            trigger.cancel();
        }
    };

    let result = applier(&w).apply("ap-1", &plan, &req, &cancel, &sink).unwrap();

    assert_eq!(result.state, ApplyState::Cancelled, "the person stopped it");
    assert_eq!(result.groups_applied, 1, "the second group finished before the stop");
    assert_eq!(
        result.failures.len(),
        1,
        "the group that broke on its own is missing from the result: {:?}",
        result.failures
    );
    assert_eq!(result.failures[0].group_id, doomed.group_id);
    assert_eq!(result.failures[0].reason, BlockReason::PermissionDenied);
    assert_eq!(result.groups_failed, 1);
}

// ---------------------------------------------------------------------------
// Revert
// ---------------------------------------------------------------------------

#[test]
fn reverting_puts_every_file_back_exactly_as_it_was() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    let paths: Vec<_> = [&a, &b, &c]
        .iter()
        .map(|i| w.write_model(i, "models/loras/m.safetensors", &weights("m")))
        .collect();

    let before = real_bytes(&w);
    let plan = w.plan(&[a, b, c]);
    run_apply(&w, &plan);

    let record = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.state, ApplyState::Reverted);

    for p in &paths {
        assert!(p.is_file(), "{} is missing after the revert", p.display());
        assert!(!w.is_link(p), "{} is still a link", p.display());
        assert_eq!(std::fs::read(p).unwrap(), weights("m"));
    }
    assert!(!w.vault_root.join("loras/m.safetensors").exists());
    assert_eq!(real_bytes(&w), before, "the disk must look exactly as it did");
}

#[test]
fn reverting_a_cross_drive_apply_also_puts_everything_back() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    vault_on_another_drive(&w);
    run_apply(&w, &plan);

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();

    for p in [&pa, &pb] {
        assert!(!w.is_link(p));
        assert_eq!(std::fs::read(p).unwrap(), weights("m"));
    }
}

#[test]
fn reverting_forgets_the_vault_file_and_the_links() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);
    assert_eq!(w.store.links().unwrap().len(), 2);

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(w.store.vault_file(&weights_hash("m")).unwrap().is_none());
    assert!(w.store.links().unwrap().is_empty(), "a reverted run leaves no link records");
}

#[test]
fn reverting_twice_is_refused_rather_than_doing_damage() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
}

#[test]
fn reverting_a_run_that_does_not_exist_says_so() {
    let w = TestWorld::new();
    let err = applier(&w).revert("never-happened", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn a_revert_refuses_when_the_drive_has_no_room_to_put_the_files_back() {
    // Better to refuse than to stop halfway through putting things back.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    w.platform.set_free_bytes(1);
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::IoError);
    assert!(err.message.contains("not enough room"));
}

fn set_time(p: &Path, secs: u64) -> i128 {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    crate::time_util::Timestamp::mtime_nanos(&std::fs::metadata(p).unwrap())
}

fn time_of(p: &Path) -> i128 {
    crate::time_util::Timestamp::mtime_nanos(&std::fs::metadata(p).unwrap())
}

#[test]
fn every_file_put_back_carries_the_time_it_had() {
    // The kept copy comes back by rename and keeps its time. The duplicates
    // come back as copies, and each must carry its own time, not the kept
    // copy's and not the time of the undo. The scan cache trusts size and
    // time, so a new time costs a full read of the file on the next scan.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    let paths: Vec<_> = [&a, &b, &c]
        .iter()
        .map(|i| w.write_model(i, "models/loras/m.safetensors", &weights("m")))
        .collect();
    let times: Vec<i128> = paths
        .iter()
        .enumerate()
        .map(|(n, p)| set_time(p, 1_600_000_000 + n as u64 * 86_400))
        .collect();

    let plan = w.plan(&[a, b, c]);
    run_apply(&w, &plan);
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();

    let after: Vec<i128> = paths.iter().map(|p| time_of(p)).collect();
    assert_eq!(after, times);
}

#[test]
fn a_journal_from_before_times_were_recorded_still_reads() {
    // Written by an older build: the delete step has no time in it.
    let json = r#"{"kind":"deleteStash","stash":"/i/b.old","original":"/i/b","vault_path":"/v/a","sha256":"AA","size_bytes":10}"#;
    let step: JournalStep = serde_json::from_str(json).unwrap();
    assert!(matches!(step, JournalStep::DeleteStash { mtime_nanos: None, .. }));
}

#[test]
fn an_undo_reports_what_it_puts_back_while_it_runs() {
    // Measured before this: an undo sent the apply payload with every counter
    // at zero, and nothing at all while a file was being copied.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    let paths: Vec<_> = [&a, &b, &c]
        .iter()
        .map(|i| w.write_model(i, "models/loras/m.safetensors", &weights("m")))
        .collect();
    let plan = w.plan(&[a, b, c]);
    run_apply(&w, &plan);

    let sink: RecordingSink<RevertProgress> = RecordingSink::new();
    applier(&w).revert("ap-1", &CancelToken::new(), &sink).unwrap();
    let updates = sink.snapshot();

    let size = weights("m").len() as u64;
    let first = updates.first().expect("an update");
    assert_eq!(first.files_to_put_back, 3, "the kept copy and two duplicates");
    assert_eq!(first.bytes_to_copy, 2 * size, "only the duplicates are copied");
    // The first step undone is the last duplicate's delete, so the first
    // thing the person sees is that file being copied back.
    assert_eq!(first.action, RevertAction::CopyingBack);
    // Compared as paths, which on Windows treat / and \ as one separator.
    assert_eq!(first.current_path.as_deref().map(PathBuf::from), Some(paths[2].clone()));

    let last = updates.last().unwrap();
    assert_eq!(last.phase, RevertPhase::Finalizing);
    assert_eq!(last.files_put_back, 3);
    assert_eq!(last.links_removed, 3, "one link in each install");
    assert_eq!(last.bytes_copied, 2 * size);
    assert_eq!(last.step_index, last.step_total);

    let copied: Vec<u64> = updates.iter().map(|u| u.bytes_copied).collect();
    assert!(copied.windows(2).all(|p| p[0] <= p[1]), "bytes copied went backwards: {copied:?}");
}

#[test]
fn the_cost_of_an_undo_is_known_before_it_starts() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    for i in [&a, &b, &c] {
        w.write_model(i, "models/loras/m.safetensors", &weights("m"));
    }
    let plan = w.plan(&[a, b, c]);
    run_apply(&w, &plan);
    w.platform.set_free_bytes(5_000_000);

    let preview = applier(&w).preview_revert("ap-1").unwrap();
    let size = weights("m").len() as u64;
    assert_eq!(preview.files_renamed_back, 1, "the kept copy comes back by rename");
    assert_eq!(preview.files_copied_back, 2, "each duplicate is a copy of the vault file");
    assert_eq!(preview.bytes_to_copy, 2 * size);
    assert_eq!(preview.drives.len(), 1);
    let vault_on_disk =
        crate::platform::size_on_disk(&w.vault_root.join("loras/m.safetensors")).unwrap();
    assert_eq!(preview.drives[0].predicted_room_bytes, 2 * vault_on_disk);
    assert_eq!(preview.drives[0].free_bytes, Some(5_000_000));

    // Nothing moved.
    assert!(w.store.apply("ap-1").unwrap().unwrap().revertible);
    assert!(w.vault_root.join("loras/m.safetensors").is_file());
}

#[test]
fn the_preview_refuses_whatever_the_undo_would_refuse() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();

    let err = applier(&w).preview_revert("ap-1").unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(applier(&w).preview_revert("nope").unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn an_unreadable_drive_is_previewed_as_unknown_not_as_empty() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    w.platform.fail_disk_space(true);
    let preview = applier(&w).preview_revert("ap-1").unwrap();
    assert_eq!(preview.drives[0].free_bytes, None);
}

#[test]
fn sparse_models_need_only_the_room_they_really_take() {
    // A sparse model's size is not what it occupies. Checking the size would
    // refuse an undo that fits, and state a cost the drive never pays.
    const LEN: u64 = 16 * 1024 * 1024;
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = a.root.join("models/checkpoints/m.safetensors");
    let pb = b.root.join("models/checkpoints/m.safetensors");
    crate::testkit::write_sparse(&pa, "m", LEN);
    crate::testkit::write_sparse(&pb, "m", LEN);
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    let preview = applier(&w).preview_revert("ap-1").unwrap();
    assert_eq!(preview.bytes_to_copy, LEN);
    assert!(
        preview.drives[0].predicted_room_bytes < LEN / 4,
        "predicted {} bytes for a file that occupies almost nothing",
        preview.drives[0].predicted_room_bytes
    );

    // Less room than the size, more than the file takes.
    w.platform.set_free_bytes(LEN / 2);
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(crate::platform::size_on_disk(&pb).unwrap() < LEN / 4, "the duplicate came back solid");
}

/// Every place a model was found must load: a real file, or a link that
/// resolves, holding the model's bytes. An empty place is a model ComfyUI lists
/// and then cannot load.
fn every_place_loads(paths: &[PathBuf], content: &[u8], when: &str) {
    for p in paths {
        assert!(
            std::fs::symlink_metadata(p).is_ok(),
            "{when}: {} holds neither the file nor a link",
            p.display()
        );
        let bytes = std::fs::read(p)
            .unwrap_or_else(|e| panic!("{when}: {} does not load: {e}", p.display()));
        assert!(bytes == content, "{when}: {} loads the wrong bytes", p.display());
    }
}

#[test]
fn an_undo_stopped_at_any_point_leaves_every_model_loadable() {
    // Measured before this: a Stop during a copy back out of the vault left
    // that model's place empty, because the link was removed before the copy
    // began. Here the undo is stopped at every point it can be stopped, one
    // run after another, on one drive and with the vault on another.
    let mut content = weights("big");
    content.resize(3 * 1024 * 1024 + 17, 0x5A);

    for vault_elsewhere in [false, true] {
        let mut stops = 0;
        for n in 1.. {
            let w = TestWorld::new();
            let installs: Vec<_> = ["A", "B", "C"].iter().map(|l| w.add_install(l)).collect();
            // The third copy has another name, so the vault also carries a
            // second name for the model, as a link beside the vault file.
            let paths: Vec<PathBuf> = installs
                .iter()
                .zip(["m", "m", "other"])
                .map(|(i, name)| {
                    w.write_model(i, &format!("models/checkpoints/{name}.safetensors"), &content)
                })
                .collect();
            let plan = w.plan(&installs);
            if vault_elsewhere {
                vault_on_another_drive(&w);
            }
            run_apply(&w, &plan);
            let finished_at = w.store.apply("ap-1").unwrap().unwrap().finished_at;
            assert!(
                !w.store.vault_files().unwrap()[0].aliases.is_empty(),
                "the vault must carry a second name for this test to mean anything"
            );
            let when = format!("vault elsewhere {vault_elsewhere}, stopped at check {n}");

            let undo_began = crate::time_util::Timestamp::now();
            let result = applier(&w).revert("ap-1", &CancelToken::stopping_at_check(n), &NullSink);
            every_place_loads(&paths, &content, &when);

            // The time of the last undo step moves exactly when a step began.
            let stamp = w.store.apply("ap-1").unwrap().unwrap().last_undo_step_at;
            let stepped = w.store.journal("ap-1").unwrap().iter().any(|e| e.state == JournalState::Undone);
            match stamp {
                None => {
                    assert!(!stepped, "{when}: a step was undone and no time was recorded");
                    assert!(paths.iter().all(|p| w.is_link(p)), "{when}: a file came back with no time recorded");
                }
                Some(t) => assert!(
                    t >= undo_began && t <= crate::time_util::Timestamp::now(),
                    "{when}: the recorded time is not the undo's"
                ),
            }

            let err = match result {
                Ok(_) => break,
                Err(e) => e,
            };
            stops += 1;
            assert_eq!(err.code, ErrorCode::Cancelled, "{when}");
            let record = w.store.apply("ap-1").unwrap().unwrap();
            assert_eq!(record.state, ApplyState::PartlyReverted, "{when}");
            assert!(record.revertible, "{when}");
            assert_eq!(record.finished_at, finished_at, "{when}: a stopped undo rewrote when the run finished");

            // What the vault screen says about a partly undone run must be
            // true: every model loads, so nothing may be reported broken.
            let health = crate::vault::Vault::new(&w.store, &w.platform).health().unwrap();
            assert!(
                health.ok && health.foreign_files.is_empty(),
                "{when}: the health check reports trouble on a tree where every model loads: {health:?}"
            );

            let back = paths
                .iter()
                .filter(|p| std::fs::symlink_metadata(p).unwrap().file_type().is_file())
                .count() as u64;
            let preview = applier(&w).preview_revert("ap-1").unwrap();
            assert_eq!(preview.files_already_back, back, "{when}");
            assert_eq!(
                preview.files_already_back + preview.files_renamed_back + preview.files_copied_back,
                3,
                "{when}: already back and still to come must add up to every place"
            );

            // Undoing again finishes the job, and moves the time on.
            let again = crate::time_util::Timestamp::now();
            applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
            let last = w.store.apply("ap-1").unwrap().unwrap().last_undo_step_at.expect("a time");
            assert!(last >= again, "{when}: finishing the undo did not move the time");
            every_place_loads(&paths, &content, &format!("{when}, then undone again"));
            for p in &paths {
                assert!(!w.is_link(p), "{when}: {} is still a link after the second undo", p.display());
            }
            assert!(w.store.links().unwrap().is_empty(), "{when}: link records left behind");
            assert_eq!(w.store.apply("ap-1").unwrap().unwrap().state, ApplyState::Reverted);
        }
        assert!(stops > 10, "the undo was stopped only {stops} times, the test proves little");
    }
}

#[test]
fn the_link_left_for_the_kept_copy_is_never_taken_for_the_file() {
    // The kept copy's link now stays until the vault file is renamed over it.
    // The step that moves it back used to count anything at that path as the
    // original still being there, and then deleted the vault file: the only
    // copy. Stopping right after the link's own step is where that bit.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);

    // The undo is: remove the link (kept), then move the file back. Stop in
    // between, then finish.
    let _ = applier(&w).revert("ap-1", &CancelToken::stopping_at_check(2), &NullSink);
    assert!(w.is_link(&pa), "the link must still stand in for the file");
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(!w.is_link(&pa));
    assert_eq!(std::fs::read(&pa).unwrap(), weights("m"));
}

// ---------------------------------------------------------------------------
// Crash recovery
//
// Each of these reconstructs the exact disk and journal state a crash leaves at
// one particular moment, then proves the engine recovers from it.
// ---------------------------------------------------------------------------

/// Marks the run as still running and drops the journal entries after `keep`,
/// which is what the disk and the database look like when the process dies
/// partway through.
fn simulate_crash_after(w: &TestWorld, apply_id: &str, keep: usize) {
    let mut record = w.store.apply(apply_id).unwrap().unwrap();
    record.state = ApplyState::Running;
    record.finished_at = None;
    w.store.put_apply(&record).unwrap();

    for e in w.store.journal(apply_id).unwrap().into_iter().skip(keep) {
        // The step after the crash point was written but never confirmed.
        let pending = JournalEntry { state: JournalState::Pending, finished_at: None, ..e };
        w.store.update_journal(&pending).unwrap();
    }
}

#[test]
fn a_crash_between_moving_the_file_and_linking_it_is_recoverable() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    // Reproduce the disk exactly as a crash there leaves it: the file is in the
    // vault, its old place is empty, and the link step was never confirmed.
    let vault_dir = w.vault_root.join("loras");
    std::fs::create_dir_all(&vault_dir).unwrap();
    let vault_file = vault_dir.join("m.safetensors");
    std::fs::rename(&p, &vault_file).unwrap();

    let ap = "ap-crash";
    w.store
        .put_apply(&ApplyRecord {
            apply_id: ap.into(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            groups_requested: 1,
            group_ids: vec![plan.groups[0].group_id.clone()],
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            vault_free_bytes_before: None,
            vault_free_bytes_after: None,
            failures: vec![],
            revertible: true,
            last_undo_step_at: None,
        })
        .unwrap();
    let g = &plan.groups[0];
    w.store
        .append_journal(&JournalEntry {
            apply_id: ap.into(),
            seq: 0,
            group_id: g.group_id.clone(),
            step: JournalStep::MoveToVault {
                from: p.clone(),
                to: vault_file.clone(),
                copied: false,
                sha256: g.sha256.clone(),
                size_bytes: g.size_bytes,
            },
            state: JournalState::Done,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            error: None,
        })
        .unwrap();
    w.store
        .append_journal(&JournalEntry {
            apply_id: ap.into(),
            seq: 1,
            group_id: g.group_id.clone(),
            step: JournalStep::CreateLink { link: p.clone(), target: vault_file.clone() },
            state: JournalState::Pending,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            error: None,
        })
        .unwrap();

    // The engine notices on startup.
    let found = applier(&w).interrupted().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].apply_id, ap);
    assert_eq!(found[0].steps_done, 1);
    assert_eq!(found[0].steps_pending, 1);
    assert!(found[0].description.contains("Nothing was lost"));

    applier(&w).revert(ap, &CancelToken::new(), &NullSink).unwrap();

    assert!(p.is_file(), "the file must be back where it started");
    assert!(!w.is_link(&p));
    assert_eq!(std::fs::read(&p).unwrap(), weights("m"));
    assert!(!vault_file.exists());
}

#[test]
fn a_crash_after_setting_a_duplicate_aside_is_recoverable() {
    // The worst looking moment: the old file is under a different name and its
    // place is empty. The bytes are still there, and the revert finds them.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);

    let g = &plan.groups[0];
    let stash_path = pb.with_file_name("m.safetensors.comfyvault-old-apcrash");
    std::fs::rename(&pb, &stash_path).unwrap();

    let ap = "ap-crash";
    w.store
        .put_apply(&ApplyRecord {
            apply_id: ap.into(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            groups_requested: 1,
            group_ids: vec![plan.groups[0].group_id.clone()],
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            vault_free_bytes_before: None,
            vault_free_bytes_after: None,
            failures: vec![],
            revertible: true,
            last_undo_step_at: None,
        })
        .unwrap();
    w.store
        .append_journal(&JournalEntry {
            apply_id: ap.into(),
            seq: 0,
            group_id: g.group_id.clone(),
            step: JournalStep::StashOriginal { path: pb.clone(), stash: stash_path.clone() },
            state: JournalState::Done,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            error: None,
        })
        .unwrap();

    applier(&w).revert(ap, &CancelToken::new(), &NullSink).unwrap();

    assert!(pb.is_file(), "the duplicate must be back under its own name");
    assert_eq!(std::fs::read(&pb).unwrap(), weights("m"));
    assert!(!stash_path.exists(), "the set-aside copy must not be left behind");
}

#[test]
fn a_crash_after_removing_a_duplicate_is_recovered_from_the_vault() {
    // The bytes at this path really are gone. They come back out of the vault,
    // which is safe because they are the same content by hash. That is why the
    // removal was allowed in the first place.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);

    let g = &plan.groups[0];
    let vault_file = w.vault_root.join(&g.vault_rel_path);
    std::fs::create_dir_all(vault_file.parent().unwrap()).unwrap();
    std::fs::write(&vault_file, weights("m")).unwrap();
    std::fs::remove_file(&pb).unwrap();

    let ap = "ap-crash";
    w.store
        .put_apply(&ApplyRecord {
            apply_id: ap.into(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            groups_requested: 1,
            group_ids: vec![plan.groups[0].group_id.clone()],
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            vault_free_bytes_before: None,
            vault_free_bytes_after: None,
            failures: vec![],
            revertible: true,
            last_undo_step_at: None,
        })
        .unwrap();
    w.store
        .append_journal(&JournalEntry {
            apply_id: ap.into(),
            seq: 0,
            group_id: g.group_id.clone(),
            step: JournalStep::DeleteStash {
                stash: pb.with_file_name("m.safetensors.comfyvault-old-x"),
                original: pb.clone(),
                vault_path: vault_file.clone(),
                sha256: g.sha256.clone(),
                size_bytes: g.size_bytes,
                mtime_nanos: None,
            },
            state: JournalState::Done,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            error: None,
        })
        .unwrap();

    applier(&w).revert(ap, &CancelToken::new(), &NullSink).unwrap();

    assert!(pb.is_file(), "the removed duplicate must come back");
    assert_eq!(std::fs::read(&pb).unwrap(), weights("m"));
}

#[test]
fn resume_finishes_only_what_the_person_ticked() {
    // This test used to assert the opposite, and it was written from the
    // implementation instead of from the contract. Section 6.1: "The engine
    // never applies a group the caller did not name."
    //
    // The sequence it guards: tick three of five hundred, press Apply, lose
    // power, press the recovery button. Resuming the whole plan would move all
    // five hundred into the vault and delete every duplicate of all five
    // hundred, at the moment the person is least likely to check.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["ticked", "untouched"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a.clone(), b.clone()]);
    let ticked = group_for(&plan, "ticked").group_id.clone();

    applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: vec![ticked],
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();
    simulate_crash_after(&w, "ap-1", usize::MAX);
    assert_eq!(applier(&w).interrupted().unwrap().len(), 1);

    let record = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.state, ApplyState::Completed);

    // The ticked content is consolidated and reads through everywhere.
    assert!(w.vault_root.join("loras/ticked.safetensors").is_file());
    for install in [&a, &b] {
        let p = install.root.join("models/loras/ticked.safetensors");
        assert!(w.is_link(&p));
        assert_eq!(w.read(&p), weights("ticked"));
    }

    // The one the person left alone is exactly as it was: two real files, both
    // with their own bytes, and nothing in the vault.
    assert!(
        !w.vault_root.join("loras/untouched.safetensors").exists(),
        "resume moved a group the person never ticked"
    );
    for install in [&a, &b] {
        let p = install.root.join("models/loras/untouched.safetensors");
        assert!(!w.is_link(&p), "{} became a link", p.display());
        assert_eq!(
            std::fs::read(&p).unwrap(),
            weights("untouched"),
            "{} lost its bytes",
            p.display()
        );
    }
}

#[test]
fn a_resumed_run_can_still_be_undone() {
    // A resumed pass used to restart the journal at zero, overwriting the
    // entries that described work already on disk. The journal is the only
    // record of how to undo it, so the undo then reported success and left the
    // tree consolidated.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["first", "second"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a.clone(), b.clone()]);
    let both: Vec<String> = plan.groups.iter().map(|g| g.group_id.clone()).collect();

    applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: both,
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap();

    let journal_before = w.store.journal("ap-1").unwrap().len();
    simulate_crash_after(&w, "ap-1", usize::MAX);
    applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();

    assert!(
        w.store.journal("ap-1").unwrap().len() >= journal_before,
        "the resumed pass wrote over the first pass's journal"
    );

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();

    for tag in ["first", "second"] {
        for install in [&a, &b] {
            let p = install.root.join(format!("models/loras/{tag}.safetensors"));
            assert!(p.is_file(), "{} is missing after the undo", p.display());
            assert!(!w.is_link(&p), "{} is still a link after the undo", p.display());
            assert_eq!(std::fs::read(&p).unwrap(), weights(tag));
        }
    }
    assert!(!w.vault_root.join("loras/first.safetensors").exists());
    assert!(!w.vault_root.join("loras/second.safetensors").exists());
}

#[test]
fn a_record_with_no_stored_selection_resumes_nothing() {
    // A record written by an older build has no selection. Guessing the whole
    // plan is exactly the defect; doing nothing is the safe direction.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a.clone(), b.clone()]);

    w.store
        .put_apply(&ApplyRecord {
            apply_id: "old".into(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            groups_requested: 1,
            group_ids: Vec::new(),
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            vault_free_bytes_before: None,
            vault_free_bytes_after: None,
            failures: vec![],
            revertible: true,
            last_undo_step_at: None,
        })
        .unwrap();

    let record = applier(&w).resume("old", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.groups_applied, 0);
    assert!(!w.vault_root.join("loras/m.safetensors").exists());
    for install in [&a, &b] {
        let p = install.root.join("models/loras/m.safetensors");
        assert!(!w.is_link(&p));
        assert_eq!(std::fs::read(&p).unwrap(), weights("m"));
    }
}

#[test]
fn a_finished_run_is_never_reported_as_interrupted() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);

    assert!(applier(&w).interrupted().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// The journal
// ---------------------------------------------------------------------------

#[test]
fn every_step_is_written_down_before_it_happens() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    let journal = w.store.journal("ap-1").unwrap();
    assert!(!journal.is_empty());
    assert!(journal.iter().all(|e| e.state == JournalState::Done));

    // The sequence has no gaps, so a revert walks it without guessing.
    let seqs: Vec<u64> = journal.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());

    // The order is the order the work has to be undone in.
    let kinds: Vec<&str> = journal
        .iter()
        .map(|e| match &e.step {
            JournalStep::CreateDir { .. } => "dir",
            JournalStep::MoveToVault { .. } => "move",
            JournalStep::StashOriginal { .. } => "stash",
            JournalStep::CreateLink { .. } => "link",
            JournalStep::DeleteStash { .. } => "delete",
            JournalStep::RemoveLink { .. } => "unlink",
        })
        .collect();
    assert_eq!(kinds, vec!["dir", "move", "link", "stash", "link", "delete"]);
}

#[test]
fn a_move_that_copied_is_recorded_as_a_copy() {
    // The revert needs to know, because putting a copied file back needs room
    // on the other drive and putting a renamed one back does not.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    vault_on_another_drive(&w);
    run_apply(&w, &plan);

    let moved = w
        .store
        .journal("ap-1")
        .unwrap()
        .into_iter()
        .find_map(|e| match e.step {
            JournalStep::MoveToVault { copied, .. } => Some(copied),
            _ => None,
        })
        .expect("a move step");
    assert!(moved, "a cross-drive move must be recorded as a copy");
}

// ---------------------------------------------------------------------------
// Progress and records
// ---------------------------------------------------------------------------

#[test]
fn progress_is_reported_and_ends_with_the_final_totals() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);

    let sink: RecordingSink<ApplyProgress> = RecordingSink::new();
    applier(&w).apply("ap-1", &plan, &request(&plan), &CancelToken::new(), &sink).unwrap();

    let updates = sink.snapshot();
    assert!(!updates.is_empty());
    let last = updates.last().unwrap();
    assert_eq!(last.phase, ApplyPhase::Finalizing);
    assert_eq!(last.apply_id, "ap-1");
    assert_eq!(last.files_moved, 2);
    assert_eq!(last.links_created, 4);
}

#[test]
fn the_run_is_recorded_and_can_be_listed_afterwards() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);

    let stored = w.store.apply("ap-1").unwrap().unwrap();
    assert_eq!(stored.state, ApplyState::Completed);
    assert!(stored.finished_at.is_some());
    assert!(stored.revertible);
    assert_eq!(w.store.applies().unwrap().len(), 1);

    let v = serde_json::to_value(&stored).unwrap();
    assert!(v.get("applyId").is_some(), "the UI reads camelCase names");
    assert!(v.get("bytesFreed").is_some());
}

#[test]
fn the_links_the_apply_made_are_recorded_with_their_install_and_name() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    let pb = w.write_model(&b, "models/loras/other-name.safetensors", &weights("same"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    run_apply(&w, &plan);

    let links = w.store.links().unwrap();
    assert_eq!(links.len(), 2);

    let for_a = links.iter().find(|l| l.abs_path == pa).unwrap();
    let for_b = links.iter().find(|l| l.abs_path == pb).unwrap();
    assert_eq!(for_a.install_id, a.id);
    assert_eq!(for_b.install_id, b.id);
    assert_eq!(for_b.link_name, "other-name.safetensors");
    assert_eq!(for_a.sha256, weights_hash("same"));
    assert_eq!(for_a.rel_path, PathBuf::from("models/loras/lora1.safetensors"));
}

#[test]
fn applying_a_plan_that_is_not_in_this_vault_is_refused() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    let err = applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: vec!["g-does-not-exist".into()],
                verify: VerifyModeArg::SizeAndMtime,
                stop_on_error: false,
            },
            &CancelToken::new(),
            &NullSink,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn a_second_apply_finds_nothing_left_to_do() {
    // Running it twice must be safe. The second scan sees links into the vault
    // and plans nothing.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    run_apply(&w, &plan);

    let second = w.plan(&[a, b]);
    assert!(second.groups.is_empty(), "there is nothing left to consolidate");
    assert!(second.blocked.is_empty(), "work already done is not a problem to report");
}

// ---------------------------------------------------------------------------
// Deleting is the one irreversible step. The proof that a duplicate is
// identical has to be bytes read now, not a hash from an earlier scan.
// ---------------------------------------------------------------------------

#[test]
fn a_wrong_cache_row_cannot_make_the_engine_delete_the_wrong_file() {
    // The hash in a scan entry can come from the cache rather than from the
    // file. A row that matches but is wrong is the only interesting question
    // about a cache, and the answer must not be "delete the bytes".
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("a-content"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("b-content"));

    // Plant a row claiming B's file holds A's content, which is what a stale
    // row or a coarse-timestamp collision produces.
    let meta = std::fs::metadata(&pb).unwrap();
    w.store
        .put_cached_hash(&crate::store::HashCacheRecord {
            path: pb.clone(),
            size_bytes: meta.len(),
            mtime_nanos: crate::time_util::Timestamp::mtime_nanos(&meta),
            sha256: weights_hash("a-content"),
        })
        .unwrap();

    let plan = w.plan(&[a, b]);
    assert_eq!(plan.groups.len(), 1, "the wrong row did group them, as it would");

    let result = run_apply(&w, &plan);
    assert_eq!(result.groups_applied, 0, "a group built on a wrong row must not complete");

    // Both files still hold their own bytes.
    assert_eq!(std::fs::read(&pa).unwrap(), weights("a-content"));
    assert_eq!(
        std::fs::read(&pb).unwrap(),
        weights("b-content"),
        "B's own weights were destroyed on the strength of a hash nothing read"
    );
    assert!(!w.is_link(&pb));
}

#[test]
fn the_check_before_deleting_can_be_switched_off_knowingly() {
    // It costs a full read of every duplicate, so it is a setting. The default
    // is on, and this proves the setting is what decides it rather than luck.
    let w = TestWorld::new();
    let mut settings = w.settings.clone();
    settings.verify_before_delete = false;
    w.store.put_settings(&settings).unwrap();

    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("a-content"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("b-content"));

    let meta = std::fs::metadata(&pb).unwrap();
    w.store
        .put_cached_hash(&crate::store::HashCacheRecord {
            path: pb.clone(),
            size_bytes: meta.len(),
            mtime_nanos: crate::time_util::Timestamp::mtime_nanos(&meta),
            sha256: weights_hash("a-content"),
        })
        .unwrap();

    let plan = w.plan(&[a, b]);
    let result = run_apply(&w, &plan);

    assert_eq!(
        result.groups_applied, 1,
        "with the check off the engine takes the row at its word, which is the point of the setting"
    );
    assert!(w.is_link(&pb));
}

#[test]
fn the_default_is_to_check() {
    assert!(
        crate::settings::Settings::default().verify_before_delete,
        "a fresh vault must prove a duplicate before deleting it"
    );
}

// ---------------------------------------------------------------------------
// An undo must refuse when something later depends on the run.
// ---------------------------------------------------------------------------

#[test]
fn undoing_a_run_after_the_model_was_renamed_in_the_vault_is_refused() {
    // Contract 6.8 promises this and nothing implemented it. Without it the
    // undo reported success, left install A holding a live link, and deleted
    // the database rows for both, so the vault screen showed nothing while an
    // install was still loading that model through the link.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/x.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/y.safetensors", &weights("same"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    run_apply(&w, &plan);

    // The person uses the vault screen to keep the other name.
    crate::vault::Vault::new(&w.store, &w.platform)
        .set_canonical_name(&weights_hash("same"), "y.safetensors")
        .unwrap();

    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(
        err.detail.unwrap().contains("x.safetensors"),
        "the person has to be told which files are in the way"
    );

    // And nothing was half undone: both installs still read their model.
    for (install, name) in [(&a, "x.safetensors"), (&b, "y.safetensors")] {
        let p = install.root.join(format!("models/loras/{name}"));
        assert!(w.is_link(&p));
        assert_eq!(w.read(&p), weights("same"));
    }
}

#[test]
fn an_unrelated_later_run_does_not_block_an_undo() {
    // The control. A check this broad would be useless if it refused every
    // undo as soon as a second consolidation had ever happened.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["first", "second"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a.clone(), b.clone()]);

    let one = |id: &str, tag: &str| {
        applier(&w)
            .apply(
                id,
                &plan,
                &ApplyRequest {
                    plan_id: plan.plan_id.clone(),
                    group_ids: vec![group_for(&plan, tag).group_id.clone()],
                    verify: VerifyModeArg::SizeAndMtime,
                    stop_on_error: false,
                },
                &CancelToken::new(),
                &NullSink,
            )
            .unwrap()
    };
    one("ap-first", "first");
    one("ap-second", "second");

    applier(&w)
        .revert("ap-first", &CancelToken::new(), &NullSink)
        .expect("an unrelated later run must not block this undo");

    // The first is back, the second is untouched.
    for install in [&a, &b] {
        let p = install.root.join("models/loras/first.safetensors");
        assert!(!w.is_link(&p));
        assert_eq!(std::fs::read(&p).unwrap(), weights("first"));

        let still = install.root.join("models/loras/second.safetensors");
        assert!(w.is_link(&still), "the other run must stay applied");
    }
}

#[test]
fn undoing_a_run_a_hand_made_link_depends_on_is_refused() {
    // A link made from the vault screen writes no journal entry, so a check
    // that reads only journals cannot see it. Undoing the run that put the
    // file in the vault then leaves that install holding a link to nothing,
    // which section 8.8 calls the most serious state this app can produce:
    // ComfyUI lists the model and fails to load it, and a node re-downloading
    // the "missing" model writes straight through the dead link into the vault.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    run_apply(&w, &plan);

    // The person adds that model to a third install from the vault screen.
    let link = crate::links::Links::new(&w.store, &w.platform)
        .create(&crate::links::CreateLinkRequest {
            install_id: c.id.clone(),
            sha256: weights_hash("m"),
            relative_dir: "models/loras".into(),
            link_name: None,
            create_dir: true,
        })
        .unwrap();
    assert_eq!(w.read(&link.abs_path), weights("m"));

    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(
        err.detail.unwrap().contains("C"),
        "the person has to be told which install is still using it"
    );

    // And the third install's link still reads, because nothing was undone.
    assert!(w.is_link(&link.abs_path));
    assert_eq!(w.read(&link.abs_path), weights("m"));
}

#[test]
fn a_hand_made_link_to_a_different_model_does_not_block_an_undo() {
    // The control. A check this broad would be useless if any manual link
    // anywhere froze every undo.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    w.write_model(&a, "models/loras/mine.safetensors", &weights("mine"));
    w.write_model(&b, "models/loras/mine.safetensors", &weights("mine"));
    w.write_model(&a, "models/loras/other.safetensors", &weights("other"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    let mine = group_for(&plan, "mine").group_id.clone();
    let other = group_for(&plan, "other").group_id.clone();

    let run = |id: &str, g: String| {
        applier(&w)
            .apply(
                id,
                &plan,
                &ApplyRequest {
                    plan_id: plan.plan_id.clone(),
                    group_ids: vec![g],
                    verify: VerifyModeArg::SizeAndMtime,
                    stop_on_error: false,
                },
                &CancelToken::new(),
                &NullSink,
            )
            .unwrap()
    };
    run("ap-mine", mine);
    run("ap-other", other);

    // A hand-made link to the OTHER model.
    crate::links::Links::new(&w.store, &w.platform)
        .create(&crate::links::CreateLinkRequest {
            install_id: c.id,
            sha256: weights_hash("other"),
            relative_dir: "models/loras".into(),
            link_name: None,
            create_dir: true,
        })
        .unwrap();

    applier(&w)
        .revert("ap-mine", &CancelToken::new(), &NullSink)
        .expect("a link to a different model must not block this undo");

    for install in [&a, &b] {
        let p = install.root.join("models/loras/mine.safetensors");
        assert!(!w.is_link(&p));
        assert_eq!(std::fs::read(&p).unwrap(), weights("mine"));
    }
}

#[test]
fn an_unsafe_vault_path_reaches_the_person_as_what_it_is() {
    // It used to arrive filed as an unreadable file while carrying the right
    // sentence, so the reason and the words disagreed.
    assert_eq!(
        block_reason_for(&VaultError::new(
            ErrorCode::PathOutsideBoundary,
            "outside"
        )),
        BlockReason::UnsafeVaultPath
    );
}

#[test]
fn an_apply_stopped_at_any_point_leaves_each_model_whole_or_untouched() {
    // Measured before this: stopped at the right point, a model of three
    // copies ended with two links and one real file, and the database held
    // no record of the two links. The clean-up that should have put the group
    // back heard the same Stop and stopped at its first copy.
    let mut big = weights("big");
    big.resize(3 * 1024 * 1024 + 17, 0x5A);
    let mut small = weights("small");
    small.resize(2 * 1024 * 1024 + 5, 0x33);

    for vault_elsewhere in [false, true] {
        let mut stops = 0;
        for n in 1.. {
            let w = TestWorld::new();
            let installs: Vec<_> = ["A", "B", "C"].iter().map(|l| w.add_install(l)).collect();
            // One model in three places, one of them under another name, and
            // a second model in two places.
            let big_paths: Vec<PathBuf> = installs
                .iter()
                .zip(["m", "m", "other"])
                .map(|(i, name)| w.write_model(i, &format!("models/checkpoints/{name}.safetensors"), &big))
                .collect();
            let small_paths: Vec<PathBuf> = installs[..2]
                .iter()
                .map(|i| w.write_model(i, "models/loras/s.safetensors", &small))
                .collect();
            let plan = w.plan(&installs);
            assert_eq!(plan.groups.len(), 2);
            if vault_elsewhere {
                vault_on_another_drive(&w);
            }
            let when = format!("vault elsewhere {vault_elsewhere}, stopped at check {n}");

            let record = applier(&w)
                .apply("ap-1", &plan, &request(&plan), &CancelToken::stopping_at_check(n), &NullSink)
                .unwrap_or_else(|e| panic!("{when}: {e:?}"));

            for (paths, content) in [(&big_paths, &big), (&small_paths, &small)] {
                every_place_loads(paths, content, &when);
                // Whole or untouched, never between.
                let linked = paths.iter().filter(|p| w.is_link(p)).count();
                assert!(
                    linked == 0 || linked == paths.len(),
                    "{when}: {linked} of {} places are links, the model is half consolidated",
                    paths.len()
                );
                // The database says what the disk says.
                for p in paths.iter() {
                    let recorded = w.store.link_at_path(p).unwrap().is_some();
                    assert_eq!(recorded, w.is_link(p), "{when}: the record for {} disagrees with the disk", p.display());
                }
                let hash = crate::scan::hash::hash_bytes(content);
                let in_vault = w.store.vault_file(&hash).unwrap().is_some();
                assert_eq!(in_vault, linked > 0, "{when}: the vault record disagrees with the links");
            }
            let health = crate::vault::Vault::new(&w.store, &w.platform).health().unwrap();
            assert!(
                health.ok && health.foreign_files.is_empty(),
                "{when}: the health check reports trouble: {health:?}"
            );
            let leftovers: Vec<_> = std::fs::read_dir(w.store.temp_dir())
                .map(|d| d.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            assert!(leftovers.is_empty(), "{when}: part files left in the vault: {leftovers:?}");

            if record.state == ApplyState::Completed {
                break;
            }
            assert_eq!(record.state, ApplyState::Cancelled, "{when}");
            stops += 1;
        }
        assert!(stops > 10, "the apply was stopped only {stops} times, the test proves little");
    }
}

#[test]
fn a_run_resumed_after_a_crash_is_recorded_as_the_whole_run() {
    // Measured before this: cut after three of nine groups and resumed, a run
    // that finished all nine was recorded as six, with the first three groups'
    // files, links and bytes missing. The record written when a run begins
    // holds no totals, and the resume added only its own.
    fn world() -> (TestWorld, Vec<Vec<PathBuf>>, ConsolidationPlan) {
        let w = TestWorld::new();
        let installs: Vec<_> = ["A", "B", "C"].iter().map(|l| w.add_install(l)).collect();
        let places = ["one", "two", "three"]
            .iter()
            .map(|tag| {
                installs
                    .iter()
                    .map(|i| w.write_model(i, &format!("models/loras/{tag}.safetensors"), &weights(tag)))
                    .collect()
            })
            .collect();
        let plan = w.plan(&installs);
        (w, places, plan)
    }

    // What the same run records when nothing goes wrong.
    let (clean, _, clean_plan) = world();
    let whole = applier(&clean)
        .apply("ap-1", &clean_plan, &request(&clean_plan), &CancelToken::new(), &NullSink)
        .unwrap();
    assert_eq!(whole.groups_applied, 3);

    let (w, places, plan) = world();
    const BEFORE: u64 = 10_000_000_000;
    w.platform.set_free_bytes(BEFORE);

    // Let the first group finish, then cut the power: the process stops
    // before it writes any totals, and before the first group's records.
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let stop = move |p: &ApplyProgress| {
        if p.group_index >= 1 {
            trigger.cancel();
        }
    };
    applier(&w).apply("ap-1", &plan, &request(&plan), &cancel, &stop).unwrap();
    let mut cut = w.store.apply("ap-1").unwrap().unwrap();
    assert_eq!(cut.groups_applied, 1, "the cut must come after exactly one group");
    cut.state = ApplyState::Running;
    cut.finished_at = None;
    cut.groups_applied = 0;
    cut.bytes_freed = 0;
    cut.files_moved = 0;
    cut.links_created = 0;
    cut.vault_free_bytes_after = None;
    w.store.put_apply(&cut).unwrap();
    // The cut fell part way through the first group's records: one link was
    // recorded, two were not, and the vault file was not.
    let recorded = w.store.links().unwrap();
    assert_eq!(recorded.len(), 3);
    for link in &recorded[1..] {
        w.store.delete_link(&link.id).unwrap();
    }
    assert_eq!(w.store.vault_files().unwrap().len(), 1);
    w.store.delete_vault_file(&w.store.vault_files().unwrap()[0].sha256).unwrap();

    w.platform.set_free_bytes(30_000_000_000);
    let resumed = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();

    assert_eq!(resumed.state, ApplyState::Completed);
    assert_eq!(resumed.groups_requested, whole.groups_requested);
    assert_eq!(resumed.groups_applied, whole.groups_applied, "groups");
    assert_eq!(resumed.files_moved, whole.files_moved, "files moved");
    assert_eq!(resumed.links_created, whole.links_created, "links");
    assert_eq!(resumed.bytes_freed, whole.bytes_freed, "bytes freed");
    assert_eq!(resumed.vault_free_bytes_before, Some(BEFORE), "before means before the run began");

    // The database knows every link and every vault file on the disk.
    for p in places.iter().flatten() {
        assert!(w.is_link(p), "{} was not consolidated", p.display());
        assert!(w.store.link_at_path(p).unwrap().is_some(), "{} has no record", p.display());
    }
    assert_eq!(w.store.links().unwrap().len(), 9, "a link was recorded twice");
    assert_eq!(w.store.vault_files().unwrap().len(), 3);
}

#[test]
fn the_time_of_the_last_undo_step_stays_empty_until_a_step_begins() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);
    let stamp = || w.store.apply("ap-1").unwrap().unwrap().last_undo_step_at;
    assert_eq!(stamp(), None, "a run nobody undid");

    // Refused for lack of room: nothing touched.
    w.platform.set_free_bytes(1);
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(stamp(), None, "a refused undo recorded a step");
    w.platform.set_free_bytes(u64::MAX / 2);

    // Stopped before its first step: partly undone in name, nothing touched.
    applier(&w).revert("ap-1", &CancelToken::stopping_at_check(1), &NullSink).unwrap_err();
    assert_eq!(w.store.apply("ap-1").unwrap().unwrap().state, ApplyState::PartlyReverted);
    assert_eq!(stamp(), None, "an undo stopped before any step recorded one");

    // An older build wrote records without the field.
    let mut old = serde_json::to_value(w.store.apply("ap-1").unwrap().unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("lastUndoStepAt");
    let read: ApplyRecord = serde_json::from_value(old).unwrap();
    assert_eq!(read.last_undo_step_at, None);
}

#[test]
fn a_scan_between_two_stopped_undos_is_out_of_date_once_the_second_one_moves_a_file() {
    // Undo, stop, scan, undo again, stop again. The scan was right when it
    // finished, and is out of date once the second undo puts anything back. A
    // time written only once, at the first undo, stays older than that scan
    // and calls it current.
    let w = TestWorld::new();
    let installs: Vec<_> = ["A", "B", "C"].iter().map(|l| w.add_install(l)).collect();
    for tag in ["one", "two", "three"] {
        for i in &installs {
            w.write_model(i, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        }
    }
    let plan = w.plan(&installs);
    run_apply(&w, &plan);
    let stamp = || w.store.apply("ap-1").unwrap().unwrap().last_undo_step_at;
    let undone = || {
        w.store.journal("ap-1").unwrap().iter().filter(|e| e.state == JournalState::Undone).count()
    };

    applier(&w).revert("ap-1", &CancelToken::stopping_at_check(8), &NullSink).unwrap_err();
    let first = undone();
    assert!(first > 0, "the first undo must have put something back");

    let scanned = crate::time_util::Timestamp::now();
    std::thread::sleep(std::time::Duration::from_millis(5));

    // A second attempt stopped before its first step changes nothing, so the
    // scan is still current and the time must not claim otherwise.
    applier(&w).revert("ap-1", &CancelToken::stopping_at_check(1), &NullSink).unwrap_err();
    assert_eq!(undone(), first);
    assert!(stamp().unwrap() < scanned, "nothing came back, yet the scan is called out of date");

    applier(&w).revert("ap-1", &CancelToken::stopping_at_check(8), &NullSink).unwrap_err();
    assert!(undone() > first, "the second undo must have put something back");
    assert!(
        stamp().unwrap() > scanned,
        "files came back after the scan, and the recorded time still calls it current"
    );
}

// ---------------------------------------------------------------------------
// The vault's database is not trusted
// ---------------------------------------------------------------------------

fn crafted(ap: &str, seq: u64, step: JournalStep, state: JournalState) -> JournalEntry {
    JournalEntry {
        apply_id: ap.into(),
        seq,
        group_id: "g-crafted".into(),
        step,
        state,
        started_at: crate::time_util::Timestamp::now(),
        finished_at: None,
        error: None,
    }
}

#[test]
fn an_undo_refuses_a_run_that_names_a_file_outside_the_vault_and_the_installs() {
    // One row added to the database made an undo delete a document that was
    // in no install and not in the vault.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);

    let victim = w.write_file("Documents/thesis.docx", b"irreplaceable");
    let decoy = w.write_file("Documents/anything.txt", b"any real file");
    w.store
        .append_journal(&crafted(
            "ap-1",
            99,
            JournalStep::MoveToVault { from: decoy, to: victim.clone(), copied: false, sha256: "0".repeat(64), size_bytes: 1 },
            JournalState::Done,
        ))
        .unwrap();

    let err = applier(&w).preview_revert("ap-1").unwrap_err();
    assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    assert!(err.detail.unwrap().contains("thesis.docx"), "the refusal must name the place");
    assert_eq!(std::fs::read(&victim).unwrap(), b"irreplaceable");
    assert!(w.is_link(&pa) && w.is_link(&pb), "the refusal came after files were touched");
    assert_ne!(w.store.apply("ap-1").unwrap().unwrap().state, ApplyState::PartlyReverted);
}

#[test]
fn a_crafted_cut_off_run_cannot_write_a_file_anywhere() {
    // A fake cut-off run made the recovery buttons copy a script into the
    // Windows Startup folder.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/x.safetensors", &weights("x"));
    let plan = w.plan(&[a]);

    let payload = b"@echo off\r\n".to_vec();
    let payload_path = w.vault_root.join("loras/readme.safetensors");
    std::fs::create_dir_all(payload_path.parent().unwrap()).unwrap();
    std::fs::write(&payload_path, &payload).unwrap();
    let startup = w.path().join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/update.bat");

    w.store
        .put_apply(&ApplyRecord {
            apply_id: "ap-evil".into(),
            plan_id: plan.plan_id.clone(),
            state: ApplyState::Running,
            started_at: crate::time_util::Timestamp::now(),
            finished_at: None,
            groups_requested: 1,
            group_ids: vec![],
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            vault_free_bytes_before: None,
            vault_free_bytes_after: None,
            failures: vec![],
            revertible: true,
            last_undo_step_at: None,
        })
        .unwrap();
    w.store
        .append_journal(&crafted(
            "ap-evil",
            0,
            JournalStep::DeleteStash {
                stash: w.path().join("nothing-here"),
                original: startup.clone(),
                vault_path: payload_path,
                sha256: crate::scan::hash::hash_bytes(&payload),
                size_bytes: payload.len() as u64,
                mtime_nanos: None,
            },
            JournalState::Pending,
        ))
        .unwrap();

    for result in [
        applier(&w).resume("ap-evil", &CancelToken::new(), &NullSink).map(|_| ()),
        applier(&w).revert("ap-evil", &CancelToken::new(), &NullSink).map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideBoundary);
    }
    assert!(!startup.exists(), "a file was written outside every install");
}

#[test]
fn a_stored_plan_that_names_a_place_outside_the_installs_is_not_applied() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let mut plan = w.plan(&[a, b]);
    // The plan is read back from the database, so it can say anything.
    let outside = w.write_file("Documents/m.safetensors", &weights("m"));
    let meta = std::fs::metadata(&outside).unwrap();
    let link = &mut plan.groups[0].links[1];
    link.abs_path = outside.clone();
    link.size_bytes = meta.len();
    link.mtime_nanos = crate::time_util::Timestamp::mtime_nanos(&meta);

    let record = run_apply(&w, &plan);
    assert_eq!(record.groups_applied, 0);
    assert_eq!(record.failures.len(), 1);
    assert!(outside.is_file() && !w.is_link(&outside), "a file outside every install was touched");
}

#[test]
fn a_stored_install_that_is_not_a_comfyui_install_gives_no_place_to_touch() {
    // A row in the database is only a claim. Its folders count only when the
    // folder on this disk is a ComfyUI install now.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a.clone(), b]);
    run_apply(&w, &plan);

    // The install stops being one: its markers are gone.
    for f in ["main.py", "nodes.py", "execution.py", "server.py", "folder_paths.py"] {
        let _ = std::fs::remove_file(a.root.join(f));
    }
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    assert!(w.is_link(&pa));
}

#[test]
fn a_run_over_every_kind_of_model_folder_can_still_be_undone() {
    // The places the check accepts are the places a scan can find a model:
    // the models folder, a folder named in extra_model_paths.yaml, and the
    // model folders under output.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let shared = w.path().join("SharedModels");
    let a = w.add_extra_model_path(&a, "loras", &shared);
    let places = vec![
        w.write_model(&a, "models/checkpoints/c.safetensors", &weights("c")),
        w.write_file("SharedModels/c.safetensors", &weights("c")),
        w.write_model(&b, "output/checkpoints/c.safetensors", &weights("c")),
    ];
    let plan = w.plan(&[a, b]);
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].links.len(), 3, "the scan must find all three places");
    run_apply(&w, &plan);
    assert!(places.iter().all(|p| w.is_link(p)));

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    for p in &places {
        assert!(!w.is_link(p));
        assert_eq!(std::fs::read(p).unwrap(), weights("c"));
    }
}

// ---------------------------------------------------------------------------
// An undo proves what it deletes
// ---------------------------------------------------------------------------

#[test]
fn an_undo_keeps_the_only_copy_when_a_different_file_replaced_its_link() {
    // A downloader updates a model by renaming a new file onto its path,
    // which replaces the link. The undo used to see "a file is there", take
    // the vault file for a leftover copy, delete it, and report success.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/only.safetensors", &weights("v1"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);
    let vault_file = w.vault_root.join("loras/only.safetensors");

    let tmp = p.with_extension("download");
    std::fs::write(&tmp, weights("v2")).unwrap();
    std::fs::rename(&tmp, &p).unwrap();

    let err = applier(&w).preview_revert("ap-1").unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "the preview must say the undo cannot go ahead");
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.detail.as_deref().unwrap().contains("only.safetensors"));
    assert_eq!(std::fs::read(&vault_file).unwrap(), weights("v1"), "the only copy of the model is gone");
    assert_eq!(std::fs::read(&p).unwrap(), weights("v2"), "the new file was touched");
    assert_eq!(w.store.apply("ap-1").unwrap().unwrap().state, ApplyState::Completed);
}

#[test]
fn an_undo_whose_kept_copy_came_back_the_same_drops_the_vault_file() {
    // The same bytes at the old place: the model is safe there, and the vault
    // file is the one to drop.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    run_apply(&w, &plan);
    std::fs::remove_file(&p).unwrap();
    std::fs::write(&p, weights("m")).unwrap();

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), weights("m"));
    assert!(!w.vault_root.join("loras/m.safetensors").exists());
}

#[test]
fn a_move_refused_because_the_vault_path_was_taken_leaves_nothing_to_undo_there() {
    // A file landed at the vault path after the plan was made. The move was
    // refused, but its journal step stayed pending, and the undo later took it
    // for a move that happened and deleted that file.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/a.safetensors", &weights("y"));
    w.write_model(&a, "models/loras/z.safetensors", &weights("z"));
    let plan = w.plan(&[a]);
    let squatter = w.write_file("ComfyVault/loras/a.safetensors", b"someone else's only copy");

    let record = run_apply(&w, &plan);
    assert_eq!(record.state, ApplyState::CompletedWithErrors);
    let journal = w.store.journal("ap-1").unwrap();
    assert!(journal.iter().all(|e| e.state != JournalState::Pending), "a refused step was left pending");
    assert!(journal.iter().any(|e| e.state == JournalState::Failed));

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(std::fs::read(&squatter).unwrap(), b"someone else's only copy");
}

#[test]
fn two_runs_from_plans_made_before_either_can_both_be_undone() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/a.safetensors", &weights("x"));
    w.write_model(&b, "models/loras/a.safetensors", &weights("y"));
    w.write_model(&b, "models/loras/z.safetensors", &weights("z"));
    let p1 = w.plan(&[a]);
    let p2 = w.plan(&[b]);
    applier(&w).apply("ap-1", &p1, &request(&p1), &CancelToken::new(), &NullSink).unwrap();
    let second = applier(&w).apply("ap-2", &p2, &request(&p2), &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(second.groups_failed, 1, "the second run's move onto the taken name is refused");

    applier(&w).revert("ap-2", &CancelToken::new(), &NullSink).unwrap();
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
}

#[test]
fn a_recovery_keeps_the_only_copy_when_a_different_file_replaced_its_link() {
    // The same proof holds when a cut-off group is rolled back before a
    // recovery finishes the run.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);
    let source = plan.groups[0].source.abs_path.clone();
    let vault_file = w.vault_root.join(&plan.groups[0].vault_rel_path);
    // Cut off after the source's link: the rest of the group is unconfirmed.
    simulate_crash_after(&w, "ap-1", 3);
    std::fs::remove_file(&source).unwrap();
    std::fs::write(&source, weights("other")).unwrap();

    let err = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(std::fs::read(&vault_file).unwrap(), weights("m"), "the only copy of the model is gone");
    assert_eq!(std::fs::read(&source).unwrap(), weights("other"));
    let _ = (pa, pb);
}

// ---------------------------------------------------------------------------
// A crash at any step of a duplicate is recovered
// ---------------------------------------------------------------------------

/// A finished two-copy run, with the duplicate's place, its set-aside name and
/// the journal index of each of its steps.
fn two_copy_run(w: &TestWorld) -> (PathBuf, PathBuf, Vec<JournalEntry>) {
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(w, &plan);
    let dup = if plan.groups[0].source.abs_path == pa { pb } else { pa };
    let journal = w.store.journal("ap-1").unwrap();
    let stash = journal
        .iter()
        .find_map(|e| match &e.step {
            JournalStep::StashOriginal { stash, .. } => Some(stash.clone()),
            _ => None,
        })
        .unwrap();
    (dup, stash, journal)
}

/// Puts the duplicate's bytes back under a name, with the time they had, the
/// way a rename that a crash cut off would have left them.
fn leave_set_aside(journal: &[JournalEntry], stash: &Path) {
    std::fs::write(stash, weights("m")).unwrap();
    let nanos = journal
        .iter()
        .find_map(|e| match &e.step {
            JournalStep::DeleteStash { mtime_nanos, .. } => *mtime_nanos,
            _ => None,
        })
        .unwrap();
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(nanos as u64);
    std::fs::File::options().write(true).open(stash).unwrap().set_modified(t).unwrap();
}

fn index_of(journal: &[JournalEntry], kind: fn(&JournalStep) -> bool) -> usize {
    journal.iter().position(|e| kind(&e.step)).unwrap()
}

#[test]
fn a_crash_while_a_duplicate_is_read_again_before_its_delete_is_recovered() {
    // The re-read before the delete reads the whole duplicate, minutes for a
    // large model, and no step was on record while it ran. A crash there left
    // links everywhere and the bytes under the set-aside name, and recovery
    // called the group finished and counted the space as returned.
    let w = TestWorld::new();
    let (dup, stash, journal) = two_copy_run(&w);
    // The delete step is written first now, so the crash leaves it pending.
    let delete = index_of(&journal, |s| matches!(s, JournalStep::DeleteStash { .. }));
    simulate_crash_after(&w, "ap-1", delete);
    leave_set_aside(&journal, &stash);

    let record = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.state, ApplyState::Completed);
    assert!(!stash.exists(), "the duplicate's bytes are still on disk under their set-aside name");
    assert!(w.is_link(&dup));
    assert_eq!(std::fs::read(&dup).unwrap(), weights("m"));
}

#[test]
fn a_crash_right_after_a_duplicate_is_set_aside_is_recovered() {
    // The set-aside name was recorded only after the rename, so a crash in
    // between left the place empty and the bytes under a name nothing knew.
    let w = TestWorld::new();
    let (dup, stash, journal) = two_copy_run(&w);
    let set_aside = index_of(&journal, |s| matches!(s, JournalStep::StashOriginal { .. }));
    simulate_crash_after(&w, "ap-1", set_aside);
    std::fs::remove_file(&dup).unwrap();
    leave_set_aside(&journal, &stash);

    let record = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.state, ApplyState::Completed);
    assert!(w.is_link(&dup) && !stash.exists());
    assert_eq!(std::fs::read(&dup).unwrap(), weights("m"));
}

#[test]
fn a_set_aside_duplicate_an_older_build_did_not_name_is_found_and_put_back() {
    // A journal from before the name was recorded first: the step has no
    // name, and the bytes are under the name that build chose.
    let w = TestWorld::new();
    let (dup, stash, journal) = two_copy_run(&w);
    let set_aside = index_of(&journal, |s| matches!(s, JournalStep::StashOriginal { .. }));
    simulate_crash_after(&w, "ap-1", set_aside);
    let unnamed = JournalEntry {
        step: JournalStep::StashOriginal { path: dup.clone(), stash: PathBuf::new() },
        ..w.store.journal("ap-1").unwrap()[set_aside].clone()
    };
    w.store.update_journal(&unnamed).unwrap();
    std::fs::remove_file(&dup).unwrap();
    std::fs::write(&stash, weights("m")).unwrap();

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(!w.is_link(&dup), "the duplicate's place was left empty or linked");
    assert_eq!(std::fs::read(&dup).unwrap(), weights("m"));
    assert!(!stash.exists());
}

#[test]
fn a_group_whose_duplicate_was_never_deleted_is_not_called_finished() {
    // Links at every place and nothing pending, but no delete on record: the
    // state a crash during the re-read left before the delete step was
    // written first. The group must be redone, not credited.
    let w = TestWorld::new();
    let (dup, stash, journal) = two_copy_run(&w);
    let delete = index_of(&journal, |s| matches!(s, JournalStep::DeleteStash { .. }));
    simulate_crash_after(&w, "ap-1", journal.len());
    let gone = JournalEntry { state: JournalState::Reverted, ..journal[delete].clone() };
    w.store.update_journal(&gone).unwrap();
    leave_set_aside(&journal, &stash);

    let record = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(!stash.exists(), "the duplicate's bytes stayed under their set-aside name");
    assert_eq!(record.groups_applied, 1);
    assert!(w.is_link(&dup));
}

#[test]
fn the_delete_is_on_record_before_the_duplicate_is_read_again() {
    // Stopped during the re-read: the delete step exists, closed as failed,
    // so a crash at the same moment would have left it pending.
    let mut content = weights("m");
    content.resize(3 * 1024 * 1024, 1);
    for n in 1.. {
        let w = TestWorld::new();
        let a = w.add_install("A");
        let b = w.add_install("B");
        w.write_model(&a, "models/loras/m.safetensors", &content);
        w.write_model(&b, "models/loras/m.safetensors", &content);
        let plan = w.plan(&[a, b]);
        applier(&w).apply("ap-1", &plan, &request(&plan), &CancelToken::stopping_at_check(n), &NullSink).unwrap();
        let journal = w.store.journal("ap-1").unwrap();
        let linked = journal.iter().filter(|e| matches!(e.step, JournalStep::CreateLink { .. })).count();
        if linked < 2 {
            continue;
        }
        // The first stop after both links is inside the re-read.
        assert!(
            journal.iter().any(|e| matches!(e.step, JournalStep::DeleteStash { .. }) && e.state == JournalState::Failed),
            "stopped while the duplicate was read again, and no delete step was on record"
        );
        break;
    }
}


// ---------------------------------------------------------------------------
// Half-written copies left by a process that ended
// ---------------------------------------------------------------------------

#[test]
fn a_half_written_copy_left_by_a_cut_off_undo_is_removed() {
    let w = TestWorld::new();
    let (dup, _, _) = two_copy_run(&w);
    // What a copy back leaves when the window is closed in the middle of it.
    let leftover = dup.with_extension("comfyvault-restore-0123456789abcdef0123456789abcdef");
    std::fs::write(&leftover, &weights("m")[..2048]).unwrap();
    let part = dup.with_file_name("4242-fedcba9876543210fedcba9876543210.part");
    std::fs::write(&part, b"half").unwrap();
    // Names that only look close stay.
    let theirs = dup.with_file_name("notes.comfyvault-restore-mine.txt");
    std::fs::write(&theirs, b"the person's").unwrap();
    let vault_part = w.store.temp_dir().join("17-00112233445566778899aabbccddeeff.part");
    std::fs::create_dir_all(vault_part.parent().unwrap()).unwrap();
    std::fs::write(&vault_part, b"half").unwrap();

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(!leftover.exists() && !part.exists() && !vault_part.exists(), "a half-written copy stayed");
    assert!(theirs.exists(), "a file that is not a leftover copy was removed");
}

#[test]
fn opening_a_vault_removes_the_half_written_copies_in_its_own_folder() {
    let w = TestWorld::new();
    let part = w.store.temp_dir().join("17-00112233445566778899aabbccddeeff.part");
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::write(&part, b"half").unwrap();
    let root = w.vault_root.clone();
    drop(w.store);
    let e = crate::engine::Engine::with_platform(
        w.dir.path().join("config.json"),
        std::sync::Arc::new(crate::platform::FakePlatform::new()),
    );
    e.select_vault(&root, false).unwrap();
    assert!(!part.exists());
}

#[test]
fn an_undo_leaves_a_link_the_person_made_where_the_run_made_one() {
    // A second name beside the vault file is a link the run made. The person
    // pointed that name at another model since. The undo removed any link at
    // the place, whatever it led to.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/other-name.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    run_apply(&w, &plan);
    let alias = w
        .store
        .journal("ap-1")
        .unwrap()
        .into_iter()
        .find_map(|e| match e.step {
            JournalStep::CreateLink { link, .. } if link.starts_with(&w.vault_root) => Some(link),
            _ => None,
        })
        .expect("the run made a second name in the vault");

    let theirs = w.write_file("ComfyVault/loras/their-model.safetensors", b"theirs");
    std::fs::remove_file(&alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&theirs, &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&theirs, &alias).unwrap();

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert!(w.is_link(&alias), "the person's link was removed");
    assert_eq!(std::fs::read(&alias).unwrap(), b"theirs");
}

#[test]
fn a_recovery_checks_files_the_way_the_run_was_asked_to() {
    // A run asked to read every file again was finished, after a cut, with
    // only the size-and-time check. An edit that kept both then went through.
    let w = TestWorld::new();
    let mut settings = w.store.settings().unwrap();
    settings.verify_before_delete = false;
    w.store.put_settings(&settings).unwrap();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a, b]);
    let req = ApplyRequest { verify: VerifyModeArg::Rehash, ..request(&plan) };

    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let stop = move |p: &ApplyProgress| {
        if p.group_index >= 1 {
            trigger.cancel();
        }
    };
    let first = applier(&w).apply("ap-1", &plan, &req, &cancel, &stop).unwrap();
    assert_eq!(first.groups_applied, 1);
    w.store.put_apply(&ApplyRecord { state: ApplyState::Running, finished_at: None, ..first }).unwrap();

    // The group left over: one copy edited, size and time kept.
    let left = plan.groups.iter().find(|g| !w.is_link(&g.source.abs_path)).unwrap();
    let copy = left.links.iter().find(|l| !l.is_source).unwrap().abs_path.clone();
    let mut edited = std::fs::read(&copy).unwrap();
    let last = edited.len() - 1;
    edited[last] ^= 0xFF;
    let mtime = std::fs::metadata(&copy).unwrap().modified().unwrap();
    std::fs::write(&copy, &edited).unwrap();
    std::fs::File::options().write(true).open(&copy).unwrap().set_modified(mtime).unwrap();

    let resumed = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(resumed.failures.len(), 1, "the edit went through the weaker check");
    assert_eq!(resumed.failures[0].reason, BlockReason::FileChanged);
    assert_eq!(std::fs::read(&copy).unwrap(), edited, "the edited file was deleted");
}

#[cfg(windows)]
#[test]
fn an_undo_while_comfyui_holds_the_model_says_so_and_can_be_finished() {
    use std::os::windows::fs::OpenOptionsExt;
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a, b]);
    let native = crate::platform::NativePlatform::new();
    Applier::new(&w.store, &native)
        .apply("ap-1", &plan, &request(&plan), &CancelToken::new(), &NullSink)
        .unwrap();
    let vault_file = w.vault_root.join(&plan.groups[0].vault_rel_path);
    // Read sharing only, the way Python opens a file.
    let held = std::fs::File::options().read(true).share_mode(0x1).open(&vault_file).unwrap();

    let err = Applier::new(&w.store, &native).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::FileLocked, "{}", err.message);
    every_place_loads(&[pa.clone(), pb.clone()], &weights("m"), "while held");

    drop(held);
    Applier::new(&w.store, &native).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    every_place_loads(&[pa, pb], &weights("m"), "after closing");
}

// ---------------------------------------------------------------------------
// Copies of content the vault already holds
// ---------------------------------------------------------------------------

/// A vault that already holds `m` from run ap-1, and a new install holding
/// copies of it. Returns the new copies and the vault file.
fn copies_of_a_model_already_in_the_vault(w: &TestWorld, content: &[u8]) -> (Vec<PathBuf>, PathBuf, ConsolidationPlan) {
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", content);
    let first = w.plan(&[a.clone()]);
    run_apply(w, &first);
    let vault_file = w.vault_root.join("loras/m.safetensors");
    assert!(vault_file.is_file());

    let b = w.add_install("B");
    let c = w.add_install("C");
    let copies = vec![
        w.write_model(&b, "models/loras/m.safetensors", content),
        w.write_model(&c, "models/loras/renamed.safetensors", content),
    ];
    let plan = w.plan(&[a, b, c]);
    (copies, vault_file, plan)
}

#[test]
fn a_new_copy_of_a_model_already_in_the_vault_becomes_a_link() {
    // It was blocked as "a different file already sits at the vault path",
    // which was false, and it stayed a real file for good.
    let w = TestWorld::new();
    let (copies, vault_file, plan) = copies_of_a_model_already_in_the_vault(&w, &weights("m"));
    assert!(plan.blocked.is_empty(), "blocked: {:?}", plan.blocked);
    assert_eq!(plan.groups.len(), 1);
    let g = &plan.groups[0];
    assert!(g.already_in_vault);
    assert_eq!(g.links.len(), 2);
    assert_eq!(g.bytes_freed, 2 * weights("m").len() as u64, "both copies go");
    assert!(!g.single_copy && !g.cross_volume);
    assert_eq!(plan.totals.files_moved, 0);

    let record = applier(&w).apply("ap-2", &plan, &request(&plan), &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.state, ApplyState::Completed);
    assert_eq!(record.files_moved, 0);
    assert_eq!(record.bytes_freed, 2 * weights("m").len() as u64);
    every_place_loads(&copies, &weights("m"), "after the run");
    assert!(copies.iter().all(|p| w.is_link(p)));
    assert_eq!(std::fs::read(&vault_file).unwrap(), weights("m"));
    let names = &w.store.vault_file(&weights_hash("m")).unwrap().unwrap().aliases;
    assert!(names.iter().any(|n| n == "renamed.safetensors"), "the second name was not recorded");

    // The earlier run cannot be undone while these copies are links to its
    // vault file.
    let err = applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    // Undone, every copy comes back and the earlier run's vault file stays.
    applier(&w).revert("ap-2", &CancelToken::new(), &NullSink).unwrap();
    for p in &copies {
        assert!(!w.is_link(p));
        assert_eq!(std::fs::read(p).unwrap(), weights("m"));
    }
    assert_eq!(std::fs::read(&vault_file).unwrap(), weights("m"));
    assert!(w.store.vault_file(&weights_hash("m")).unwrap().is_some(), "the earlier run's record went");
    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
}

#[test]
fn copies_are_kept_when_the_vault_file_is_not_the_model_it_claims_to_be() {
    // Once the copies go, the vault file is the only one. It is read first.
    let w = TestWorld::new();
    let (copies, vault_file, plan) = copies_of_a_model_already_in_the_vault(&w, &weights("m"));
    let mut other = weights("m");
    let last = other.len() - 1;
    other[last] ^= 0xFF;
    std::fs::write(&vault_file, &other).unwrap();

    let record = applier(&w).apply("ap-2", &plan, &request(&plan), &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.groups_applied, 0);
    assert_eq!(record.failures[0].reason, BlockReason::FileChanged);
    for p in &copies {
        assert!(!w.is_link(p));
        assert_eq!(std::fs::read(p).unwrap(), weights("m"));
    }
}

#[test]
fn a_vault_record_whose_file_is_gone_does_not_make_a_link_only_group() {
    let w = TestWorld::new();
    let (copies, vault_file, _) = copies_of_a_model_already_in_the_vault(&w, &weights("m"));
    std::fs::remove_file(&vault_file).unwrap();
    let installs = w.store.installs().unwrap();
    let plan = w.plan(&installs);
    let g = plan.groups.iter().find(|g| g.sha256 == weights_hash("m")).expect("a group");
    assert!(!g.already_in_vault, "copies would be deleted with no vault file to keep");
    let record = applier(&w).apply("ap-2", &plan, &request(&plan), &CancelToken::new(), &NullSink).unwrap();
    assert_eq!(record.groups_applied, 1);
    assert!(vault_file.is_file());
    every_place_loads(&copies, &weights("m"), "after the run");
}

#[test]
fn a_link_only_group_stopped_at_any_point_is_whole_or_untouched_and_undoes_cleanly() {
    let mut content = weights("big");
    content.resize(3 * 1024 * 1024 + 17, 0x5A);
    let mut stops = 0;
    for n in 1.. {
        let w = TestWorld::new();
        assert!(n < 5000, "the apply never finished");
        let (copies, vault_file, plan) = copies_of_a_model_already_in_the_vault(&w, &content);
        let when = format!("stopped at check {n}");
        let record = applier(&w)
            .apply("ap-2", &plan, &request(&plan), &CancelToken::stopping_at_check(n), &NullSink)
            .unwrap();
        every_place_loads(&copies, &content, &when);
        let linked = copies.iter().filter(|p| w.is_link(p)).count();
        assert!(linked == 0 || linked == copies.len(), "{when}: half consolidated");
        assert_eq!(std::fs::read(&vault_file).unwrap(), content, "{when}: the vault file changed");
        for p in &copies {
            assert_eq!(w.store.link_at_path(p).unwrap().is_some(), w.is_link(p), "{when}: record and disk disagree");
        }
        if record.state == ApplyState::Completed {
            // And the undo, stopped at every point too.
            for u in 1.. {
                assert!(u < 5000, "the undo never finished");
                let result = applier(&w).revert("ap-2", &CancelToken::stopping_at_check(u), &NullSink);
                every_place_loads(&copies, &content, &format!("undo stopped at check {u}"));
                assert_eq!(std::fs::read(&vault_file).unwrap(), content);
                if result.is_ok() {
                    break;
                }
            }
            assert!(copies.iter().all(|p| !w.is_link(p)));
            break;
        }
        stops += 1;
    }
    assert!(stops > 5, "the apply was stopped only {stops} times");
}

#[test]
fn a_link_only_group_is_finished_only_when_every_copy_is_deleted() {
    // The source of such a group is a copy too. A crash before its delete
    // must not leave its bytes under a set-aside name for good.
    let w = TestWorld::new();
    let (copies, _, plan) = copies_of_a_model_already_in_the_vault(&w, &weights("m"));
    applier(&w).apply("ap-2", &plan, &request(&plan), &CancelToken::new(), &NullSink).unwrap();
    let journal = w.store.journal("ap-2").unwrap();
    let source = plan.groups[0].source.abs_path.clone();
    let (i, stash) = journal
        .iter()
        .enumerate()
        .find_map(|(i, e)| match &e.step {
            JournalStep::DeleteStash { original, stash, .. } if *original == source => Some((i, stash.clone())),
            _ => None,
        })
        .unwrap();
    simulate_crash_after(&w, "ap-2", journal.len());
    w.store.update_journal(&JournalEntry { state: JournalState::Reverted, ..journal[i].clone() }).unwrap();
    std::fs::write(&stash, weights("m")).unwrap();
    let nanos = match &journal[i].step {
        JournalStep::DeleteStash { mtime_nanos, .. } => mtime_nanos.unwrap(),
        _ => unreachable!(),
    };
    std::fs::File::options()
        .write(true)
        .open(&stash)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(nanos as u64))
        .unwrap();

    applier(&w).resume("ap-2", &CancelToken::new(), &NullSink).unwrap();
    assert!(!stash.exists(), "the source copy's bytes stayed under their set-aside name");
    every_place_loads(&copies, &weights("m"), "after recovery");
}

#[test]
fn a_recovery_refuses_a_run_that_was_not_cut_off() {
    // Resuming a run the person had undone applied the whole of it again.
    let w = TestWorld::new();
    let (dup, _, _) = two_copy_run(&w);
    let err = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "a finished run was finished again");

    applier(&w).revert("ap-1", &CancelToken::new(), &NullSink).unwrap();
    let err = applier(&w).resume("ap-1", &CancelToken::new(), &NullSink).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(!w.is_link(&dup), "an undone run was applied again");
}
