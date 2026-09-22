//! Plan tests. Every one builds its own tree in a temporary folder.

use super::*;
use crate::testkit::{weights, weights_hash, TestWorld};

fn group_for<'a>(plan: &'a ConsolidationPlan, tag: &str) -> &'a PlanGroup {
    let sha = weights_hash(tag);
    plan.groups
        .iter()
        .find(|g| g.sha256 == sha)
        .unwrap_or_else(|| panic!("no group for {tag}; groups: {:?}", plan.groups.iter().map(|g| &g.vault_rel_path).collect::<Vec<_>>()))
}

fn blocked_for<'a>(plan: &'a ConsolidationPlan, name: &str) -> &'a BlockedRow {
    plan.blocked
        .iter()
        .find(|b| b.abs_path.file_name().unwrap().to_string_lossy() == name)
        .unwrap_or_else(|| panic!("nothing blocked called {name}; blocked: {:?}", plan.blocked.iter().map(|b| (&b.abs_path, b.reason)).collect::<Vec<_>>()))
}

fn name_of(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().to_string()
}

#[test]
fn a_plan_changes_nothing_on_disk() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let pa = w.write_model(&a, "models/loras/lora1.safetensors", &weights("lora1"));
    let pb = w.write_model(&b, "models/loras/lora1.safetensors", &weights("lora1"));

    let plan = w.plan(&[a, b]);
    assert_eq!(plan.groups.len(), 1);

    assert!(!w.is_link(&pa), "a plan must not create a link");
    assert!(!w.is_link(&pb));
    assert_eq!(w.read(&pa), weights("lora1"));
    assert_eq!(w.read(&pb), weights("lora1"));
    assert!(!w.vault_root.join("loras").exists(), "a plan must not create a vault folder");
}

#[test]
fn two_copies_become_one_vault_file_and_one_link() {
    // The person's own example: lora1 sits in loras\awesomeloras\ in one install
    // and in loras\newloras\ in the other, and it ends up clean in vault\loras\.
    let w = TestWorld::new();
    let a = w.add_install("Production");
    let b = w.add_install("Normal");
    w.write_model(&a, "models/loras/awesomeloras/lora1.safetensors", &weights("lora1"));
    w.write_model(&b, "models/loras/newloras/lora1.safetensors", &weights("lora1"));

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "lora1");

    assert_eq!(g.occurrences, 2);
    assert_eq!(
        g.links.len(), 2,
        "a link takes the place of every file that was there, the moved one included"
    );
    assert_eq!(g.links.iter().filter(|l| l.is_source).count(), 1);
    assert!(!g.single_copy);
    assert_eq!(g.bytes_freed, weights("lora1").len() as u64);
    assert_eq!(
        g.vault_rel_path,
        PathBuf::from("loras").join("lora1.safetensors"),
        "the sub-folder disappears inside the vault"
    );
    assert!(!g.vault_name_adjusted);
}

#[test]
fn a_single_copy_still_moves_and_frees_nothing() {
    // The person will read the row count and the space number together, so the
    // two must not contradict each other.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/only.safetensors", &weights("only"));

    let plan = w.plan(&[a]);
    let g = group_for(&plan, "only");
    assert!(g.single_copy);
    assert_eq!(g.bytes_freed, 0);
    assert_eq!(g.occurrences, 1);
    assert_eq!(g.links.len(), 1, "the one copy still gets a link where it was");
    assert!(g.links[0].is_source);
    assert_eq!(g.source.chosen_because, SourceChoice::OnlyCopy);

    assert_eq!(plan.totals.single_copy_groups, 1);
    assert_eq!(plan.totals.groups_freeing_space, 0);
    assert_eq!(plan.totals.bytes_freed, 0);
    assert_eq!(plan.totals.files_moved, 1);
}

#[test]
fn a_link_keeps_the_name_it_had_even_when_the_vault_name_differs() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/my-favourite.safetensors", &weights("same"));

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "same");
    assert_eq!(g.links.len(), 2);
    for link in &g.links {
        assert_eq!(
            link.link_name,
            name_of(&link.abs_path),
            "a link always keeps the name the file had in that install"
        );
    }
    let renamed = g.links.iter().find(|l| l.link_name == "my-favourite.safetensors").unwrap();
    assert!(renamed.name_differs_from_vault);
}

