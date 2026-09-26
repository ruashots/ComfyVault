//! Vault cleanup and health tests.

use super::*;
use crate::apply::{ApplyRequest, Applier, VerifyModeArg};
use crate::links::CreateLinkRequest;
use crate::progress::NullSink;
use crate::testkit::{weights, weights_hash, TestWorld};


fn vault<'a>(w: &'a TestWorld) -> Vault<'a> {
    Vault::new(&w.store, &w.platform)
}

/// Consolidates one content that two installs hold under different names, which
/// is what produces a vault file with a second name.
fn two_names(w: &TestWorld) -> (crate::install::Install, crate::install::Install) {
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/my-favourite.safetensors", &weights("same"));

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

// --- listing ---------------------------------------------------------------

#[test]
fn a_consolidated_model_is_listed_with_its_links_and_names() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let sha = weights_hash("same");

    let f = vault(&w).file(&sha).unwrap().unwrap();
    assert_eq!(f.canonical_name, "lora1.safetensors");
    assert_eq!(f.aliases, vec!["my-favourite.safetensors"]);
    assert_eq!(f.link_count, 2);
    assert!(f.present);
    assert_eq!(f.size_bytes, weights("same").len() as u64);

    let installs: Vec<&str> = f.links.iter().map(|l| l.install_id.as_str()).collect();
    assert!(installs.contains(&a.id.as_str()));
    assert!(installs.contains(&b.id.as_str()));
}

#[test]
fn the_listing_pages_filters_and_sorts() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    for n in 0..5 {
        w.write_model(&i, &format!("models/loras/m{n}.safetensors"), &weights(&format!("m{n}")));
    }
    w.write_model(&i, "models/checkpoints/c.safetensors", &weights("c"));

    let plan = w.plan(&[i]);
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

    let v = vault(&w);
    let all = v.list(0, 100, &VaultFilter::default(), VaultSort::Name, false).unwrap();
    assert_eq!(all.total, 6);

    let page = v.list(2, 2, &VaultFilter::default(), VaultSort::Name, false).unwrap();
    assert_eq!(page.files.len(), 2);
    assert_eq!(page.total, 6, "the total counts everything, not just the page");
    assert_eq!(page.offset, 2);

    let loras = v
        .list(0, 100, &VaultFilter { category: Some("loras".into()), ..Default::default() }, VaultSort::Name, false)
        .unwrap();
    assert_eq!(loras.total, 5);

    let named = v
        .list(0, 100, &VaultFilter { name_contains: Some("M3".into()), ..Default::default() }, VaultSort::Name, false)
        .unwrap();
    assert_eq!(named.total, 1, "the search must ignore case");
}

#[test]
fn a_listing_never_returns_more_than_the_page_limit() {
    let w = TestWorld::new();
    let page = vault(&w)
        .list(0, 999_999, &VaultFilter::default(), VaultSort::Name, false)
        .unwrap();
    assert!(page.files.len() <= MAX_PAGE);
}

// --- names -----------------------------------------------------------------

#[test]
fn a_content_with_two_names_appears_in_the_name_groups() {
    let w = TestWorld::new();
    two_names(&w);

    let groups = vault(&w).name_groups().unwrap();
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.canonical_name, "lora1.safetensors");
    assert_eq!(g.names.len(), 2);

    let canonical = g.names.iter().find(|n| n.is_canonical).unwrap();
    assert_eq!(canonical.name, "lora1.safetensors");
    assert_eq!(canonical.used_by_links, 2, "both installs resolve through the real file");

    let alias = g.names.iter().find(|n| !n.is_canonical).unwrap();
    assert_eq!(alias.name, "my-favourite.safetensors");
    assert_eq!(alias.used_by_links, 0, "install links point at the real file, not at the name");
}

#[test]
fn a_content_with_one_name_is_not_a_name_group() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/only.safetensors", &weights("only"));
    let plan = w.plan(&[a]);
    Applier::new(&w.store, &w.platform)
        .apply("ap-1", &plan, &ApplyRequest {
            plan_id: plan.plan_id.clone(),
            group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
            verify: VerifyModeArg::SizeAndMtime,
            stop_on_error: false,
        }, &CancelToken::new(), &NullSink)
        .unwrap();

    assert!(vault(&w).name_groups().unwrap().is_empty());
}

#[test]
fn the_person_can_choose_which_name_the_vault_keeps() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let sha = weights_hash("same");

    let updated = vault(&w).set_canonical_name(&sha, "my-favourite.safetensors").unwrap();
    assert_eq!(updated.canonical_name, "my-favourite.safetensors");
    assert_eq!(updated.aliases, vec!["lora1.safetensors"]);

    // The chosen name is now the real file.
    let new_real = w.vault_root.join("loras/my-favourite.safetensors");
    assert!(new_real.is_file());
    assert!(!w.is_link(&new_real));
    assert_eq!(std::fs::read(&new_real).unwrap(), weights("same"));

    // The old name stays, as a link, so a workflow that used it still works.
    let old = w.vault_root.join("loras/lora1.safetensors");
    assert!(w.is_link(&old));
    assert_eq!(w.read(&old), weights("same"));

    // Both installs still read their model.
    for (install, name) in [(&a, "lora1.safetensors"), (&b, "my-favourite.safetensors")] {
        let p = install.root.join(format!("models/loras/{name}"));
        assert!(w.is_link(&p));
        assert_eq!(w.read(&p), weights("same"), "{} stopped working", p.display());
    }
}

#[test]
fn install_links_are_repointed_so_nothing_resolves_through_two_links() {
    // A link to a link works, but it is fragile: removing the middle name would
    // break every install at once.
    let w = TestWorld::new();
    let (a, _b) = two_names(&w);
    let sha = weights_hash("same");

    vault(&w).set_canonical_name(&sha, "my-favourite.safetensors").unwrap();

    let p = a.root.join("models/loras/lora1.safetensors");
    let target = w.link_target(&p).unwrap();
    assert_eq!(
        target,
        w.vault_root.join("loras/my-favourite.safetensors"),
        "the install link must point straight at the real file"
    );
    assert!(!w.is_link(&target), "and that must be the real file, not another link");

    let record = w.store.link_at_path(&p).unwrap().unwrap();
    assert_eq!(record.vault_rel_path, PathBuf::from("loras/my-favourite.safetensors"));
}

