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