#[test]
fn a_second_name_for_one_content_becomes_a_name_inside_the_vault() {
    // This is what the cleanup screen later works on. Without it the vault
    // would silently forget a name a saved workflow may refer to.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/my-favourite.safetensors", &weights("same"));

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "same");
    assert_eq!(g.vault_aliases, vec!["my-favourite.safetensors"]);
}

#[test]
fn one_name_appearing_twice_produces_no_duplicate_alias() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("same"));
    w.write_model(&b, "models/loras/lora1.safetensors", &weights("same"));

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "same");
    assert!(g.vault_aliases.is_empty(), "the one name is already the vault name");
}

#[test]
fn two_different_contents_with_one_name_both_survive() {
    // The name clash rule: the first keeps the plain name, the second takes an
    // adjusted one. Neither is lost and neither overwrites the other.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("version-one"));
    w.write_model(&b, "models/loras/lora1.safetensors", &weights("version-two"));

    let plan = w.plan(&[a, b]);
    assert_eq!(plan.groups.len(), 2, "different content is never merged");
    assert_eq!(plan.totals.name_clashes, 1);

    let names: Vec<String> = plan.groups.iter().map(|g| name_of(&g.vault_rel_path)).collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1], "both contents need their own vault name");
    assert!(names.contains(&"lora1.safetensors".to_string()));

    let adjusted = plan.groups.iter().find(|g| g.vault_name_adjusted).unwrap();
    let plain = plan.groups.iter().find(|g| !g.vault_name_adjusted).unwrap();
    let adjusted_name = name_of(&adjusted.vault_rel_path);
    assert!(adjusted_name.starts_with("lora1__"), "got {adjusted_name}");
    assert!(adjusted_name.ends_with(".safetensors"), "the extension must survive");
    assert_eq!(adjusted.clashes_with.as_deref(), Some(plain.sha256.as_str()));
}

#[test]
fn the_content_with_more_copies_keeps_the_plain_name() {
    // Two different files share one name. Handing the plain name to whichever
    // hash happened to sort first is deterministic but impossible to explain.
    // The widely used one keeps the name people recognize.
    let w = TestWorld::new();
    let installs: Vec<_> = (0..5).map(|n| w.add_install(&format!("I{n}"))).collect();
    for i in &installs[0..2] {
        w.write_model(i, "models/loras/lora1.safetensors", &weights("rare"));
    }
    for i in &installs[2..5] {
        w.write_model(i, "models/loras/lora1.safetensors", &weights("common"));
    }

    let plan = w.plan(&installs);
    let common = group_for(&plan, "common");
    let rare = group_for(&plan, "rare");

    assert!(!common.vault_name_adjusted, "three copies must keep the plain name");
    assert_eq!(name_of(&common.vault_rel_path), "lora1.safetensors");
    assert!(rare.vault_name_adjusted, "two copies must take the adjusted name");
    assert_eq!(rare.clashes_with.as_deref(), Some(common.sha256.as_str()));
}

#[test]
fn a_clash_still_lets_each_install_keep_its_own_link_name() {
    // The adjusted group's installs keep the name they always had, so a saved
    // workflow that names lora1.safetensors keeps working in both of them.
    let w = TestWorld::new();
    let installs: Vec<_> = (0..5).map(|n| w.add_install(&format!("I{n}"))).collect();
    for i in &installs[0..2] {
        w.write_model(i, "models/loras/lora1.safetensors", &weights("rare"));
    }
    for i in &installs[2..5] {
        w.write_model(i, "models/loras/lora1.safetensors", &weights("common"));
    }

    let plan = w.plan(&installs);
    let rare = group_for(&plan, "rare");
    assert!(rare.vault_name_adjusted);
    assert_eq!(rare.links.len(), 2, "both copies of the rarer content get a link");
    assert_eq!(
        rare.links[0].link_name, "lora1.safetensors",
        "the install keeps the name it always had, whatever the vault calls the file"
    );
    assert!(rare.links[0].name_differs_from_vault);
    assert_eq!(
        name_of(&rare.source.abs_path),
        "lora1.safetensors",
        "the copy that becomes the vault file was also called lora1 where it lived"
    );
}