#[test]
fn choosing_the_name_it_already_has_changes_nothing() {
    let w = TestWorld::new();
    two_names(&w);
    let sha = weights_hash("same");

    let f = vault(&w).set_canonical_name(&sha, "lora1.safetensors").unwrap();
    assert_eq!(f.canonical_name, "lora1.safetensors");
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file());
}

#[test]
fn choosing_a_name_the_model_does_not_have_is_refused() {
    let w = TestWorld::new();
    two_names(&w);
    let err = vault(&w)
        .set_canonical_name(&weights_hash("same"), "invented.safetensors")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file(), "nothing may change");
}

#[test]
fn a_name_can_be_removed_and_the_file_stays() {
    let w = TestWorld::new();
    two_names(&w);
    let sha = weights_hash("same");
    let alias_path = w.vault_root.join("loras/my-favourite.safetensors");
    assert!(w.is_link(&alias_path));

    vault(&w).remove_alias(&sha, "my-favourite.safetensors").unwrap();

    assert!(!alias_path.exists());
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file(), "the file itself stays");
    assert!(vault(&w).file(&sha).unwrap().unwrap().aliases.is_empty());
}

#[test]
fn the_name_the_vault_keeps_cannot_be_removed() {
    let w = TestWorld::new();
    two_names(&w);
    let err = vault(&w)
        .remove_alias(&weights_hash("same"), "lora1.safetensors")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("Choose another name to keep first"));
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file());
}

#[test]
fn a_name_an_install_still_uses_is_kept_and_the_links_are_named() {
    let w = TestWorld::new();
    let (_a, _b) = two_names(&w);
    let sha = weights_hash("same");

    // Make an install link resolve through the second name.
    let c = w.add_install("C");
    crate::links::Links::new(&w.store, &w.platform)
        .create(&CreateLinkRequest {
            install_id: c.id.clone(),
            sha256: sha.clone(),
            relative_dir: "models/loras".into(),
            link_name: None,
            create_dir: true,
        })
        .unwrap();
    let mut link = w.store.links_for_install(&c.id).unwrap().pop().unwrap();
    link.vault_rel_path = PathBuf::from("loras/my-favourite.safetensors");
    w.store.put_link(&link).unwrap();

    let err = vault(&w).remove_alias(&sha, "my-favourite.safetensors").unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.detail.unwrap().contains("models"), "the person needs to know which links");
    assert!(w.vault_root.join("loras/my-favourite.safetensors").exists());
}

// --- orphans and deleting --------------------------------------------------

#[test]
fn a_model_nothing_links_to_is_listed_as_an_orphan() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    assert!(vault(&w).orphans().unwrap().is_empty(), "both installs still use it");

    // The person deletes both links by hand, outside the app.
    for install in [&a, &b] {
        for e in std::fs::read_dir(install.root.join("models/loras")).unwrap().flatten() {
            std::fs::remove_file(e.path()).unwrap();
        }
    }

    let orphans = vault(&w).orphans().unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].link_count, 0);
}

#[test]
fn deleting_a_vault_file_needs_the_hash_repeated_back() {
    let w = TestWorld::new();
    two_names(&w);
    let sha = weights_hash("same");

    let err = vault(&w).delete_file(&sha, "yes").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(err.message.contains("not confirmed"));
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file());
}

#[test]
fn a_model_an_install_still_links_to_is_never_deleted() {
    let w = TestWorld::new();
    two_names(&w);
    let sha = weights_hash("same");

    let err = vault(&w).delete_file(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("Remove those links first"));
    assert!(w.vault_root.join("loras/lora1.safetensors").is_file());
}

#[test]
fn an_orphan_can_be_deleted_and_every_name_goes_with_it() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let sha = weights_hash("same");

    for install in [&a, &b] {
        for e in std::fs::read_dir(install.root.join("models/loras")).unwrap().flatten() {
            std::fs::remove_file(e.path()).unwrap();
        }
    }

    let freed = vault(&w).delete_file(&sha, &sha).unwrap();
    assert_eq!(freed, weights("same").len() as u64);
    assert!(!w.vault_root.join("loras/lora1.safetensors").exists());
    assert!(
        !w.vault_root.join("loras/my-favourite.safetensors").exists(),
        "the second name must go too, or it would point at nothing"
    );
    assert!(w.store.vault_file(&sha).unwrap().is_none());
}

// --- deleting a model together with its links -------------------------------

/// One model held three times in two installs, one copy under another name,
/// consolidated. The vault ends up with the file, a second name beside it,
/// and three links out in the installs.
fn three_links_two_installs(w: &TestWorld) -> (Vec<PathBuf>, String) {
    let a = w.add_install("A");
    let b = w.add_install("B");
    let places = vec![
        w.write_model(&a, "models/loras/m.safetensors", &weights("m")),
        w.write_model(&a, "models/loras/sub/m.safetensors", &weights("m")),
        w.write_model(&b, "models/loras/other-name.safetensors", &weights("m")),
    ];
    consolidate_all(w, &[a, b]);
    for p in &places {
        assert!(w.is_link(p), "{p:?} should be a link after the run");
    }
    (places, weights_hash("m"))
}

