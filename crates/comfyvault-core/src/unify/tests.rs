//! Giving a model one name in every install.

use super::*;
use crate::apply::{ApplyRequest, Applier, VerifyModeArg};
use crate::install::Install;
use crate::links::CreateLinkRequest;
use crate::progress::{CancelToken, NullSink};
use crate::testkit::{weights, weights_hash, TestWorld};

const OLD: &str = "my-favourite.safetensors";
const KEPT: &str = "lora1.safetensors";

fn unify(w: &TestWorld) -> Unify<'_> {
    Unify::new(&w.store, &w.platform)
}

fn sha() -> String {
    weights_hash("same")
}

/// One model in two installs under two names, consolidated. Install A's link
/// is `lora1.safetensors`, which is also the vault's name for it. Install B's
/// is `my-favourite.safetensors`, which the vault keeps as a second name.
fn two_names(w: &TestWorld) -> (Install, Install) {
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, &format!("models/loras/{KEPT}"), &weights("same"));
    w.write_model(&b, &format!("models/loras/{OLD}"), &weights("same"));
    let plan = w.plan(&[a.clone(), b.clone()]);
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
    (a, b)
}

fn link_by_hand(w: &TestWorld, install: &Install, dir: &str, name: &str) -> LinkRecord {
    Links::new(&w.store, &w.platform)
        .create(&CreateLinkRequest {
            install_id: install.id.clone(),
            sha256: sha(),
            relative_dir: dir.into(),
            dir: None,
            link_name: Some(name.into()),
            create_dir: true,
        })
        .unwrap()
}

fn loras(i: &Install) -> PathBuf {
    i.root.join("models").join("loras")
}

/// Everything a job could change: every file and link under the installs and
/// the vault, the link records, the vault record, and the journal.
fn snapshot(w: &TestWorld) -> String {
    let mut disk: Vec<String> = walkdir::WalkDir::new(w.path())
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| !e.path().starts_with(w.store.internal_dir()))
        .map(|e| format!("{} link={}", e.path().display(), e.path_is_symlink()))
        .collect();
    disk.sort();
    let mut links: Vec<String> = w
        .store
        .links()
        .unwrap()
        .into_iter()
        .map(|l| format!("{} {} {} {}", l.abs_path.display(), l.id, l.link_name, l.vault_rel_path.display()))
        .collect();
    links.sort();
    let file = w.store.vault_file(&sha()).unwrap();
    let journals: Vec<String> = w.store.journal_ids().unwrap().into_iter().filter(|j| j.starts_with("unify-")).collect();
    format!("{disk:#?}\n{links:#?}\n{file:?}\n{journals:?}")
}

fn names_in_use(w: &TestWorld) -> Vec<String> {
    let mut n: Vec<String> = w.store.links_for_hash(&sha()).unwrap().into_iter().map(|l| l.link_name).collect();
    n.sort();
    n.dedup();
    n
}

fn comfy_process(root: &Path) -> crate::platform::ProcessInfo {
    crate::platform::ProcessInfo {
        pid: 7,
        name: "python.exe".into(),
        exe_path: Some(root.join("python_embeded").join("python.exe")),
        cwd: Some(root.to_path_buf()),
        command_line: vec!["python.exe".into(), "ComfyUI/main.py".into()],
        started_at: None,
    }
}

fn action_at(plan: &UnifyPlan, path: &Path) -> UnifyAction {
    plan.links
        .iter()
        .find(|l| crate::paths::same_path_lexically(&l.abs_path, path))
        .unwrap_or_else(|| panic!("no link at {} in {:#?}", path.display(), plan.links))
        .action
}

// --- the plan --------------------------------------------------------------

