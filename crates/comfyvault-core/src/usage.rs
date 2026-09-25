//! Is a model used by any saved workflow?
//!
//! # This check is deliberately shallow, and says so
//!
//! It looks for the file name as plain text inside the saved workflow files.
//! It does not read the graph, it does not resolve node inputs, and it does not
//! know which node a name belongs to. A match means the name appears somewhere
//! in the file. It does not prove the model is loaded when the workflow runs.
//!
//! Every answer carries [`METHOD`], and the contract requires the interface to
//! show it beside the result. A person must never read "not used" as "safe to
//! delete" without being told exactly what was checked.
//!
//! # What it cannot see
//!
//! A workflow the person never saved lives in their browser, not on the disk.
//! The engine cannot see it, and the interface has to say so.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::install::Install;

/// The sentence every result carries, and the interface must show.
pub const METHOD: &str =
    "The file name was searched for as plain text inside saved workflow files.";

/// The sentence used when there was nothing at all to search.
///
/// The contract's result shape is a flat list, so how much was searched has
/// nowhere else to travel. Without this, a person whose workflows only ever
/// lived in the browser would read "not used" for every model they own and
/// believe the app had checked.
pub const METHOD_NOTHING_SEARCHED: &str =
    "No saved workflow files were found, so nothing was searched. A workflow that was never saved lives in the browser, where this app cannot see it.";

/// Workflow files larger than this are skipped and reported.
pub const MAX_WORKFLOW_BYTES: u64 = 50 * 1024 * 1024;

/// Folders the search never walks into.
const SKIP_DIRS: [&str; 9] = [
    "models",
    "custom_nodes",
    "output",
    "input",
    "temp",
    ".git",
    "venv",
    ".venv",
    "python_embeded",
];

/// Where a name was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMatch {
    pub install_id: String,
    pub install_label: String,
    pub workflow_path: String,
    pub workflow_name: String,
}

/// The answer for one name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageResult {
    pub name: String,
    pub used: bool,
    pub matches: Vec<UsageMatch>,
    /// [`METHOD`], or [`METHOD_NOTHING_SEARCHED`] when there were no saved
    /// workflow files to search. The interface shows it beside the result.
    pub method: String,
    /// Were any saved workflow files searched at all?
    ///
    /// `false` and `used: false` are different answers: nothing was checked,
    /// rather than checked and not found. The interface decides very different
    /// things from the two, so it is a fact here rather than something read
    /// back out of the sentence.
    pub searched: bool,
}

/// What the search covered, so the interface can be honest about the gaps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub results: Vec<UsageResult>,
    pub workflows_searched: u64,
    pub workflows_skipped: Vec<String>,
    pub method: String,
}