fn consolidate_all(w: &TestWorld, installs: &[crate::install::Install]) {
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

fn vault_file_of(w: &TestWorld, sha: &str) -> PathBuf {
    w.vault_root.join(w.store.vault_file(sha).unwrap().unwrap().vault_rel_path())
}

fn alias_of(w: &TestWorld, sha: &str) -> PathBuf {
    let r = w.store.vault_file(sha).unwrap().unwrap();
    assert_eq!(r.aliases.len(), 1, "the setup gives the model a second name");
    w.vault_root.join(r.alias_rel_path(&r.aliases[0]))
}

/// Whether an error's text names this path, written either way round.
fn names(text: &str, p: &Path) -> bool {
    crate::paths::compare_key(Path::new(text)).contains(&crate::paths::compare_key(p))
}

/// Everything a refused delete must leave exactly as it was.
fn assert_untouched(w: &TestWorld, places: &[PathBuf], sha: &str, file: &Path) {
    assert!(file.is_file(), "the vault file must stay");
    assert_eq!(std::fs::read(file).unwrap(), weights("m"));
    for p in places {
        assert!(w.is_link(p), "{p:?} is no longer a link");
        assert_eq!(w.read(p), weights("m"), "{p:?} no longer loads the model");
    }
    assert!(w.store.vault_file(sha).unwrap().is_some(), "the vault record must stay");
    assert_eq!(w.store.links_for_hash(sha).unwrap().len(), places.len(), "the link records must stay");
}

#[test]
fn a_model_is_deleted_with_every_link_to_it_in_every_install() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);

    let done = vault(&w).delete_file_and_links(&sha, &sha).unwrap();

    assert_eq!(done.bytes_freed, weights("m").len() as u64);
    let mut reported = done.links_removed.clone();
    reported.sort();
    let mut expected = places.clone();
    expected.sort();
    assert_eq!(reported, expected, "every install link is reported, and nothing else");

    for p in &places {
        assert!(std::fs::symlink_metadata(p).is_err(), "{p:?} is still there");
    }
    assert!(std::fs::symlink_metadata(&file).is_err(), "the vault file is still there");
    assert!(std::fs::symlink_metadata(&alias).is_err(), "the second name is still there");
    assert!(w.store.vault_file(&sha).unwrap().is_none());
    assert!(w.store.links_for_hash(&sha).unwrap().is_empty(), "the link records must go too");
    assert!(vault(&w).orphans().unwrap().is_empty(), "a deleted model is not an orphan");
    assert_eq!(vault(&w).list(0, 100, &Default::default(), VaultSort::Name, false).unwrap().total, 0);
    assert!(vault(&w).health().unwrap().ok, "nothing is left dangling");
}

#[test]
fn a_link_in_an_install_that_is_no_longer_registered_goes_too() {
    // Removing an install from the list leaves its links in place. Left
    // behind by a delete, they would point at nothing.
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let b = w.store.installs().unwrap().into_iter().find(|i| i.label == "B").unwrap();
    assert!(w.store.delete_install(&b.id).unwrap());

    let done = vault(&w).delete_file_and_links(&sha, &sha).unwrap();
    assert!(done.links_removed.contains(&places[2]));
    assert!(std::fs::symlink_metadata(&places[2]).is_err(), "the unregistered install's link is still there");
    assert!(w.store.links_for_hash(&sha).unwrap().is_empty());
}

#[test]
fn every_removal_is_journaled_and_the_file_goes_last() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);

    vault(&w).delete_file_and_links(&sha, &sha).unwrap();

    let id = w
        .store
        .journal_ids()
        .unwrap()
        .into_iter()
        .find(|i| i.starts_with("delete-"))
        .expect("the delete keeps a journal");
    let journal = w.store.journal(&id).unwrap();
    assert!(journal.iter().all(|e| e.state == crate::store::JournalState::Done));
    assert_eq!(journal.len(), places.len() + 2, "three links, one name, one file");

    let removed_links: Vec<&PathBuf> = journal[..places.len()]
        .iter()
        .map(|e| match &e.step {
            JournalStep::RemoveLink { link, .. } => link,
            other => panic!("an install link must go first, found {other:?}"),
        })
        .collect();
    for p in &places {
        assert!(removed_links.contains(&p));
    }
    assert!(
        matches!(&journal[places.len()].step, JournalStep::RemoveLink { link, .. } if *link == alias),
        "the second name goes after the install links"
    );
    assert!(
        matches!(&journal[places.len() + 1].step, JournalStep::DeleteVaultFile { path, .. } if *path == file),
        "the file goes last"
    );
}

#[test]
fn without_being_asked_to_remove_links_a_linked_model_is_still_refused() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);

    let err = vault(&w).delete_file(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_untouched(&w, &places, &sha, &file);
}

#[test]
fn a_delete_with_links_needs_the_hash_repeated_back() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);

    let err = vault(&w).delete_file_and_links(&sha, &weights_hash("other")).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert_untouched(&w, &places, &sha, &file);
}

#[test]
fn a_real_file_where_a_link_was_refuses_the_whole_delete() {
    // Somebody replaced a link with a file of their own. It is not this
    // engine's to remove, and deleting the model around it is a surprise.
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);

    std::fs::remove_file(&places[1]).unwrap();
    std::fs::write(&places[1], b"somebody's own file").unwrap();

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    let detail = err.detail.clone().unwrap_or_default();
    assert!(names(&detail, &places[1]), "the path is not named: {detail}");

    assert_eq!(std::fs::read(&places[1]).unwrap(), b"somebody's own file");
    assert!(file.is_file());
    for p in [&places[0], &places[2]] {
        assert!(w.is_link(p));
        assert_eq!(w.read(p), weights("m"));
    }
    assert!(w.store.vault_file(&sha).unwrap().is_some());
    assert_eq!(w.store.links_for_hash(&sha).unwrap().len(), 3);
}

#[test]
fn a_link_that_now_leads_to_another_file_refuses_the_whole_delete() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);

    let elsewhere = w.path().join("elsewhere.safetensors");
    std::fs::write(&elsewhere, weights("another")).unwrap();
    std::fs::remove_file(&places[2]).unwrap();
    w.platform.create_file_symlink(&places[2], &elsewhere).unwrap();

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(names(&err.detail.unwrap_or_default(), &places[2]));

    assert!(w.is_link(&places[2]), "the other link is not the delete's to remove");
    assert_eq!(w.read(&places[2]), weights("another"));
    assert!(elsewhere.is_file());
    assert!(file.is_file());
    for p in [&places[0], &places[1]] {
        assert!(w.is_link(p));
    }
}

#[test]
fn a_second_name_that_is_no_longer_a_link_refuses_the_whole_delete() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);

    std::fs::remove_file(&alias).unwrap();
    std::fs::write(&alias, b"a real file under the second name").unwrap();

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(names(&err.detail.unwrap_or_default(), &alias));
    assert_eq!(std::fs::read(&alias).unwrap(), b"a real file under the second name");
    for p in &places {
        // The links that went through the old second name point at a real
        // file now, but none of them may have been removed.
        assert!(w.is_link(p), "{p:?} was removed");
    }
    assert!(file.is_file());
}

#[test]
fn a_vault_file_that_is_not_the_recorded_one_is_never_deleted() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);

    std::fs::write(&file, b"different bytes of a different size").unwrap();

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(std::fs::read(&file).unwrap(), b"different bytes of a different size");
    for p in &places {
        assert!(w.is_link(p), "{p:?} was removed");
    }
    assert!(w.store.vault_file(&sha).unwrap().is_some());
}