#[test]
fn the_plan_says_keep_rename_remove_or_taken_for_each_link_and_changes_nothing() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    // In A, a folder that already has the model under both names.
    link_by_hand(&w, &a, "models/loras/x", KEPT);
    link_by_hand(&w, &a, "models/loras/x", OLD);
    // In B, a folder where a different file already has the chosen name.
    link_by_hand(&w, &b, "models/loras/y", OLD);
    w.write_model(&b, &format!("models/loras/y/{KEPT}"), b"someone else's weights");
    let before = snapshot(&w);

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(plan.name, KEPT);
    assert_eq!(plan.links.len(), 5);
    assert_eq!(action_at(&plan, &loras(&a).join(KEPT)), UnifyAction::Keep);
    assert_eq!(action_at(&plan, &loras(&a).join("x").join(KEPT)), UnifyAction::Keep);
    assert_eq!(action_at(&plan, &loras(&a).join("x").join(OLD)), UnifyAction::Remove);
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::Rename);
    assert_eq!(action_at(&plan, &loras(&b).join("y").join(OLD)), UnifyAction::BlockedTaken);

    let rename = plan.links.iter().find(|l| l.action == UnifyAction::Rename).unwrap();
    assert_eq!(rename.new_abs_path.as_deref(), Some(loras(&b).join(KEPT).as_path()));
    assert_eq!(rename.taken_by, None);
    let taken = plan.links.iter().find(|l| l.action == UnifyAction::BlockedTaken).unwrap();
    assert_eq!(taken.taken_by.as_deref(), Some(crate::paths::display_path(&loras(&b).join("y").join(KEPT)).as_str()));
    assert_eq!(taken.new_abs_path, None);

    assert_eq!(snapshot(&w), before, "reading the plan changed something");
}

#[test]
fn the_plan_lists_the_saved_workflows_that_name_a_name_going_away() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    w.write_model(&b, "user/default/workflows/upscale.json", format!(r#"{{"ckpt":"{OLD}"}}"#).as_bytes());
    w.write_model(&a, "user/default/workflows/other.json", format!(r#"{{"ckpt":"{KEPT}"}}"#).as_bytes());

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(plan.workflows.len(), 1, "only the name that goes away is searched");
    let r = &plan.workflows[0];
    assert_eq!(r.name, OLD);
    assert!(r.searched && r.used);
    assert_eq!(r.matches.len(), 1);
    assert_eq!(r.matches[0].install_id, b.id);
    assert_eq!(r.matches[0].workflow_name, "upscale");
    assert!(!r.method.is_empty());

    // Choosing the other name searches for the first one instead.
    let plan = unify(&w).plan(&sha(), OLD).unwrap();
    assert_eq!(plan.workflows.len(), 1);
    assert_eq!(plan.workflows[0].name, KEPT);
    assert_eq!(plan.workflows[0].matches[0].install_id, a.id);
}

#[test]
fn the_plan_names_the_running_comfyui_only_for_installs_whose_links_change() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    w.platform.set_processes(vec![comfy_process(&a.root)]);
    assert!(unify(&w).plan(&sha(), KEPT).unwrap().running.is_empty(), "A keeps its name");
    assert_eq!(unify(&w).plan(&sha(), OLD).unwrap().running, vec![a.id.clone()]);

    w.platform.set_processes(vec![comfy_process(&b.root)]);
    assert_eq!(unify(&w).plan(&sha(), KEPT).unwrap().running, vec![b.id.clone()]);
}

#[test]
fn two_links_in_one_install_in_different_folders_are_both_renamed() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    link_by_hand(&w, &b, "models/loras/z", OLD);

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::Rename);
    assert_eq!(action_at(&plan, &loras(&b).join("z").join(OLD)), UnifyAction::Rename);

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(done.renamed.len(), 2);
    assert!(w.is_link(&loras(&b).join("z").join(KEPT)));
    assert!(std::fs::symlink_metadata(loras(&b).join("z").join(OLD)).is_err());
}

#[test]
fn two_links_in_one_folder_become_one_link_under_the_name() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    link_by_hand(&w, &b, "models/loras", "third-name.safetensors");

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    // Path order: my-favourite comes before third-name.
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::Rename);
    assert_eq!(action_at(&plan, &loras(&b).join("third-name.safetensors")), UnifyAction::Remove);

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(done.renamed.len(), 1);
    assert_eq!(done.removed.len(), 1);
    assert!(w.is_link(&loras(&b).join(KEPT)));
    assert_eq!(names_in_use(&w), vec![KEPT.to_string()]);
}

#[test]
fn a_link_nobody_recorded_at_the_new_place_blocks_rather_than_being_kept() {
    // Removing B's recorded link in favour of a link the vault does not know
    // would leave B with a link nothing tidies, and the model would read as
    // unused and be deleted.
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let real = w.vault_root.join("loras").join(KEPT);
    w.platform.create_file_symlink(&loras(&b).join(KEPT), &real).unwrap();

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::BlockedTaken);
}