#[test]
fn the_same_name_in_two_categories_is_not_a_clash() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/model.safetensors", &weights("as-lora"));
    w.write_model(&a, "models/checkpoints/model.safetensors", &weights("as-ckpt"));

    let plan = w.plan(&[a]);
    assert_eq!(plan.totals.name_clashes, 0, "different folders are different names");
    assert_eq!(group_for(&plan, "as-lora").vault_rel_path, PathBuf::from("loras/model.safetensors"));
    assert_eq!(
        group_for(&plan, "as-ckpt").vault_rel_path,
        PathBuf::from("checkpoints/model.safetensors")
    );
}

#[test]
fn a_name_already_held_by_a_vault_file_forces_an_adjusted_name() {
    // A second consolidation must not collide with what the first one stored.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.store
        .put_vault_file(&crate::store::VaultFileRecord {
            sha256: "0".repeat(64),
            canonical_name: "lora1.safetensors".into(),
            category: "loras".into(),
            size_bytes: 1,
            added_at: crate::time_util::Timestamp::now(),
            aliases: vec![],
        })
        .unwrap();
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("new-content"));

    let plan = w.plan(&[a]);
    let g = group_for(&plan, "new-content");
    assert!(g.vault_name_adjusted);
    assert_eq!(g.clashes_with.as_deref(), Some("0".repeat(64).as_str()));
}

#[test]
fn a_locked_file_is_blocked_and_never_planned() {
    // Windows refuses to move a file another program holds open. The engine
    // reports it instead of trying and failing halfway.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/held.safetensors", &weights("held"));
    w.platform.lock_file(p.clone());

    let plan = w.plan(&[a]);
    assert!(plan.groups.is_empty(), "a held file must not be planned");
    let row = blocked_for(&plan, "held.safetensors");
    assert_eq!(row.reason, BlockReason::FileLocked);
    assert!(row.detail.contains("Close ComfyUI"));
}

#[test]
fn a_file_changed_after_the_scan_is_blocked() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("before"));

    let out = w.scan(&[a.clone()]);
    // The person carries on with their day between the scan and the plan.
    std::fs::write(&p, weights("after")).unwrap();
    let f = std::fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    drop(f);

    let plan = w
        .planner()
        .build("p", &out.record.scan_id, &out.entries, &[a])
        .unwrap();
    assert!(plan.groups.is_empty());
    assert_eq!(blocked_for(&plan, "m.safetensors").reason, BlockReason::FileChanged);
}

#[test]
fn a_file_deleted_after_the_scan_is_blocked() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let p = w.write_model(&a, "models/loras/m.safetensors", &weights("m"));

    let out = w.scan(&[a.clone()]);
    std::fs::remove_file(&p).unwrap();

    let plan = w.planner().build("p", &out.record.scan_id, &out.entries, &[a]).unwrap();
    assert_eq!(blocked_for(&plan, "m.safetensors").reason, BlockReason::FileMissing);
}

#[test]
fn without_symlink_support_the_plan_is_still_readable() {
    // The person reads what they would gain, and that is what sends them to
    // turn Developer Mode on. A blank screen tells them nothing, in exactly the
    // state where it matters most.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.write_model(&b, "models/loras/m.safetensors", &weights("m"));
    w.platform.set_symlinks_unsupported(true);

    let plan = w.plan(&[a, b]);

    assert_eq!(plan.groups.len(), 1, "the plan must still say what it would do");
    assert_eq!(plan.totals.bytes_freed, weights("m").len() as u64);
    assert!(!plan.symlinks_supported);

    // One row says the computer cannot make links. Not one per group: the
    // reason is a fact about the computer, not about any file.
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].reason, BlockReason::SymlinkUnsupported);
    assert_eq!(plan.blocked[0].abs_path, w.vault_root);
}

#[test]
fn a_computer_that_can_make_links_gets_no_platform_row() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));

    let plan = w.plan(&[a]);
    assert!(plan.symlinks_supported);
    assert!(plan.blocked.iter().all(|b| b.reason != BlockReason::SymlinkUnsupported));
}