#[test]
fn a_model_another_program_holds_open_is_refused_before_any_link_goes() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    w.platform.lock_file(&file);

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::FileLocked);
    assert!(names(err.path.as_deref().unwrap_or_default(), &file));
    assert_untouched(&w, &places, &sha, &file);
}

#[test]
fn a_link_the_disk_refuses_to_remove_puts_back_the_ones_already_gone() {
    // The second name is removed after every install link, so failing it
    // proves the three install links come back.
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);
    w.platform.fail_remove_symlink_at(&alias, std::io::ErrorKind::PermissionDenied);

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert!(err.message.contains("Nothing was deleted"), "{}", err.message);
    assert!(names(err.path.as_deref().unwrap_or_default(), &alias));

    assert_untouched(&w, &places, &sha, &file);
    assert!(w.is_link(&alias));

    // The journal says what happened: removed, then put back.
    let id = w.store.journal_ids().unwrap().into_iter().find(|i| i.starts_with("delete-")).unwrap();
    let states: Vec<crate::store::JournalState> =
        w.store.journal(&id).unwrap().iter().map(|e| e.state).collect();
    use crate::store::JournalState::{Failed, Reverted};
    assert_eq!(states, vec![Reverted, Reverted, Reverted, Failed]);

    // Once the disk lets it, the same delete goes through.
    w.platform.clear_injections();
    vault(&w).delete_file_and_links(&sha, &sha).unwrap();
    assert!(!file.exists());
}

#[test]
fn a_link_that_cannot_be_put_back_is_named() {
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);
    w.platform.fail_remove_symlink_at(&alias, std::io::ErrorKind::PermissionDenied);
    w.platform.fail_symlink_at(&places[0], VaultError::new(ErrorCode::IoError, "refused by a test"));

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert!(err.message.contains("could not be put back"), "{}", err.message);
    assert!(names(&err.detail.unwrap_or_default(), &places[0]));

    assert!(file.is_file(), "the model itself is never deleted when a step fails");
    for p in [&places[1], &places[2]] {
        assert!(w.is_link(p));
        assert_eq!(w.read(p), weights("m"));
    }
}

#[cfg(unix)]
#[test]
fn a_vault_file_the_disk_refuses_to_delete_puts_every_link_back() {
    use std::os::unix::fs::PermissionsExt;
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let places = vec![
        w.write_model(&a, "models/loras/m.safetensors", &weights("m")),
        w.write_model(&b, "models/loras/m.safetensors", &weights("m")),
    ];
    consolidate_all(&w, &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    // A folder nobody may write to refuses the delete of a file inside it.
    let folder = file.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = vault(&w).delete_file_and_links(&sha, &sha);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();

    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert!(err.message.contains("Nothing was deleted"), "{}", err.message);
    assert_untouched(&w, &places, &sha, &file);
}

#[cfg(windows)]
#[test]
fn a_vault_file_windows_holds_open_puts_every_link_back() {
    // The fake says nobody holds the file, the way a check made a moment too
    // early would. A real handle then makes Windows refuse the delete itself.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let places = vec![
        w.write_model(&a, "models/loras/m.safetensors", &weights("m")),
        w.write_model(&b, "models/loras/m.safetensors", &weights("m")),
    ];
    consolidate_all(&w, &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    // Opened the way Python opens a file: others may read and write it, but
    // not delete it. Rust's own default would let the delete through.
    use std::os::windows::fs::OpenOptionsExt;
    let held = std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(&file).unwrap();
    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    drop(held);

    assert_eq!(err.code, ErrorCode::FileLocked, "{err:?}");
    assert!(err.message.contains("Nothing was deleted"), "{}", err.message);
    assert_untouched(&w, &places, &sha, &file);
}

#[test]
fn a_delete_cut_off_after_some_links_went_is_finished_by_running_it_again() {
    // What a crash between two removals leaves: a link gone from the disk,
    // its record still there.
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    std::fs::remove_file(&places[0]).unwrap();

    let done = vault(&w).delete_file_and_links(&sha, &sha).unwrap();
    assert_eq!(done.links_removed.len(), 2, "only the links that were still there");
    assert!(!file.exists());
    assert!(w.store.links_for_hash(&sha).unwrap().is_empty(), "the record of the gone link goes too");
    assert!(w.store.vault_file(&sha).unwrap().is_none());
}

#[test]
fn a_delete_cut_off_after_the_file_went_is_finished_by_running_it_again() {
    // What a crash between the file and the records leaves, and also what a
    // model whose file was lost looks like: links that point at nothing.
    let w = TestWorld::new();
    let (places, sha) = three_links_two_installs(&w);
    let file = vault_file_of(&w, &sha);
    let alias = alias_of(&w, &sha);
    std::fs::remove_file(&file).unwrap();

    let done = vault(&w).delete_file_and_links(&sha, &sha).unwrap();
    assert_eq!(done.bytes_freed, 0, "nothing was there to free");
    assert_eq!(done.links_removed.len(), 3);
    for p in places.iter().chain([&alias]) {
        assert!(std::fs::symlink_metadata(p).is_err(), "{p:?} was left pointing at nothing");
    }
    assert!(w.store.links_for_hash(&sha).unwrap().is_empty());
    assert!(w.store.vault_file(&sha).unwrap().is_none());
    assert!(vault(&w).health().unwrap().ok);

    // The delete reached its end, even with no file to remove, so the undo
    // says the model was deleted rather than that a delete stopped.
    let err = Applier::new(&w.store, &w.platform)
        .revert("ap-1", &CancelToken::new(), &NullSink)
        .unwrap_err();
    assert!(err.message.contains("deleted in Cleanup"), "{}", err.message);
}

#[test]
fn undoing_the_run_that_made_a_deleted_model_refuses_before_touching_anything() {
    // The run's steps describe a vault file that is gone. Undoing them would
    // stop part way. The delete's journal is what lets the undo see that.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let kept = vec![
        w.write_model(&a, "models/loras/keep.safetensors", &weights("keep")),
        w.write_model(&b, "models/loras/keep.safetensors", &weights("keep")),
    ];
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_all(&w, &[a, b]);

    let sha = weights_hash("m");
    vault(&w).delete_file_and_links(&sha, &sha).unwrap();

    let err = Applier::new(&w.store, &w.platform)
        .revert("ap-1", &CancelToken::new(), &NullSink)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "{err:?}");
    // The person is told why, not sent to undo a delete that cannot be undone.
    assert!(err.message.contains("deleted in Cleanup"), "{}", err.message);
    assert!(err.message.contains("can no longer be undone"), "{}", err.message);
    assert!(!err.message.contains("Undo the later change"), "{}", err.message);
    let detail = err.detail.clone().unwrap_or_default();
    assert!(detail.contains("m.safetensors"), "the deleted model is named: {detail}");
    assert!(!detail.contains("keep.safetensors"), "a model still there is not named: {detail}");

    // The preview asks the same question first, and gets the same answer.
    let preview = Applier::new(&w.store, &w.platform).preview_revert("ap-1").unwrap_err();
    assert_eq!(preview.message, err.message);
    for p in &kept {
        assert!(w.is_link(p), "{p:?} was put back by an undo that should have refused");
        assert_eq!(w.read(p), weights("keep"));
    }
    assert_eq!(
        w.store.apply("ap-1").unwrap().unwrap().state,
        crate::store::ApplyState::Completed,
        "the run must not be marked as partly undone"
    );
}