#[test]
fn a_name_that_differs_only_in_case_is_the_same_name_on_windows() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let old = w.store.links_for_hash(&sha()).unwrap().into_iter().find(|l| l.install_id == b.id).unwrap();
    Links::new(&w.store, &w.platform).remove(&old.id).unwrap();
    link_by_hand(&w, &b, "models/loras", "LORA1.safetensors");

    let asked = unify(&w).plan(&sha(), "Lora1.Safetensors");
    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    if cfg!(windows) {
        let asked = asked.expect("Windows reads it as the name the links use");
        assert_eq!(asked.name, KEPT, "spelled as the first link spells it");
        assert!(plan.links.iter().all(|l| l.action == UnifyAction::Keep), "{:#?}", plan.links);
    } else {
        assert_eq!(asked.expect_err("no link has that name").code, ErrorCode::InvalidArgument);
        assert_eq!(action_at(&plan, &loras(&b).join("LORA1.safetensors")), UnifyAction::Rename);
    }
}

#[test]
fn a_name_no_install_uses_or_a_bad_hash_is_refused() {
    let w = TestWorld::new();
    two_names(&w);
    for (hash, name) in [
        (sha(), "invented.safetensors".to_string()),
        (sha(), "..\\x.safetensors".to_string()),
        ("not-a-hash".to_string(), KEPT.to_string()),
    ] {
        let err = unify(&w).plan(&hash, &name).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument, "{hash} {name}");
        assert_eq!(unify(&w).unify(&hash, &name).unwrap_err().code, ErrorCode::InvalidArgument);
    }
    let unknown = "A".repeat(64);
    assert_eq!(unify(&w).plan(&unknown, KEPT).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn a_recorded_link_outside_every_install_refuses_the_whole_job() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    // A record that names a folder outside the installs, as a vault from
    // somewhere else could.
    let elsewhere = w.path().join("Documents").join("third.safetensors");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    let real = w.vault_root.join("loras").join(KEPT);
    w.platform.create_file_symlink(&elsewhere, &real).unwrap();
    let mut record = w.store.links_for_hash(&sha()).unwrap().into_iter().find(|l| l.install_id == b.id).unwrap();
    record.id = "planted".into();
    record.abs_path = elsewhere.clone();
    record.link_name = "third.safetensors".into();
    w.store.put_link(&record).unwrap();
    let before = snapshot(&w);

    assert_eq!(unify(&w).plan(&sha(), KEPT).unwrap_err().code, ErrorCode::PathOutsideBoundary);
    assert_eq!(unify(&w).unify(&sha(), KEPT).unwrap_err().code, ErrorCode::PathOutsideBoundary);
    assert_eq!(snapshot(&w), before);
}

// --- the job ---------------------------------------------------------------

#[test]
fn after_the_job_every_link_carries_the_name_and_the_vault_has_only_that_name() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(done.renamed, vec![RenamedLink { install_id: b.id.clone(), from: loras(&b).join(OLD), to: loras(&b).join(KEPT) }]);
    assert!(done.removed.is_empty() && done.skipped.is_empty() && done.stopped.is_none());
    assert_eq!(done.vault_name, KEPT);

    for i in [&a, &b] {
        assert!(w.is_link(&loras(i).join(KEPT)));
        assert_eq!(w.read(&loras(i).join(KEPT)), weights("same"), "{} loads the model", i.label);
    }
    assert!(std::fs::symlink_metadata(loras(&b).join(OLD)).is_err(), "the old name is gone");
    assert_eq!(names_in_use(&w), vec![KEPT.to_string()]);
    assert!(w.store.link_at_path(&loras(&b).join(OLD)).unwrap().is_none());
    let new = w.store.link_at_path(&loras(&b).join(KEPT)).unwrap().expect("the new link is recorded");
    assert_eq!(new.install_id, b.id);
    assert_eq!(new.link_name, KEPT);
    assert_eq!(new.rel_path, PathBuf::from("models").join("loras").join(KEPT));

    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert_eq!(file.canonical_name, KEPT);
    assert!(file.aliases.is_empty(), "the second name nothing uses is gone: {:?}", file.aliases);
    assert!(std::fs::symlink_metadata(w.vault_root.join("loras").join(OLD)).is_err());
    assert!(Vault::new(&w.store, &w.platform).name_groups().unwrap().is_empty(), "the card goes");
}