#[test]
fn an_empty_plan_on_a_computer_without_links_reports_nothing_to_block() {
    // Nothing to consolidate means nothing is blocked, whatever the computer
    // can or cannot do.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.platform.set_symlinks_unsupported(true);

    let plan = w.plan(&[a]);
    assert!(plan.groups.is_empty());
    assert!(plan.blocked.is_empty());
}

#[test]
fn a_vault_inside_an_install_blocks_that_install() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    // A vault under the install would make the engine move files into
    // themselves and then scan what it just wrote.
    let inner_vault = a.root.join("ComfyVault");
    let store = crate::store::Store::open(&inner_vault, true).unwrap();
    let planner = Planner::new(&store, &w.platform);

    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let out = crate::scan::Scanner::new(&store, &w.platform, w.settings.clone())
        .scan("s", &[a.clone()], &crate::progress::CancelToken::new(), &crate::progress::NullSink)
        .unwrap();

    let plan = planner.build("p", "s", &out.entries, &[a]).unwrap();
    assert!(plan.groups.is_empty());
    assert_eq!(blocked_for(&plan, "m.safetensors").reason, BlockReason::VaultInsideInstall);
}

#[test]
fn counted_only_files_do_not_appear_as_blocked_rows() {
    // They are reported as bytes in the scan totals. Listing thousands of
    // Hugging Face cache files as "blocked" would bury the rows the person can
    // actually act on.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "custom_nodes/Pack/ckpts/bundled.safetensors", &weights("bundled"));
    w.write_model(&a, "models/loras/normal.safetensors", &weights("normal"));

    let plan = w.plan(&[a]);
    assert_eq!(plan.groups.len(), 1);
    assert!(
        plan.blocked.iter().all(|b| name_of(&b.abs_path) != "bundled.safetensors"),
        "counted files belong in the scan totals, not in the plan"
    );
}

#[test]
fn an_already_consolidated_link_is_left_out_of_the_plan_entirely() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let vault_file = w.vault_root.join("loras/already.safetensors");
    std::fs::create_dir_all(vault_file.parent().unwrap()).unwrap();
    std::fs::write(&vault_file, weights("already")).unwrap();

    let at = a.root.join("models/loras/already.safetensors");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&vault_file, &at).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&vault_file, &at).unwrap();

    let plan = w.plan(&[a]);
    assert!(plan.groups.is_empty());
    assert!(plan.blocked.is_empty(), "work already done is not a problem to report");
}

#[test]
fn a_copy_on_the_vault_drive_is_chosen_as_the_source() {
    // That move is a rename: instant, and it needs no second copy of the bytes.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let on_other_drive = w.write_model(&a, "models/loras/lora1.safetensors", &weights("shared"));
    let on_vault_drive = w.write_model(&b, "models/loras/lora1.safetensors", &weights("shared"));

    w.platform.set_volume(&a.root, "D:");
    w.platform.set_volume(&w.vault_root, "C:");
    w.platform.set_volume(&b.root, "C:");

    let plan = w.plan(&[a, b]);
    let g = group_for(&plan, "shared");
    assert_eq!(g.source.abs_path, on_vault_drive);
    assert_eq!(g.source.chosen_because, SourceChoice::SameVolume);
    assert!(g.source.same_volume_as_vault);
    assert!(!g.cross_volume);
    assert_eq!(g.links[0].abs_path, on_other_drive);
}

#[test]
fn a_group_entirely_on_another_drive_is_marked_cross_volume() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("far"));
    w.platform.set_volume(w.path(), "D:");
    w.platform.set_volume(&w.vault_root, "C:");

    let plan = w.plan(&[a]);
    let g = group_for(&plan, "far");
    assert!(g.cross_volume);
    assert!(!g.source.same_volume_as_vault);
    assert_eq!(w.store.vault_root(), w.vault_root);
}

#[test]
fn the_source_choice_does_not_depend_on_the_order_the_disk_returns() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/lora1.safetensors", &weights("shared"));
    w.write_model(&b, "models/loras/lora1.safetensors", &weights("shared"));

    let first = w.plan(&[a.clone(), b.clone()]);
    let second = w.plan(&[b, a]);
    assert_eq!(
        group_for(&first, "shared").source.abs_path,
        group_for(&second, "shared").source.abs_path,
        "the same scan must always produce the same plan"
    );
}