// --- a delete and what happens around it -------------------------------------

/// The fake platform, plus one action run just before a chosen link is
/// removed. It stands in for another command, or another program, acting
/// between a delete's checks and its removals.
struct Hooked<'a> {
    inner: &'a crate::platform::FakePlatform,
    before_remove: std::sync::Mutex<std::collections::HashMap<PathBuf, Box<dyn FnOnce() + Send + 'a>>>,
}

impl<'a> Hooked<'a> {
    fn new(inner: &'a crate::platform::FakePlatform) -> Self {
        Self { inner, before_remove: Default::default() }
    }

    fn before_removing(&self, link: &Path, f: impl FnOnce() + Send + 'a) {
        self.before_remove.lock().unwrap().insert(link.to_path_buf(), Box::new(f));
    }
}

impl crate::platform::Platform for Hooked<'_> {
    fn create_file_symlink(&self, link: &Path, target: &Path) -> Result<()> {
        self.inner.create_file_symlink(link, target)
    }
    fn remove_symlink(&self, link: &Path) -> Result<()> {
        let hook = self.before_remove.lock().unwrap().remove(link);
        if let Some(f) = hook {
            f();
        }
        self.inner.remove_symlink(link)
    }
    fn read_symlink(&self, link: &Path) -> Result<PathBuf> {
        self.inner.read_symlink(link)
    }
    fn symlink_capability(&self) -> crate::platform::SymlinkCapability {
        self.inner.symlink_capability()
    }
    fn lock_state(&self, path: &Path) -> crate::platform::LockState {
        self.inner.lock_state(path)
    }
    fn volume_id(&self, path: &Path) -> Result<crate::platform::VolumeId> {
        self.inner.volume_id(path)
    }
    fn file_identity(&self, path: &Path) -> Option<crate::platform::FileIdentity> {
        self.inner.file_identity(path)
    }
    fn disk_space(&self, path: &Path) -> Result<crate::platform::DiskSpace> {
        self.inner.disk_space(path)
    }
    fn list_processes(&self) -> Vec<crate::platform::ProcessInfo> {
        self.inner.list_processes()
    }
    fn listening_ports(&self, pids: &[u32]) -> std::collections::HashMap<u32, Vec<u16>> {
        self.inner.listening_ports(pids)
    }
    fn processes_holding(&self, pids: &[u32], files: &[PathBuf]) -> std::collections::HashMap<u32, bool> {
        self.inner.processes_holding(pids, files)
    }
    fn long_paths_enabled(&self) -> Option<bool> {
        self.inner.long_paths_enabled()
    }
    fn drive_roots(&self) -> Vec<PathBuf> {
        self.inner.drive_roots()
    }
    fn drives(&self) -> Vec<crate::platform::DriveInfo> {
        self.inner.drives()
    }
    fn rename(&self, from: &Path, to: &Path) -> std::result::Result<(), crate::platform::RenameError> {
        self.inner.rename(from, to)
    }
}

fn consolidate_as(w: &TestWorld, apply_id: &str, installs: &[crate::install::Install]) {
    let plan = w.plan(installs);
    Applier::new(&w.store, &w.platform)
        .apply(
            apply_id,
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

#[test]
fn a_delete_does_not_block_the_undo_of_a_run_made_after_it() {
    // A delete's journal belongs to no run. Counted as later than every run,
    // it refused the undo of any run after it that used the same paths, for
    // ever, saying the model had been deleted.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a.clone(), b.clone()]);
    let sha = weights_hash("m");
    vault(&w).delete_file_and_links(&sha, &sha).unwrap();

    // The same model comes back at the same places, and a new run takes it.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let a = w.refresh(&a);
    let b = w.refresh(&b);
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-2", &[a, b]);
    assert!(w.is_link(&pa) && w.is_link(&pb));

    let undone = Applier::new(&w.store, &w.platform).revert("ap-2", &CancelToken::new(), &NullSink);
    assert!(undone.is_ok(), "the undo of a run made after the delete was refused: {:?}", undone.err());
    assert_eq!(std::fs::read(&pa).unwrap(), weights("m"));
    assert!(!w.is_link(&pa), "the file is back in its place");

    // The control: the run the delete came after is still refused.
    let err = Applier::new(&w.store, &w.platform)
        .revert("ap-1", &CancelToken::new(), &NullSink)
        .unwrap_err();
    assert!(err.message.contains("deleted in Cleanup"), "{}", err.message);
}

#[test]
fn a_link_made_while_a_model_is_deleted_is_never_left_pointing_at_nothing() {
    // The Library can link the model into another install after the delete
    // read its links and before it removed the file.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    let hooked = Hooked::new(&w.platform);
    let (store, inner, sha2, c_id) = (&w.store, &w.platform, sha.clone(), c.id.clone());
    for p in [&pa, &pb] {
        let (sha2, c_id) = (sha2.clone(), c_id.clone());
        hooked.before_removing(p, move || {
            // Only the first removal finds no link in C yet.
            let _ = crate::links::Links::new(store, inner).create(&CreateLinkRequest {
                install_id: c_id,
                sha256: sha2,
                relative_dir: "models/loras".into(),
                link_name: None,
                create_dir: true,
            });
        });
    }

    let err = Vault::new(&w.store, &hooked).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "{err:?}");

    let pc = c.root.join("models/loras/m.safetensors");
    assert!(w.is_link(&pc), "the link made in the Library is gone");
    assert_eq!(w.read(&pc), weights("m"), "install C holds a link to nothing");
    assert!(file.is_file(), "the model was deleted from under the new link");
    for p in [&pa, &pb] {
        assert!(w.is_link(p), "{p:?} was not put back");
    }
    assert!(vault(&w).health().unwrap().ok);
}

#[test]
fn a_real_file_that_replaces_a_link_after_the_check_is_never_deleted() {
    // Removing a link removes a real file just as readily, so each link is
    // checked again right before it goes.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    // Whichever goes first swaps the other for the person's own file.
    let hooked = Hooked::new(&w.platform);
    for (now, other) in [(&pa, pb.clone()), (&pb, pa.clone())] {
        hooked.before_removing(now, move || {
            if std::fs::symlink_metadata(&other).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                std::fs::remove_file(&other).unwrap();
                std::fs::write(&other, b"the person's own file").unwrap();
            }
        });
    }

    let err = Vault::new(&w.store, &hooked).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "{err:?}");

    let (theirs, ours): (Vec<&PathBuf>, Vec<&PathBuf>) =
        [&pa, &pb].into_iter().partition(|p| !w.is_link(p));
    assert_eq!(theirs.len(), 1);
    assert_eq!(std::fs::read(theirs[0]).unwrap(), b"the person's own file", "the person's file was deleted");
    assert!(names(err.path.as_deref().unwrap_or_default(), theirs[0]));
    assert_eq!(w.read(ours[0]), weights("m"), "the removed link was not put back");
    assert!(file.is_file());
}