#[test]
fn choosing_the_name_the_vault_does_not_keep_renames_the_vault_file_too() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);

    let done = unify(&w).unify(&sha(), OLD).unwrap();
    assert_eq!(done.renamed.len(), 1);
    assert_eq!(done.renamed[0].install_id, a.id);
    assert_eq!(done.vault_name, OLD);

    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert_eq!(file.canonical_name, OLD);
    assert!(file.aliases.is_empty(), "{:?}", file.aliases);
    let real = w.vault_root.join("loras").join(OLD);
    assert!(real.is_file() && !w.is_link(&real));
    assert!(std::fs::symlink_metadata(w.vault_root.join("loras").join(KEPT)).is_err());
    for i in [&a, &b] {
        assert_eq!(w.read(&loras(i).join(OLD)), weights("same"));
    }
    // Every record points at the file's real place, never through a name.
    for l in w.store.links_for_hash(&sha()).unwrap() {
        assert_eq!(l.vault_rel_path, PathBuf::from("loras").join(OLD));
    }
}

#[test]
fn a_name_the_vault_never_kept_becomes_the_vault_name() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    // B also links it under a third name, then the other names go.
    let third = "third.safetensors";
    link_by_hand(&w, &b, "models/loras/t", third);

    let done = unify(&w).unify(&sha(), third).unwrap();
    assert_eq!(done.renamed.len(), 2);
    assert_eq!(done.vault_name, third);
    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert_eq!(file.canonical_name, third);
    assert!(file.aliases.is_empty(), "{:?}", file.aliases);
    for i in [&a, &b] {
        assert_eq!(w.read(&loras(i).join(third)), weights("same"));
    }
}

#[test]
fn the_vault_keeps_its_name_when_another_model_has_the_chosen_one() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let third = "third.safetensors";
    link_by_hand(&w, &b, "models/loras/t", third);
    // A different model already has that name in the vault.
    std::fs::write(w.vault_root.join("loras").join(third), weights("other")).unwrap();
    w.store
        .put_vault_file(&crate::store::VaultFileRecord {
            sha256: weights_hash("other"),
            canonical_name: third.into(),
            category: "loras".into(),
            size_bytes: weights("other").len() as u64,
            added_at: Timestamp::now(),
            aliases: vec![],
        })
        .unwrap();

    let done = unify(&w).unify(&sha(), third).unwrap();
    assert!(done.stopped.is_none(), "{:?}", done.stopped);
    assert_eq!(done.vault_name, KEPT, "the vault kept its name");
    assert_eq!(names_in_use(&w), vec![third.to_string()], "the installs changed anyway");
    assert_eq!(std::fs::read(w.vault_root.join("loras").join(third)).unwrap(), weights("other"));
    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert!(file.aliases.is_empty(), "no link uses the old second name: {:?}", file.aliases);
}

#[test]
fn a_place_taken_since_the_plan_is_skipped_and_the_other_links_still_change() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let c = w.add_install("C");
    link_by_hand(&w, &c, "models/loras", OLD);
    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::Rename);

    // The person drops a file of their own into B with the chosen name.
    w.write_model(&b, &format!("models/loras/{KEPT}"), b"their own file");
    let done = unify(&w).unify(&sha(), KEPT).unwrap();

    assert_eq!(done.skipped.len(), 1);
    assert_eq!(done.skipped[0].install_id, b.id);
    assert_eq!(done.skipped[0].path, loras(&b).join(OLD));
    assert!(!done.skipped[0].reason.is_empty());
    assert_eq!(std::fs::read(loras(&b).join(KEPT)).unwrap(), b"their own file", "never overwritten");
    assert!(w.is_link(&loras(&b).join(OLD)), "B keeps its link");
    assert_eq!(done.renamed.len(), 1, "C still changed");
    assert_eq!(done.renamed[0].install_id, c.id);
    assert!(w.is_link(&loras(&c).join(KEPT)));
}