/// Finds the saved workflow files in one install.
///
/// Current ComfyUI stores them under `user/<user id>/workflows/`. Every child
/// of `user/` is treated as a person's folder, because a multi-user install
/// names them after the people. Any file called `workflow.json` elsewhere is
/// picked up too, which covers older layouts and files people keep by hand.
pub fn workflow_files(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    let user_dir = root.join("user");
    if user_dir.is_dir() {
        for e in walkdir::WalkDir::new(&user_dir)
            .follow_links(true)
            .max_depth(8)
            .into_iter()
            .flatten()
        {
            if !e.file_type().is_file() {
                continue;
            }
            let p = e.path();
            if p.extension().map(|x| x.eq_ignore_ascii_case("json")).unwrap_or(false) {
                out.push(p.to_path_buf());
            }
        }
    }

    for e in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            e.depth() == 0 || (!SKIP_DIRS.contains(&name.as_str()) && name != "user")
        })
        .flatten()
    {
        if e.file_type().is_file() && e.file_name().eq_ignore_ascii_case("workflow.json") {
            out.push(e.path().to_path_buf());
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Searches the saved workflows of several installs for several file names.
pub fn check(installs: &[Install], names: &[String]) -> Result<UsageReport> {
    let wanted: Vec<(String, String)> = names
        .iter()
        .filter(|n| !n.trim().is_empty())
        .map(|n| (n.clone(), n.to_lowercase()))
        .collect();

    let mut results: Vec<UsageResult> = wanted
        .iter()
        .map(|(name, _)| UsageResult {
            name: name.clone(),
            used: false,
            matches: Vec::new(),
            method: METHOD.to_string(),
            searched: false,
        })
        .collect();

    let mut searched = 0u64;
    let mut skipped: Vec<String> = Vec::new();

    for install in installs {
        for path in workflow_files(&install.root) {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > MAX_WORKFLOW_BYTES {
                skipped.push(crate::paths::display_path(&path));
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                skipped.push(crate::paths::display_path(&path));
                continue;
            };
            searched += 1;
            let haystack = text.to_lowercase();

            for (i, (_, needle)) in wanted.iter().enumerate() {
                if !haystack.contains(needle) {
                    continue;
                }
                results[i].used = true;
                results[i].matches.push(UsageMatch {
                    install_id: install.id.clone(),
                    install_label: install.label.clone(),
                    workflow_path: crate::paths::display_path(&path),
                    workflow_name: path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                });
            }
        }
    }

    skipped.sort();
    skipped.dedup();

    // How much was searched reaches the person only through this sentence,
    // because the contract's result shape is a flat list with nowhere else to
    // put it. "Nothing was searched" and "nothing was found" are not the same
    // answer, and the difference decides whether a model is safe to remove.
    let method = if searched == 0 { METHOD_NOTHING_SEARCHED } else { METHOD };
    for r in &mut results {
        r.method = method.to_string();
        r.searched = searched > 0;
    }

    Ok(UsageReport {
        results,
        workflows_searched: searched,
        workflows_skipped: skipped,
        method: method.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TestWorld;

    fn write_workflow(root: &Path, rel: &str, body: &str) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    /// A workflow shaped like the ones ComfyUI saves.
    fn workflow_naming(model: &str) -> String {
        format!(
            r#"{{"last_node_id":3,"nodes":[
                {{"id":1,"type":"CheckpointLoaderSimple","widgets_values":["{model}"]}},
                {{"id":2,"type":"CLIPTextEncode","widgets_values":["a photo"]}}
            ],"links":[]}}"#
        )
    }

    #[test]
    fn a_model_named_in_a_saved_workflow_is_found() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/workflows/portrait.json",
            &workflow_naming("lora1.safetensors"),
        );

        let report = check(&[i.clone()], &["lora1.safetensors".into()]).unwrap();
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].used);
        assert_eq!(report.results[0].matches.len(), 1);
        assert_eq!(report.results[0].matches[0].workflow_name, "portrait");
        assert_eq!(report.results[0].matches[0].install_id, i.id);
        assert_eq!(report.workflows_searched, 1);
    }

    #[test]
    fn a_model_no_workflow_names_is_reported_as_not_found() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/workflows/portrait.json",
            &workflow_naming("something-else.safetensors"),
        );

        let report = check(&[i], &["lora1.safetensors".into()]).unwrap();
        assert!(!report.results[0].used);
        assert!(report.results[0].matches.is_empty());
    }

    #[test]
    fn every_answer_carries_the_sentence_that_says_what_was_checked() {
        // The interface is required to show this. Without it, "not used" reads
        // as "safe to delete", which this check does not prove.
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(&i.root, "user/default/workflows/w.json", &workflow_naming("x.safetensors"));

        let report = check(&[i], &["anything.safetensors".into()]).unwrap();
        assert_eq!(report.method, METHOD);
        assert_eq!(report.results[0].method, METHOD);
        assert!(METHOD.contains("plain text"), "the sentence must not overstate the check");
    }

    #[test]
    fn a_name_inside_a_folder_path_is_still_found() {
        // ComfyUI records a nested model as `awesomeloras\lora1.safetensors`,
        // and JSON doubles the backslash. Searching for the plain file name
        // still has to match.
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/workflows/nested.json",
            &workflow_naming("awesomeloras\\\\lora1.safetensors"),
        );

        let report = check(&[i], &["lora1.safetensors".into()]).unwrap();
        assert!(report.results[0].used);
    }

    #[test]
    fn the_search_ignores_case() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/workflows/w.json",
            &workflow_naming("LORA1.SafeTensors"),
        );

        let report = check(&[i], &["lora1.safetensors".into()]).unwrap();
        assert!(report.results[0].used, "Windows names are not case sensitive");
    }

    #[test]
    fn workflows_are_found_in_every_person_s_folder_not_only_default() {
        // A multi-user install names the folders after the people.
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(&i.root, "user/default/workflows/a.json", &workflow_naming("m.safetensors"));
        write_workflow(
            &i.root,
            "user/sam-1234/workflows/b.json",
            &workflow_naming("m.safetensors"),
        );

        let report = check(&[i], &["m.safetensors".into()]).unwrap();
        assert_eq!(report.results[0].matches.len(), 2);
    }

    #[test]
    fn a_subgraph_file_is_searched_too() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/subgraphs/reusable.json",
            &workflow_naming("m.safetensors"),
        );
        let report = check(&[i], &["m.safetensors".into()]).unwrap();
        assert!(report.results[0].used);
    }

    #[test]
    fn a_file_called_workflow_json_elsewhere_is_searched() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(&i.root, "my-stuff/workflow.json", &workflow_naming("m.safetensors"));
        let report = check(&[i], &["m.safetensors".into()]).unwrap();
        assert!(report.results[0].used);
    }

    #[test]
    fn the_search_never_walks_the_models_folder() {
        // That is the terabyte. Walking it to answer a question about a file
        // name would freeze the window.
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(&i.root, "models/loras/workflow.json", &workflow_naming("m.safetensors"));
        write_workflow(&i.root, "custom_nodes/Pack/workflow.json", &workflow_naming("m.safetensors"));

        let found = workflow_files(&i.root);
        assert!(found.is_empty(), "found {found:?}");
    }

    #[test]
    fn a_workflow_file_that_is_too_large_is_skipped_and_reported() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let p = write_workflow(&i.root, "user/default/workflows/huge.json", "{}");
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_len(MAX_WORKFLOW_BYTES + 1).unwrap();
        drop(f);

        let report = check(&[i], &["m.safetensors".into()]).unwrap();
        assert_eq!(report.workflows_searched, 0);
        assert_eq!(report.workflows_skipped.len(), 1);
        assert!(report.workflows_skipped[0].contains("huge.json"));
    }

    #[test]
    fn several_names_are_answered_in_one_pass() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(
            &i.root,
            "user/default/workflows/w.json",
            &workflow_naming("used.safetensors"),
        );

        let report = check(
            &[i],
            &["used.safetensors".into(), "unused.safetensors".into()],
        )
        .unwrap();
        assert_eq!(report.results.len(), 2);
        assert!(report.results[0].used);
        assert!(!report.results[1].used);
        assert_eq!(report.workflows_searched, 1, "one pass over the file, not two");
    }

    #[test]
    fn one_name_found_in_two_installs_reports_both() {
        let w = TestWorld::new();
        let a = w.add_install("A");
        let b = w.add_install("B");
        for i in [&a, &b] {
            write_workflow(&i.root, "user/default/workflows/w.json", &workflow_naming("m.safetensors"));
        }

        let report = check(&[a.clone(), b.clone()], &["m.safetensors".into()]).unwrap();
        assert_eq!(report.results[0].matches.len(), 2);
        let ids: Vec<&str> = report.results[0].matches.iter().map(|m| m.install_id.as_str()).collect();
        assert!(ids.contains(&a.id.as_str()) && ids.contains(&b.id.as_str()));
    }

    #[test]
    fn an_install_with_no_saved_workflows_says_nothing_was_searched() {
        // A person who has never pressed Save has nothing on the disk.
        // Answering a confident "not used" there would be a lie, so the
        // sentence the interface shows says that nothing was searched at all.
        let w = TestWorld::new();
        let i = w.add_install("A");
        let report = check(&[i], &["m.safetensors".into()]).unwrap();

        assert!(!report.results[0].used);
        assert_eq!(report.workflows_searched, 0);
        assert_eq!(report.results[0].method, METHOD_NOTHING_SEARCHED);
        assert_eq!(report.method, METHOD_NOTHING_SEARCHED);
        assert!(!report.results[0].searched, "the fact, not just the sentence");
        assert!(report.results[0].method.contains("nothing was searched"));
        assert!(
            report.results[0].method.contains("browser"),
            "the person has to be told where their unsaved workflows live"
        );
    }

    #[test]
    fn the_two_sentences_are_never_mixed_up() {
        // "Nothing was searched" and "nothing was found" are different answers,
        // and only one of them means a model might be safe to remove.
        let w = TestWorld::new();
        let empty = w.add_install("Empty");
        let searched = w.add_install("Searched");
        write_workflow(
            &searched.root,
            "user/default/workflows/w.json",
            &workflow_naming("other.safetensors"),
        );

        let nothing = check(&[empty], &["m.safetensors".into()]).unwrap();
        assert_eq!(nothing.results[0].method, METHOD_NOTHING_SEARCHED);

        let did_search = check(&[searched], &["m.safetensors".into()]).unwrap();
        assert_eq!(did_search.workflows_searched, 1);
        assert!(did_search.results[0].searched);
        assert_eq!(
            did_search.results[0].method, METHOD,
            "a real search must not claim that nothing was searched"
        );
        assert!(!did_search.results[0].used, "and it still found nothing, which is different");
    }

    #[test]
    fn a_file_that_is_not_readable_text_is_skipped_rather_than_failing() {
        let w = TestWorld::new();
        let i = w.add_install("A");
        let p = i.root.join("user/default/workflows/binary.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, [0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let report = check(&[i], &["m.safetensors".into()]).unwrap();
        assert_eq!(report.workflows_skipped.len(), 1);
        assert!(!report.results[0].used);
    }

    #[test]
    fn an_empty_name_is_ignored_rather_than_matching_everything() {
        // An empty needle is contained in every string, so it would report
        // every workflow as using it.
        let w = TestWorld::new();
        let i = w.add_install("A");
        write_workflow(&i.root, "user/default/workflows/w.json", &workflow_naming("m.safetensors"));

        let report = check(&[i], &["".into(), "   ".into()]).unwrap();
        assert!(report.results.is_empty());
    }
}