#[test]
fn a_delete_cut_off_part_way_is_reported_and_finished_by_running_it_again() {
    // The computer stops after one link went and before the file did. The
    // model is still in the vault, and one install lost it.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let pb = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    let hooked = Hooked::new(&w.platform);
    for (now, other) in [(&pa, pb.clone()), (&pb, pa.clone())] {
        hooked.before_removing(now, move || {
            if std::fs::symlink_metadata(&other).is_err() {
                panic!("the computer stopped");
            }
        });
    }
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Vault::new(&w.store, &hooked).delete_file_and_links(&sha, &sha)
    }));
    assert!(crashed.is_err(), "the stop did not happen");
    assert!(file.is_file());
    assert!(w.is_link(&pa) != w.is_link(&pb), "exactly one link went before the stop");

    // The health check says so, and names the model.
    let health = vault(&w).health().unwrap();
    assert!(!health.ok, "the health check says all is well");
    assert_eq!(health.stopped_deletes.len(), 1);
    assert_eq!(health.stopped_deletes[0].sha256, sha);

    // The undo does not claim the model was deleted: it is still there.
    let err = Applier::new(&w.store, &w.platform)
        .revert("ap-1", &CancelToken::new(), &NullSink)
        .unwrap_err();
    assert!(!err.message.contains("was deleted in Cleanup"), "{}", err.message);
    assert!(err.message.contains("stopped part way"), "{}", err.message);

    // Running the delete again finishes it, and then the undo says it was.
    vault(&w).delete_file_and_links(&sha, &sha).unwrap();
    assert!(!file.exists());
    let health = vault(&w).health().unwrap();
    assert!(health.stopped_deletes.is_empty());
    assert!(health.ok);
    let err = Applier::new(&w.store, &w.platform)
        .revert("ap-1", &CancelToken::new(), &NullSink)
        .unwrap_err();
    assert!(err.message.contains("deleted in Cleanup"), "{}", err.message);
}

#[test]
fn a_delete_that_put_its_links_back_is_not_a_stopped_delete() {
    let w = TestWorld::new();
    let (_places, sha) = three_links_two_installs(&w);
    let alias = alias_of(&w, &sha);
    w.platform.fail_remove_symlink_at(&alias, std::io::ErrorKind::PermissionDenied);
    vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();

    let health = vault(&w).health().unwrap();
    assert!(health.stopped_deletes.is_empty(), "every link is back, so nothing stopped");
    assert!(health.ok);
}

#[test]
fn a_record_that_points_outside_the_vault_deletes_nothing_there() {
    // A record written by an older build, or by hand, can name a folder
    // outside the vault. A file of the same size there is somebody's.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a, b]);

    let victim = w.path().join("victim/m.safetensors");
    std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
    std::fs::write(&victim, weights("m")).unwrap();
    let mut rec = w.store.vault_file(&weights_hash("m")).unwrap().unwrap();
    rec.sha256 = crate::scan::hash::normalize_sha256(&"ab".repeat(32)).unwrap();
    rec.category = "../victim".into();
    rec.aliases.clear();
    w.store.put_vault_file(&rec).unwrap();

    let err = vault(&w).delete_file_and_links(&rec.sha256, &rec.sha256).unwrap_err();
    assert!(victim.is_file(), "a file outside the vault was deleted");
    assert_eq!(err.code, ErrorCode::PathOutsideBoundary, "{err:?}");
}

#[test]
fn a_second_name_that_points_outside_the_vault_removes_nothing_there() {
    // A second name is a file name from the record. One that climbs out of
    // the vault must be refused as outside, before anything is looked at,
    // even when what it names is a link to this very model.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    consolidate_as(&w, "ap-1", &[a, b]);
    let sha = weights_hash("m");
    let file = vault_file_of(&w, &sha);

    let outside = w.path().join("outside/m.safetensors");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    w.platform.create_file_symlink(&outside, &file).unwrap();
    let mut rec = w.store.vault_file(&sha).unwrap().unwrap();
    rec.aliases = vec!["../../outside/m.safetensors".into()];
    w.store.put_vault_file(&rec).unwrap();

    let err = vault(&w).delete_file_and_links(&sha, &sha).unwrap_err();
    assert_eq!(err.code, ErrorCode::PathOutsideBoundary, "{err:?}");
    assert!(w.is_link(&outside), "a link outside the vault was removed");
    assert!(file.is_file());
}