#[test]
fn a_link_windows_refuses_to_make_is_skipped_and_the_rest_go_on() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let c = w.add_install("C");
    link_by_hand(&w, &c, "models/loras", OLD);
    w.platform.fail_symlink_at(loras(&b).join(KEPT), VaultError::new(ErrorCode::PermissionDenied, "no"));

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(done.skipped.len(), 1);
    assert_eq!(done.skipped[0].install_id, b.id);
    assert!(w.is_link(&loras(&b).join(OLD)), "B keeps its only link");
    assert!(w.store.link_at_path(&loras(&b).join(OLD)).unwrap().is_some());
    assert_eq!(done.renamed.len(), 1);
    assert_eq!(done.renamed[0].install_id, c.id);
}

#[test]
fn an_old_link_windows_will_not_remove_stops_the_job_with_both_names_working() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let old_record = w.store.link_at_path(&loras(&b).join(OLD)).unwrap().unwrap();
    w.platform.fail_remove_symlink_at(loras(&b).join(OLD), std::io::ErrorKind::PermissionDenied);

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    let stop = done.stopped.clone().expect("the job stopped");
    assert_eq!(stop.install_id.as_deref(), Some(b.id.as_str()));
    assert_eq!(stop.path, loras(&b).join(OLD));
    assert!(!stop.message.is_empty());
    assert!(done.renamed.is_empty(), "nothing finished: {:?}", done.renamed);

    // Both names load the model, and both are recorded.
    assert_eq!(w.read(&loras(&b).join(OLD)), weights("same"));
    assert_eq!(w.read(&loras(&b).join(KEPT)), weights("same"));
    assert!(w.store.link_at_path(&loras(&b).join(OLD)).unwrap().is_some());
    assert!(w.store.link_at_path(&loras(&b).join(KEPT)).unwrap().is_some());
    // Stopped before the vault, so the vault still keeps both names.
    assert_eq!(w.store.vault_file(&sha()).unwrap().unwrap().aliases, vec![OLD.to_string()]);

    // The undo puts back what was there.
    unify(&w).undo(&done.unify_id).unwrap();
    assert!(std::fs::symlink_metadata(loras(&b).join(KEPT)).is_err(), "the new link is gone");
    assert!(w.store.link_at_path(&loras(&b).join(KEPT)).unwrap().is_none());
    assert_eq!(w.read(&loras(&b).join(OLD)), weights("same"));
    assert_eq!(w.store.link_at_path(&loras(&b).join(OLD)).unwrap().unwrap(), old_record, "the old record is untouched");
}

#[test]
fn a_stopped_job_is_finished_by_running_it_again() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    w.platform.fail_remove_symlink_at(loras(&b).join(OLD), std::io::ErrorKind::PermissionDenied);
    assert!(unify(&w).unify(&sha(), KEPT).unwrap().stopped.is_some());

    // Windows lets go. The plan now says the old name goes.
    w.platform.clear_remove_symlink_failure(&loras(&b).join(OLD));
    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(action_at(&plan, &loras(&b).join(OLD)), UnifyAction::Remove);
    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert!(done.stopped.is_none());
    assert_eq!(done.removed, vec![RemovedLink { install_id: b.id.clone(), path: loras(&b).join(OLD) }]);
    assert_eq!(names_in_use(&w), vec![KEPT.to_string()]);
    assert!(w.store.vault_file(&sha()).unwrap().unwrap().aliases.is_empty());
}

#[test]
fn a_running_comfyui_for_an_install_that_changes_stops_the_job_before_anything_changes() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    w.platform.set_processes(vec![comfy_process(&b.root)]);
    let before = snapshot(&w);

    let err = unify(&w).unify(&sha(), KEPT).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.message, "Close B first");
    assert_eq!(snapshot(&w), before, "nothing changed");
}

// --- undo ------------------------------------------------------------------