#[test]
fn a_vault_drive_without_room_blocks_the_largest_groups_first() {
    // The person keeps as many rows as their drive can actually take.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/small.safetensors", &weights("small"));
    w.write_model(&a, "models/loras/large.safetensors", &weights("large"));

    w.platform.set_volume(w.path(), "D:");
    w.platform.set_volume(&w.vault_root, "C:");
    // Room for one of the two, not both.
    w.platform.set_free_bytes(weights("small").len() as u64 + 10);

    let plan = w.plan(&[a]);
    assert_eq!(plan.groups.len(), 1, "one group must survive");
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].reason, BlockReason::NotEnoughSpace);
}

#[test]
fn plenty_of_room_blocks_nothing() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    w.platform.set_volume(w.path(), "D:");
    w.platform.set_volume(&w.vault_root, "C:");
    w.platform.set_free_bytes(u64::MAX / 2);

    let plan = w.plan(&[a]);
    assert_eq!(plan.groups.len(), 1);
    assert!(plan.blocked.is_empty());
}

#[test]
fn the_totals_agree_with_the_groups() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    let c = w.add_install("C");
    for i in [&a, &b, &c] {
        w.write_model(i, "models/loras/shared.safetensors", &weights("shared"));
    }
    w.write_model(&a, "models/loras/alone.safetensors", &weights("alone"));

    let plan = w.plan(&[a, b, c]);
    let size = weights("shared").len() as u64;

    assert_eq!(plan.totals.groups, 2);
    assert_eq!(plan.totals.groups_freeing_space, 1);
    assert_eq!(plan.totals.single_copy_groups, 1);
    assert_eq!(plan.totals.bytes_freed, size * 2);
    assert_eq!(plan.totals.files_moved, 2, "one file moves per group");
    assert_eq!(
        plan.totals.links_created, 4,
        "three copies of one content plus one lone file is four places that get a link"
    );

    let summed: u64 = plan.groups.iter().map(|g| g.bytes_freed).sum();
    assert_eq!(summed, plan.totals.bytes_freed);
}

#[test]
fn groups_are_ordered_with_the_biggest_saving_first() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/dup.safetensors", &weights("dup"));
    w.write_model(&b, "models/loras/dup.safetensors", &weights("dup"));
    w.write_model(&a, "models/loras/single.safetensors", &weights("single"));

    let plan = w.plan(&[a, b]);
    assert!(
        plan.groups[0].bytes_freed >= plan.groups[1].bytes_freed,
        "the row that saves the most belongs at the top"
    );
}

#[test]
fn selecting_groups_returns_them_and_refuses_an_unknown_one() {
    // Apply must never quietly do less than the caller asked for.
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    let id = plan.groups[0].group_id.clone();
    assert_eq!(plan.select(&[id.clone()]).unwrap().len(), 1);

    let err = plan.select(&[id, "g-nope".to_string()]).unwrap_err();
    assert_eq!(err.code, crate::ErrorCode::NotFound);
}

#[test]
fn an_empty_selection_selects_nothing_rather_than_everything() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);
    assert!(plan.select(&[]).unwrap().is_empty());
}

#[test]
fn a_plan_round_trips_through_the_store_and_through_json() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let plan = w.plan(&[a]);

    let back = w.store.plan(&plan.plan_id).unwrap().unwrap();
    assert_eq!(back.groups.len(), plan.groups.len());
    assert_eq!(back.groups[0].sha256, plan.groups[0].sha256);

    let v = serde_json::to_value(&plan).unwrap();
    assert!(v.get("planId").is_some(), "field names reach the UI as camelCase");
    assert!(v["groups"][0].get("vaultRelPath").is_some());
    assert!(v["totals"].get("bytesFreed").is_some());
}