#[test]
fn with_the_file_gone_a_link_to_another_model_is_kept() {
    // Running a delete again after its file went has nothing to follow, so a
    // link must name the file. One that now leads to another model is not
    // this delete's to remove.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&a, "models/loras/n.safetensors", &weights("n"));
    w.write_model(&b, "models/loras/n.safetensors", &weights("n"));
    consolidate_as(&w, "ap-1", &[a, b]);
    let m = weights_hash("m");
    let m_file = vault_file_of(&w, &m);
    let n_file = vault_file_of(&w, &weights_hash("n"));

    std::fs::remove_file(&m_file).unwrap();
    std::fs::remove_file(&pa).unwrap();
    w.platform.create_file_symlink(&pa, &n_file).unwrap();

    let err = vault(&w).delete_file_and_links(&m, &m).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(w.is_link(&pa), "a working link to another model was removed");
    assert_eq!(w.read(&pa), weights("n"));
}

// --- health ----------------------------------------------------------------

#[test]
fn a_healthy_vault_reports_nothing_wrong() {
    let w = TestWorld::new();
    two_names(&w);
    let h = vault(&w).health().unwrap();
    assert!(h.ok, "{h:?}");
    assert_eq!(h.checked_links, 2);
    assert_eq!(h.checked_files, 1);
    assert!(h.dangling_links.is_empty());
    assert!(h.foreign_files.is_empty(), "the vault holds only what the app put there");
}

#[test]
fn a_link_that_points_at_nothing_is_the_first_thing_reported() {
    // ComfyUI lists a dangling link in its model menu and then fails to load
    // it, and a custom node that re-downloads the missing model writes through
    // the link into the vault. It is worse than a missing file.
    let w = TestWorld::new();
    two_names(&w);
    std::fs::remove_file(w.vault_root.join("loras/lora1.safetensors")).unwrap();

    let h = vault(&w).health().unwrap();
    assert!(!h.ok);
    assert_eq!(h.dangling_links.len(), 2);
    assert_eq!(h.missing_vault_files.len(), 1);
}

#[test]
fn a_real_file_where_a_link_belonged_is_reported() {
    let w = TestWorld::new();
    let (a, _b) = two_names(&w);
    let p = a.root.join("models/loras/lora1.safetensors");
    std::fs::remove_file(&p).unwrap();
    std::fs::write(&p, b"the person downloaded it again").unwrap();

    let h = vault(&w).health().unwrap();
    assert!(!h.ok);
    assert_eq!(h.replaced_links.len(), 1);
    assert_eq!(h.replaced_links[0].abs_path, p);
    assert_eq!(
        std::fs::read(&p).unwrap(),
        b"the person downloaded it again",
        "the health check must only look"
    );
}

#[test]
fn a_file_the_app_did_not_put_in_the_vault_is_reported() {
    let w = TestWorld::new();
    two_names(&w);
    std::fs::write(w.vault_root.join("loras/dropped-in.safetensors"), b"x").unwrap();

    let h = vault(&w).health().unwrap();
    assert_eq!(h.foreign_files.len(), 1);
    assert!(h.foreign_files[0].contains("dropped-in.safetensors"));
}

#[test]
fn the_engines_own_folder_is_never_called_a_foreign_file() {
    let w = TestWorld::new();
    two_names(&w);
    let h = vault(&w).health().unwrap();
    assert!(
        !h.foreign_files.iter().any(|f| f.contains(".comfyvault")),
        "the database must not be reported as somebody's stray file: {:?}",
        h.foreign_files
    );
}

#[test]
fn dangling_links_can_be_cleared_in_one_go() {
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    std::fs::remove_file(w.vault_root.join("loras/lora1.safetensors")).unwrap();

    assert_eq!(vault(&w).remove_dangling_links().unwrap(), 2);

    for install in [&a, &b] {
        for e in std::fs::read_dir(install.root.join("models/loras")).unwrap().flatten() {
            panic!("a dead link was left behind: {}", e.path().display());
        }
    }
    assert!(w.store.links().unwrap().is_empty());
}

#[test]
fn grouping_by_content_is_pure_and_testable_without_a_disk() {
    let r = |sha: &str, name: &str| VaultFileRecord {
        sha256: sha.into(),
        canonical_name: name.into(),
        category: "loras".into(),
        size_bytes: 1,
        added_at: crate::time_util::Timestamp::now(),
        aliases: vec![],
    };
    let records = vec![r("AA", "one"), r("BB", "two"), r("AA", "three")];
    let grouped = group_by_content(&records);
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped["AA"].len(), 2);
    assert_eq!(grouped["BB"].len(), 1);
}