#[test]
fn undo_puts_back_every_old_name_and_the_vault_name_as_they_were() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let a_old = loras(&a).join(KEPT);
    let before_a = w.store.link_at_path(&a_old).unwrap().unwrap();
    let before_b = w.store.link_at_path(&loras(&b).join(OLD)).unwrap().unwrap();
    let before_file = w.store.vault_file(&sha()).unwrap().unwrap();
    let done = unify(&w).unify(&sha(), OLD).unwrap();
    assert_eq!(done.renamed.len(), 1, "A was renamed");
    assert_eq!(done.vault_name, OLD, "and so was the vault file");

    let undone = unify(&w).undo(&done.unify_id).unwrap();
    assert!(undone.undone);
    assert_eq!(w.read(&a_old), weights("same"), "A's old name loads the model again");
    assert!(std::fs::symlink_metadata(loras(&a).join(OLD)).is_err(), "A's new name is gone");

    // The vault is as it was: the same real name and the same second name.
    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert_eq!((file.canonical_name, file.aliases), (before_file.canonical_name, before_file.aliases));
    let real = w.vault_root.join("loras").join(KEPT);
    assert!(real.is_file() && !w.is_link(&real));
    assert!(w.is_link(&w.vault_root.join("loras").join(OLD)), "the second name is a link beside it again");

    // Each link has its own record back, pointing straight at the real file.
    assert_eq!(w.store.link_at_path(&a_old).unwrap().unwrap(), before_a, "A's record, as the run made it");
    assert_eq!(w.store.link_at_path(&loras(&b).join(OLD)).unwrap().unwrap(), before_b);
    for l in [&a_old, &loras(&b).join(OLD)] {
        assert_eq!(
            crate::paths::compare_key(&w.platform.read_symlink(l).unwrap()),
            crate::paths::compare_key(&real),
            "{} goes straight to the file",
            l.display()
        );
    }

    // The card is back.
    assert_eq!(Vault::new(&w.store, &w.platform).name_groups().unwrap().len(), 1);
    // A second undo finds nothing left to do.
    unify(&w).undo(&done.unify_id).unwrap();
    assert_eq!(w.read(&a_old), weights("same"));
}

#[test]
fn undo_takes_away_a_name_the_job_gave_the_vault() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let third = "third.safetensors";
    link_by_hand(&w, &b, "models/loras/t", third);
    let before = w.store.vault_file(&sha()).unwrap().unwrap();

    let done = unify(&w).unify(&sha(), third).unwrap();
    assert_eq!(done.vault_name, third);
    unify(&w).undo(&done.unify_id).unwrap();

    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    assert_eq!((file.canonical_name, file.aliases), (before.canonical_name, before.aliases));
    assert!(std::fs::symlink_metadata(w.vault_root.join("loras").join(third)).is_err());
}

#[test]
fn undo_never_puts_an_old_name_over_something_that_took_its_place() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    w.write_model(&b, &format!("models/loras/{OLD}"), b"a new file with the old name");
    let before = snapshot(&w);

    let err = unify(&w).undo(&done.unify_id).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.detail.unwrap_or_default().contains(OLD));
    assert_eq!(snapshot(&w), before, "nothing changed");
    assert_eq!(std::fs::read(loras(&b).join(OLD)).unwrap(), b"a new file with the old name");
}

#[test]
fn undo_leaves_a_link_that_is_no_longer_this_jobs() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    // The person replaces the new link with a file of their own.
    let new = loras(&b).join(KEPT);
    w.platform.remove_symlink(&new).unwrap();
    std::fs::write(&new, b"theirs").unwrap();

    unify(&w).undo(&done.unify_id).unwrap();
    assert_eq!(std::fs::read(&new).unwrap(), b"theirs");
    assert_eq!(w.read(&loras(&b).join(OLD)), weights("same"));
}

#[test]
fn undo_of_a_model_since_deleted_is_refused() {
    let w = TestWorld::new();
    two_names(&w);
    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    Vault::new(&w.store, &w.platform).delete_file_and_links(&sha(), &sha()).unwrap();

    let err = unify(&w).undo(&done.unify_id).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(unify(&w).undo("unify-nothing").unwrap_err().code, ErrorCode::NotFound);
}

fn revert_ap1(w: &TestWorld) -> Result<crate::store::ApplyRecord> {
    Applier::new(&w.store, &w.platform).revert("ap-1", &CancelToken::new(), &NullSink)
}

/// Both installs hold their own real file again, under the names they had.
fn assert_back_as_before_the_consolidation(w: &TestWorld, a: &Install, b: &Install) {
    for p in [loras(a).join(KEPT), loras(b).join(OLD)] {
        assert!(!w.is_link(&p), "{} is a real file again", p.display());
        assert_eq!(std::fs::read(&p).unwrap(), weights("same"));
    }
    for p in [loras(a).join(OLD), loras(b).join(KEPT)] {
        assert!(std::fs::symlink_metadata(&p).is_err(), "{} is gone", p.display());
    }
}