#[test]
fn every_block_reason_has_a_sentence_a_person_can_read() {
    for r in [
        BlockReason::FileLocked,
        BlockReason::FileChanged,
        BlockReason::FileMissing,
        BlockReason::PermissionDenied,
        BlockReason::InCustomNodes,
        BlockReason::InHuggingFaceCache,
        BlockReason::AlreadyInVault,
        BlockReason::ExternalLink,
        BlockReason::SymlinkUnsupported,
        BlockReason::VaultInsideInstall,
        BlockReason::TargetExistsNotLink,
        BlockReason::NotEnoughSpace,
        BlockReason::ReadError,
    ] {
        let m = r.message();
        assert!(!m.is_empty(), "{r:?} has no message");
        assert!(m.ends_with('.'), "{r:?} is not a sentence: {m}");
        assert!(
            m.chars().next().unwrap().is_uppercase(),
            "{r:?} does not start like a sentence: {m}"
        );
    }
}

#[test]
fn an_adjusted_name_keeps_the_extension_and_is_stable() {
    let sha = "3F9A2C17".to_string() + &"0".repeat(56);
    assert_eq!(adjusted_name("lora1.safetensors", &sha), "lora1__3F9A2C17.safetensors");
    assert_eq!(adjusted_name("a.b.ckpt", &sha), "a.b__3F9A2C17.ckpt");
    assert_eq!(adjusted_name("noext", &sha), "noext__3F9A2C17");
    // A dotfile has no stem, so the whole name is kept and the tag appended.
    assert_eq!(adjusted_name(".hidden", &sha), ".hidden__3F9A2C17");
}

#[test]
fn every_copy_in_a_group_gets_a_link_and_exactly_one_of_them_is_the_source() {
    // The count the person reads is "N links go back where they were", so it
    // has to equal the number of places that held the file. Leaving the moved
    // copy out would report one short on every row.
    let w = TestWorld::new();
    let installs: Vec<_> = (0..4).map(|n| w.add_install(&format!("I{n}"))).collect();
    for i in &installs {
        w.write_model(i, "models/loras/m.safetensors", &weights("m"));
    }
    w.write_model(&installs[0], "models/loras/alone.safetensors", &weights("alone"));

    let plan = w.plan(&installs);
    for g in &plan.groups {
        assert_eq!(
            g.links.len() as u64,
            g.occurrences,
            "group {} has {} links for {} copies",
            g.vault_rel_path.display(),
            g.links.len(),
            g.occurrences
        );
        assert_eq!(
            g.links.iter().filter(|l| l.is_source).count(),
            1,
            "exactly one copy becomes the vault file"
        );
        let source = g.links.iter().find(|l| l.is_source).unwrap();
        assert_eq!(source.abs_path, g.source.abs_path, "and it is the one named in source");
    }

    let shared = group_for(&plan, "m");
    assert_eq!(shared.occurrences, 4);
    assert_eq!(shared.links.len(), 4);
    assert_eq!(
        shared.bytes_freed,
        weights("m").len() as u64 * 3,
        "four copies frees three copies' worth: the fourth moves"
    );
}

// ---------------------------------------------------------------------------
// Closing ComfyUI makes the plan BIGGER
//
// The interface is built on this: re-checking the machine has to re-derive the
// plan, not clear a flag on the old one. A file that was held open becomes
// movable, and a copy that was blocked can become the copy that is kept.
// ---------------------------------------------------------------------------

#[test]
fn a_file_that_stops_being_held_open_joins_the_plan() {
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/free.safetensors", &weights("free"));
    w.write_model(&b, "models/loras/free.safetensors", &weights("free"));
    w.write_model(&a, "models/loras/held.safetensors", &weights("held"));
    let held_b = w.write_model(&b, "models/loras/held.safetensors", &weights("held"));

    // ComfyUI is running and has one model loaded.
    w.platform.lock_file(held_b.clone());
    let out = w.scan(&[a.clone(), b.clone()]);

    let while_running = w
        .planner()
        .build("p1", &out.record.scan_id, &out.entries, &[a.clone(), b.clone()])
        .unwrap();

    // The held copy is dropped from its group, so what is left of that content
    // is a single copy that moves and frees nothing. The person is told why:
    // the held path is in `blocked`, named, with its reason.
    let held_now = group_for(&while_running, "held");
    assert_eq!(held_now.occurrences, 1);
    assert_eq!(held_now.bytes_freed, 0, "nothing is freed while the other copy is held");
    assert_eq!(blocked_for(&while_running, "held.safetensors").reason, BlockReason::FileLocked);
    let before = while_running.totals.bytes_freed;
    assert_eq!(before, weights("free").len() as u64, "only the free content saves anything");

    // The person closes ComfyUI and the interface re-derives the plan from the
    // same scan.
    w.platform.unlock_file(&held_b);
    let after_closing = w
        .planner()
        .build("p2", &out.record.scan_id, &out.entries, &[a, b])
        .unwrap();

    let held_after = group_for(&after_closing, "held");
    assert_eq!(held_after.occurrences, 2, "both copies are in the group now");
    assert_eq!(held_after.bytes_freed, weights("held").len() as u64);
    assert!(
        after_closing.totals.bytes_freed > before,
        "closing ComfyUI must return more space, got {} then {}",
        before,
        after_closing.totals.bytes_freed
    );
    assert!(
        after_closing.blocked.iter().all(|b| b.reason != BlockReason::FileLocked),
        "nothing is held open any more"
    );
}

