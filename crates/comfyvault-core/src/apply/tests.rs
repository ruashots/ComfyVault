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

fn run_apply(w: &TestWorld, plan: &ConsolidationPlan) -> ApplyRecord {
    applier(w)
        .apply("ap-1", plan, &request(plan), &CancelToken::new(), &NullSink)
        .expect("apply")
}

/// Counts the bytes of real files, ignoring links. This is the number the
/// person's drive actually shows.
fn real_bytes(root: &Path) -> u64 {
    let mut total = 0;
    for e in walkdir::WalkDir::new(root).follow_links(false).into_iter().flatten() {
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

    let before = real_bytes(w.path());
    let plan = w.plan(&[a, b, c]);
    let result = run_apply(&w, &plan);
    let after = real_bytes(w.path());

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
    w.platform.force_cross_volume(true);

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

    let before = real_bytes(w.path());
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
    assert_eq!(real_bytes(w.path()), before, "the disk must look exactly as it did");
}

#[test]
fn reverting_a_cross_drive_apply_also_puts_everything_back() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a, b]);
    w.platform.force_cross_volume(true);
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
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            failures: vec![],
            revertible: true,
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
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            failures: vec![],
            revertible: true,
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
            groups_applied: 0,
            groups_failed: 0,
            bytes_freed: 0,
            files_moved: 0,
            links_created: 0,
            failures: vec![],
            revertible: true,
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
fn a_run_cut_short_after_one_of_two_groups_can_be_finished() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    for tag in ["one", "two"] {
        w.write_model(&a, &format!("models/loras/{tag}.safetensors"), &weights(tag));
        w.write_model(&b, &format!("models/loras/{tag}.safetensors"), &weights(tag));
    }
    let plan = w.plan(&[a.clone(), b.clone()]);

    // Apply only the first group, then make it look like the process died.
    let first = plan.groups[0].group_id.clone();
    applier(&w)
        .apply(
            "ap-1",
            &plan,
            &ApplyRequest {
                plan_id: plan.plan_id.clone(),
                group_ids: vec![first.clone()],
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

    // Both contents are now in the vault and every old place reads through.
    for tag in ["one", "two"] {
        let vault_file = w.vault_root.join(format!("loras/{tag}.safetensors"));
        assert!(vault_file.is_file(), "{tag} did not reach the vault");
        for install in [&a, &b] {
            let p = install.root.join(format!("models/loras/{tag}.safetensors"));
            assert!(w.is_link(&p), "{} is not a link", p.display());
            assert_eq!(w.read(&p), weights(tag));
        }
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
    w.platform.force_cross_volume(true);
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