#[test]
fn asking_about_text_that_is_not_a_hash_is_refused() {
    let w = TestWorld::new();
    assert_eq!(vault(&w).file("nope").unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(
        vault(&w).set_canonical_name("nope", "x").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(vault(&w).remove_alias("nope", "x").unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(vault(&w).delete_file("nope", "nope").unwrap_err().code, ErrorCode::InvalidArgument);
}

#[test]
fn asking_about_a_model_the_vault_does_not_have_says_so() {
    let w = TestWorld::new();
    let absent = "A".repeat(64);
    assert!(vault(&w).file(&absent).unwrap().is_none());
    assert_eq!(
        vault(&w).set_canonical_name(&absent, "x").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(vault(&w).remove_alias(&absent, "x").unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(vault(&w).delete_file(&absent, &absent).unwrap_err().code, ErrorCode::NotFound);
}

// --- the Library's listing -------------------------------------------------

fn contents(w: &TestWorld, filter: VaultFilter, sort: VaultSort) -> ContentPage {
    vault(w).list_contents(0, 100, &filter, sort, false).unwrap()
}

#[test]
fn content_still_out_in_the_installs_is_one_row_counted_once() {
    // The Library counts a model once, whether it sits in one install or four.
    let w = TestWorld::new();
    let installs: Vec<_> = (0..3).map(|n| w.add_install(&format!("I{n}"))).collect();
    for i in &installs {
        w.write_model(i, "models/loras/m.safetensors", &weights("m"));
    }
    w.scan(&installs);

    let page = contents(&w, VaultFilter::default(), VaultSort::Size);
    assert_eq!(page.total, 1, "three copies of one content is one row");

    let row = &page.rows[0];
    assert_eq!(row.sha256, weights_hash("m"));
    assert_eq!(row.occurrence_count, 3, "and it says where all three are");
    assert_eq!(row.link_count, 0);
    assert!(!row.in_vault);
    assert_eq!(row.install_ids.len(), 3);
    assert_eq!(row.name, "m.safetensors");
    assert_eq!(row.category, "loras");
    assert!(page.scan_id.is_some());
}

#[test]
fn consolidated_content_keeps_its_single_row_with_the_links_counted() {
    let w = TestWorld::new();
    two_names(&w);
    w.scan(&w.store.installs().unwrap());

    let page = contents(&w, VaultFilter::default(), VaultSort::Size);
    assert_eq!(page.total, 1, "consolidating must not split the row in two");

    let row = &page.rows[0];
    assert!(row.in_vault);
    assert_eq!(row.link_count, 2);
    assert_eq!(row.occurrence_count, 2, "the two links are where it reaches");
    assert_eq!(row.name, "lora1.safetensors");
    assert_eq!(row.aliases, vec!["my-favourite.safetensors"]);
}

#[test]
fn a_content_half_consolidated_is_still_one_row_with_both_sides_added() {
    // A partial apply, or a copy that arrived after one. The Library must not
    // show the same model twice.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a.clone(), b.clone()]);
    Applier::new(&w.store, &w.platform)
        .apply("ap-1", &plan, &ApplyRequest {
            plan_id: plan.plan_id.clone(),
            group_ids: plan.groups.iter().map(|g| g.group_id.clone()).collect(),
            verify: VerifyModeArg::SizeAndMtime,
            stop_on_error: false,
        }, &CancelToken::new(), &NullSink)
        .unwrap();

    // A third install turns up later holding a real copy of the same content.
    let c = w.add_install("C");
    w.write_model(&c, "models/loras/m.safetensors", &weights("m"));
    w.scan(&[a, b, c]);

    let page = contents(&w, VaultFilter::default(), VaultSort::Size);
    assert_eq!(page.total, 1, "one content is one row, however it is spread");

    let row = &page.rows[0];
    assert!(row.in_vault);
    assert_eq!(row.link_count, 2, "two places are links");
    assert_eq!(row.occurrence_count, 3, "and a third still holds the real file");
    assert_eq!(row.install_ids.len(), 3);
}

#[test]
fn the_listing_filters_by_whether_the_vault_holds_it() {
    let w = TestWorld::new();
    two_names(&w);
    let late = w.add_install("Late");
    w.write_model(&late, "models/loras/new.safetensors", &weights("new"));
    w.scan(&w.store.installs().unwrap());

    let all = contents(&w, VaultFilter::default(), VaultSort::Size);
    assert_eq!(all.total, 2);

    let vaulted = contents(
        &w,
        VaultFilter { in_vault: Some(true), ..Default::default() },
        VaultSort::Size,
    );
    assert_eq!(vaulted.total, 1);
    assert!(vaulted.rows[0].in_vault);

    let not_yet = contents(
        &w,
        VaultFilter { in_vault: Some(false), ..Default::default() },
        VaultSort::Size,
    );
    assert_eq!(not_yet.total, 1);
    assert_eq!(not_yet.rows[0].name, "new.safetensors");
}

#[test]
fn the_listing_filters_by_category_and_by_name() {
    let w = TestWorld::new();
    let i = w.add_install("A");
    w.write_model(&i, "models/loras/alpha.safetensors", &weights("alpha"));
    w.write_model(&i, "models/loras/beta.safetensors", &weights("beta"));
    w.write_model(&i, "models/checkpoints/gamma.safetensors", &weights("gamma"));
    w.scan(&[i]);

    let loras = contents(
        &w,
        VaultFilter { category: Some("loras".into()), ..Default::default() },
        VaultSort::Name,
    );
    assert_eq!(loras.total, 2);

    let named = contents(
        &w,
        VaultFilter { name_contains: Some("GAMMA".into()), ..Default::default() },
        VaultSort::Name,
    );
    assert_eq!(named.total, 1, "the search must ignore case");
    assert_eq!(named.rows[0].name, "gamma.safetensors");
}

#[test]
fn unused_means_in_the_vault_with_nothing_reaching_it() {
    // A model still sitting in an install is used by definition: it is there.
    // Only a vault file nothing links to is unused.
    let w = TestWorld::new();
    let (a, b) = two_names(&w);
    let other = w.add_install("Other");
    w.write_model(&other, "models/loras/untouched.safetensors", &weights("untouched"));
    w.scan(&w.store.installs().unwrap());

    assert_eq!(
        contents(&w, VaultFilter { orphans_only: true, ..Default::default() }, VaultSort::Size).total,
        0,
        "nothing is unused while both installs still link to it"
    );

    for install in [&a, &b] {
        for e in std::fs::read_dir(install.root.join("models/loras")).unwrap().flatten() {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    w.scan(&w.store.installs().unwrap());

    let orphans = contents(&w, VaultFilter { orphans_only: true, ..Default::default() }, VaultSort::Size);
    assert_eq!(orphans.total, 1);
    assert_eq!(orphans.rows[0].occurrence_count, 0);
    assert!(orphans.rows[0].in_vault);
}

#[test]
fn the_listing_sorts_and_pages() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    // One content in two places, three more in one each.
    w.write_model(&a, "models/loras/shared.safetensors", &weights("shared"));
    w.write_model(&b, "models/loras/shared.safetensors", &weights("shared"));
    for n in 0..3 {
        w.write_model(&a, &format!("models/loras/x{n}.safetensors"), &weights(&format!("x{n}")));
    }
    w.scan(&[a, b]);

    let by_occurrences =
        vault(&w).list_contents(0, 100, &VaultFilter::default(), VaultSort::Occurrences, true).unwrap();
    assert_eq!(by_occurrences.total, 4);
    assert_eq!(
        by_occurrences.rows[0].occurrence_count, 2,
        "the most widespread content belongs at the top"
    );

    let page = vault(&w).list_contents(2, 2, &VaultFilter::default(), VaultSort::Name, false).unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.total, 4, "the total counts everything, not just the page");
    assert_eq!(page.offset, 2);
}

#[test]
fn the_listing_works_before_anything_has_been_scanned() {
    let w = TestWorld::new();
    w.add_install("A");
    let page = contents(&w, VaultFilter::default(), VaultSort::Size);
    assert_eq!(page.total, 0);
    assert!(page.scan_id.is_none(), "and it says why there is nothing out there");
}

#[test]
fn a_listing_never_returns_more_rows_than_the_page_limit() {
    let w = TestWorld::new();
    let page = vault(&w)
        .list_contents(0, 999_999, &VaultFilter::default(), VaultSort::Name, false)
        .unwrap();
    assert!(page.rows.len() <= MAX_PAGE);
}