#[test]
fn a_copy_that_was_held_open_can_become_the_copy_that_is_kept() {
    // The choice of which copy survives is re-derived too. The copy on the
    // vault's own drive is the one worth keeping, and if it was the one held
    // open, closing ComfyUI changes which file moves.
    let w = TestWorld::new();
    let on_other_drive = w.add_install("OnD");
    let on_vault_drive = w.add_install("OnC");
    let far = w.write_model(&on_other_drive, "models/loras/m.safetensors", &weights("m"));
    let near = w.write_model(&on_vault_drive, "models/loras/m.safetensors", &weights("m"));

    w.platform.set_volume(&on_other_drive.root, "D:");
    w.platform.set_volume(&w.vault_root, "C:");
    w.platform.set_volume(&on_vault_drive.root, "C:");

    // ComfyUI holds the copy that would otherwise be kept.
    w.platform.lock_file(near.clone());
    let out = w.scan(&[on_other_drive.clone(), on_vault_drive.clone()]);

    let while_running = w
        .planner()
        .build("p1", &out.record.scan_id, &out.entries, &[on_other_drive.clone(), on_vault_drive.clone()])
        .unwrap();

    // Only the far copy is left in the group, so the plan would copy across
    // drives and free nothing.
    let before = group_for(&while_running, "m");
    assert_eq!(before.source.abs_path, far);
    assert!(before.cross_volume, "the only copy left is on the other drive");
    assert_eq!(before.bytes_freed, 0);

    w.platform.unlock_file(&near);
    let after_closing = w
        .planner()
        .build("p2", &out.record.scan_id, &out.entries, &[on_other_drive, on_vault_drive])
        .unwrap();

    let g = group_for(&after_closing, "m");
    assert_eq!(
        g.source.abs_path, near,
        "the copy on the vault's drive is the one to keep, now that it is free"
    );
    assert_eq!(g.source.chosen_because, SourceChoice::SameVolume);
    assert!(!g.cross_volume, "so the move is a rename and costs nothing");
    assert_eq!(g.links.iter().find(|l| !l.is_source).unwrap().abs_path, far);
}

#[test]
fn the_plan_is_derived_fresh_every_time_and_never_cached() {
    // Two builds from one scan, with the machine changed in between, must not
    // agree. If they did, the interface could show a stale plan after the
    // person acted on what it told them to do.
    let w = TestWorld::new();
    let a = w.add_install("A");
    let b = w.add_install("B");
    w.write_model(&a, "models/loras/m.safetensors", &weights("m"));
    let held = w.write_model(&b, "models/loras/m.safetensors", &weights("m"));

    let out = w.scan(&[a.clone(), b.clone()]);

    w.platform.lock_file(held.clone());
    let locked = w.planner().build("p1", &out.record.scan_id, &out.entries, &[a.clone(), b.clone()]).unwrap();

    w.platform.unlock_file(&held);
    let free = w.planner().build("p2", &out.record.scan_id, &out.entries, &[a, b]).unwrap();

    assert_eq!(locked.groups[0].occurrences, 1, "one copy was held open");
    assert_eq!(free.groups[0].occurrences, 2);
    assert_eq!(locked.totals.bytes_freed, 0);
    assert_eq!(free.totals.bytes_freed, weights("m").len() as u64);
    assert_ne!(locked.totals.bytes_freed, free.totals.bytes_freed);
}