#[test]
fn undoing_the_consolidation_puts_back_a_name_change_nobody_undid() {
    // The card has no undo button. Undoing the consolidation must still work
    // after it, or the name change quietly took that undo away.
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    unify(&w).unify(&sha(), KEPT).unwrap();

    Applier::new(&w.store, &w.platform).preview_revert("ap-1").expect("the preview does not refuse");
    revert_ap1(&w).unwrap();
    assert_back_as_before_the_consolidation(&w, &a, &b);
}

#[test]
fn undoing_the_consolidation_puts_back_a_name_change_that_renamed_the_vault_file() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let done = unify(&w).unify(&sha(), OLD).unwrap();
    assert_eq!(done.vault_name, OLD);

    Applier::new(&w.store, &w.platform).preview_revert("ap-1").expect("the preview does not refuse");
    revert_ap1(&w).unwrap();
    assert_back_as_before_the_consolidation(&w, &a, &b);
}

#[test]
fn undoing_the_consolidation_works_after_undoing_a_name_change_that_renamed_the_vault_file() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let done = unify(&w).unify(&sha(), OLD).unwrap();
    unify(&w).undo(&done.unify_id).unwrap();
    revert_ap1(&w).unwrap();
    assert_back_as_before_the_consolidation(&w, &a, &b);
}

#[test]
fn two_name_changes_are_put_back_newest_first() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    // The Library's rename button first, then the card back to the first
    // name, which renames the vault file a second time.
    Vault::new(&w.store, &w.platform).set_canonical_name(&sha(), OLD).unwrap();
    let second = unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(second.vault_name, KEPT);

    revert_ap1(&w).unwrap();
    for p in [loras(&a).join(KEPT), loras(&b).join(OLD)] {
        assert!(!w.is_link(&p), "{} is a real file again", p.display());
        assert_eq!(std::fs::read(&p).unwrap(), weights("same"));
    }
}

#[test]
fn a_name_change_that_cannot_be_put_back_refuses_the_consolidation_undo_with_nothing_changed() {
    let w = TestWorld::new();
    two_names(&w);
    unify(&w).unify(&sha(), OLD).unwrap();
    // The vault file's first name is free after the job, and a file of the
    // person's own now sits there.
    std::fs::write(w.vault_root.join("loras").join(KEPT), b"dropped in by hand").unwrap();
    let before = snapshot(&w);

    let err = revert_ap1(&w).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("name changed after it"), "{}", err.message);
    assert_eq!(snapshot(&w), before, "nothing changed");
    assert_eq!(w.store.apply("ap-1").unwrap().unwrap().state, crate::store::ApplyState::Completed);
}

// --- a crash in the middle -------------------------------------------------

#[test]
fn a_crash_between_the_new_link_and_its_record_is_finished_when_the_vault_opens() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    w.store.fault_next_link_write(crate::store::LinkWriteFault::Crash);
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unify(&w).unify(&sha(), KEPT)));
    assert!(crashed.is_err(), "the fault did not fire");
    let new = loras(&b).join(KEPT);
    assert!(w.is_link(&new), "the link was made before the crash");
    assert!(w.store.link_at_path(&new).unwrap().is_none(), "and its record was not");

    assert_eq!(unify(&w).finish_interrupted().unwrap(), 1);
    let rec = w.store.link_at_path(&new).unwrap().expect("recorded at open");
    assert_eq!(rec.install_id, b.id);
    assert_eq!(rec.sha256, sha());
    assert!(w.is_link(&loras(&b).join(OLD)), "the old link was never removed");

    // The journal has no step left pending, and running the job again
    // finishes it.
    let id = w.store.journal_ids().unwrap().into_iter().find(|j| j.starts_with(UNIFY_JOURNAL_PREFIX)).unwrap();
    assert!(w.store.journal(&id).unwrap().iter().all(|e| e.state != JournalState::Pending));
    unify(&w).unify(&sha(), KEPT).unwrap();
    assert_eq!(names_in_use(&w), vec![KEPT.to_string()]);
}

#[test]
fn a_crash_after_the_old_link_went_forgets_its_record_when_the_vault_opens() {
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let old = loras(&b).join(OLD);
    let record = w.store.link_at_path(&old).unwrap().unwrap();
    let job = UnifyJob {
        unify_id: "unify-cut".into(),
        sha256: sha(),
        name: KEPT.into(),
        started_at: Timestamp::now(),
        replaced: vec![record.clone()],
        ..Default::default()
    };
    w.store.put_unify_job(&job).unwrap();
    let vault_path = w.vault_root.join("loras").join(KEPT);
    let mut j = Journal { store: &w.store, id: job.unify_id.clone(), next: 0 };
    let made = j.pending(&b.id, JournalStep::CreateLink { link: loras(&b).join(KEPT), target: vault_path.clone() }).unwrap();
    w.platform.create_file_symlink(&loras(&b).join(KEPT), &vault_path).unwrap();
    let file = w.store.vault_file(&sha()).unwrap().unwrap();
    w.store.put_link(&unify(&w).record_for(&b.id, &loras(&b).join(KEPT), &file, "unify-cut")).unwrap();
    j.finish(made, JournalState::Done).unwrap();
    j.pending(&b.id, JournalStep::RemoveLink { link: old.clone(), target: vault_path }).unwrap();
    w.platform.remove_symlink(&old).unwrap();
    // The power goes here, before the record is forgotten.

    unify(&w).finish_interrupted().unwrap();
    assert!(w.store.link_at_path(&old).unwrap().is_none());
    let id = "unify-cut";
    assert!(w.store.journal(id).unwrap().iter().all(|e| e.state == JournalState::Done));

    // And its undo still brings the old name back with its record.
    unify(&w).undo("unify-cut").unwrap();
    assert_eq!(w.store.link_at_path(&old).unwrap().unwrap().id, record.id);
    assert_eq!(w.read(&old), weights("same"));
}

#[test]
fn a_link_in_a_models_folder_that_is_a_junction_is_renamed_in_place() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, &format!("models/loras/{KEPT}"), &weights("same"));
    let plan = w.plan(&[a.clone()]);
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
    let b = w.add_install("B");
    let other_drive = w.path().join("D-drive").join("loras");
    std::fs::create_dir_all(&other_drive).unwrap();
    std::fs::create_dir_all(b.root.join("models")).unwrap();
    crate::links::tests::junction(&loras(&b), &other_drive);
    let b = w.refresh(&b);
    link_by_hand(&w, &b, "models/loras", OLD);

    // The link is recorded where it really is, on the other drive.
    let recorded = w.store.links_for_install(&b.id).unwrap().remove(0).abs_path;
    assert_eq!(
        crate::paths::compare_key(recorded.parent().unwrap()),
        crate::paths::compare_key(&crate::paths::canonicalize_clean(&other_drive).unwrap())
    );

    let plan = unify(&w).plan(&sha(), KEPT).unwrap();
    assert_eq!(action_at(&plan, &recorded), UnifyAction::Rename);
    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    assert!(done.stopped.is_none() && done.skipped.is_empty(), "{done:?}");
    assert_eq!(w.read(&other_drive.join(KEPT)), weights("same"));
    assert!(std::fs::symlink_metadata(other_drive.join(OLD)).is_err());

    unify(&w).undo(&done.unify_id).unwrap();
    assert_eq!(w.read(&other_drive.join(OLD)), weights("same"));
    assert!(std::fs::symlink_metadata(other_drive.join(KEPT)).is_err());
}

#[cfg(windows)]
#[test]
fn a_link_windows_really_holds_open_stops_the_job_and_undo_brings_everything_back() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let w = TestWorld::new();
    let (_a, b) = two_names(&w);
    let old = loras(&b).join(OLD);
    // A handle on the link itself that shares nothing, so Windows refuses to
    // delete it: a real sharing violation, not one a test double makes up.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&old)
        .unwrap();

    let done = unify(&w).unify(&sha(), KEPT).unwrap();
    let stop = done.stopped.clone().expect("Windows refused, so the job stopped");
    assert_eq!(stop.path, old);
    assert!(stop.message.contains("open"), "{}", stop.message);
    assert_eq!(w.read(&loras(&b).join(KEPT)), weights("same"), "the new name loads the model");
    drop(held);
    assert_eq!(w.read(&old), weights("same"), "and so does the old one");

    unify(&w).undo(&done.unify_id).unwrap();
    assert!(std::fs::symlink_metadata(loras(&b).join(KEPT)).is_err());
    assert_eq!(w.read(&old), weights("same"));
}
