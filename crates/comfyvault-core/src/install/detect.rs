//! Deciding whether a folder is a ComfyUI install, and where its models live.
//!
//! # How a root is recognized
//!
//! Seven entries have sat at the root of every ComfyUI install since March
//! 2023. The engine requires five of them, then opens `folder_paths.py` and
//! requires the text `folder_names_and_paths` to appear. The count tolerates a
//! trimmed distribution; the content check removes the false positives that a
//! count alone would let through, because nothing else on a disk contains that
//! name.
//!
//! # Why the search goes deeper
//!
//! A launcher does not put ComfyUI at the top. One real layout is
//! `C:\ComfyUI-Main\ComfyUI-Easy-Install\ComfyUI\`. So a folder that
//! fails the test is searched three levels down, the shallowest match becomes
//! the root, and any other match is reported so the person can choose.
//!
//! The search skips the folders that are large or irrelevant, above all
//! `models`, which is the terabyte this product exists to shrink.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::extra_paths::{self, ExtraPath};
use crate::error::{ErrorCode, Result, VaultError};

/// Entries present at the root of every ComfyUI install since March 2023.
pub const ROOT_MARKERS: [&str; 7] = [
    "main.py",
    "nodes.py",
    "folder_paths.py",
    "execution.py",
    "server.py",
    "comfy",
    "comfy_extras",
];

/// How many markers must be present.
pub const REQUIRED_MARKERS: usize = 5;

/// The text that must appear in `folder_paths.py`. Nothing else on a disk has
/// it, so it turns a plausible folder into a certain one.
const CONTENT_MARKER: &str = "folder_names_and_paths";

/// How far below the given folder to look for a real root.
pub const MAX_SEARCH_DEPTH: usize = 3;

/// Folders the search never descends into.
const SKIP_DIRS: [&str; 14] = [
    "models",
    "custom_nodes",
    "output",
    "input",
    "temp",
    "user",
    ".git",
    "node_modules",
    "__pycache__",
    "venv",
    ".venv",
    "python_embeded",
    "python_embedded",
    "site-packages",
];

/// The five folders under `output/` that ComfyUI registers as model search
/// paths at startup, and the category each one belongs to.
///
/// `main.py` adds these unconditionally, so weights saved by a workflow land in
/// a real model path. A scan that only looked at `models/` would miss them.
pub const OUTPUT_MODEL_DIRS: [(&str, &str); 5] = [
    ("checkpoints", "checkpoints"),
    ("clip", "text_encoders"),
    ("vae", "vae"),
    ("diffusion_models", "diffusion_models"),
    ("loras", "loras"),
];

/// Where a version string came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionSource {
    #[serde(rename = "comfyui_version.py")]
    VersionFile,
    #[serde(rename = "pyproject.toml")]
    PyProject,
}

/// A folder under `output/` that ComfyUI treats as a model path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputModelDir {
    pub path: PathBuf,
    pub category: String,
    pub exists: bool,
}

/// The answer to "is this a ComfyUI install, and what is in it".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCandidate {
    pub valid: bool,
    pub root: Option<PathBuf>,
    pub nested_depth: usize,
    pub markers_found: Vec<String>,
    pub markers_missing: Vec<String>,
    pub content_check_passed: bool,
    pub other_candidates: Vec<PathBuf>,
    pub version: Option<String>,
    pub version_source: Option<VersionSource>,
    pub models_dir: Option<PathBuf>,
    pub models_dir_exists: bool,
    pub extra_paths_file: Option<PathBuf>,
    pub extra_paths: Vec<ExtraPath>,
    /// Every complaint the extra paths file produced, one per entry.
    ///
    /// A list, not one joined sentence. A yaml file with three bad categories
    /// has three things to tell the person, and each names a different line
    /// to correct. Empty means the file parsed cleanly, or there was no file.
    pub extra_paths_problems: Vec<String>,
    pub output_model_dirs: Vec<OutputModelDir>,
    pub reason: Option<String>,
}

impl InstallCandidate {
    fn invalid(reason: impl Into<String>) -> Self {
        Self {
            valid: false,
            root: None,
            nested_depth: 0,
            markers_found: Vec::new(),
            markers_missing: ROOT_MARKERS.iter().map(|s| s.to_string()).collect(),
            content_check_passed: false,
            other_candidates: Vec::new(),
            version: None,
            version_source: None,
            models_dir: None,
            models_dir_exists: false,
            extra_paths_file: None,
            extra_paths: Vec::new(),
            extra_paths_problems: Vec::new(),
            output_model_dirs: Vec::new(),
            reason: Some(reason.into()),
        }
    }
}

/// Which of the seven markers this folder has.
pub fn markers_in(dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for m in ROOT_MARKERS {
        if std::fs::symlink_metadata(dir.join(m)).is_ok() {
            found.push(m.to_string());
        } else {
            missing.push(m.to_string());
        }
    }
    (found, missing)
}

/// Does `folder_paths.py` contain the name only ComfyUI uses?
///
/// Reads the first 64 KiB. The name appears in the first few hundred bytes of
/// every version, and a bounded read keeps a huge lookalike file from stalling
/// the folder picker.
pub fn content_check(dir: &Path) -> bool {
    use std::io::Read;
    let path = dir.join("folder_paths.py");
    let Ok(mut f) = std::fs::File::open(&path) else {
        return false;
    };
    let mut buf = vec![0u8; 64 * 1024];
    let Ok(n) = f.read(&mut buf) else { return false };
    buf.truncate(n);
    String::from_utf8_lossy(&buf).contains(CONTENT_MARKER)
}

/// Is this exact folder a ComfyUI root?
pub fn is_root(dir: &Path) -> bool {
    let (found, _) = markers_in(dir);
    found.len() >= REQUIRED_MARKERS && content_check(dir)
}

/// Finds every ComfyUI root at or below `start`, shallowest first.
pub fn find_roots(start: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut frontier = vec![start.to_path_buf()];

    for _ in 0..=max_depth {
        let mut next = Vec::new();
        // Sort each level so the result never depends on directory order.
        frontier.sort();
        for dir in frontier {
            if is_root(&dir) {
                found.push(dir);
                // A root inside a root is a bundled copy, not a second install.
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let Ok(ft) = e.file_type() else { continue };
                // Follow a directory link only if it resolves to a directory,
                // and never follow one that loops back above the start.
                if !ft.is_dir() && !(ft.is_symlink() && p.is_dir()) {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                next.push(p);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    found
}

/// Reads the version, and says which file it came from.
pub fn read_version(root: &Path) -> Option<(String, VersionSource)> {
    if let Some(v) = read_version_file(&root.join("comfyui_version.py")) {
        return Some((v, VersionSource::VersionFile));
    }
    if let Some(v) = read_pyproject_version(&root.join("pyproject.toml")) {
        return Some((v, VersionSource::PyProject));
    }
    // Older than ComfyUI 0.3.11 there is nothing on disk to read. A missing
    // version is normal and never makes an install invalid.
    None
}

/// Extracts `__version__ = "0.37.0"` without running Python.
fn read_version_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        // Keep reading: the assignment is not on the first line, and a comment
        // above it must not end the search.
        let Some(rest) = line.trim().strip_prefix("__version__") else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim();
        let Some(quote) = rest.chars().next() else { continue };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value: String = rest[1..].chars().take_while(|&c| c != quote).collect();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Extracts `version = "0.37.0"` from the `[project]` table.
fn read_pyproject_version(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_project = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_project = line == "[project]";
            continue;
        }
        if !in_project {
            continue;
        }
        let Some(rest) = line.strip_prefix("version") else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim();
        let Some(quote) = rest.chars().next() else { continue };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value: String = rest[1..].chars().take_while(|&c| c != quote).collect();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// The five model folders under `output/`.
pub fn output_model_dirs(root: &Path) -> Vec<OutputModelDir> {
    OUTPUT_MODEL_DIRS
        .iter()
        .map(|(folder, category)| {
            let path = root.join("output").join(folder);
            OutputModelDir {
                exists: path.is_dir(),
                category: category.to_string(),
                path,
            }
        })
        .collect()
}

/// Inspects a folder the person chose. Changes nothing on disk.
pub fn inspect(given: &Path) -> Result<InstallCandidate> {
    if !given.exists() {
        return Ok(InstallCandidate::invalid("That folder does not exist."));
    }
    if !given.is_dir() {
        return Ok(InstallCandidate::invalid("That is a file, not a folder."));
    }

    let given = crate::paths::canonicalize_clean(given)
        .map_err(|e| VaultError::from_io(&e, given, "opening the folder"))?;

    let roots = find_roots(&given, MAX_SEARCH_DEPTH);
    let Some(root) = roots.first().cloned() else {
        // Explain the near miss on the folder the person actually picked, which
        // is the only one they can see.
        let (found, missing) = markers_in(&given);
        let content = content_check(&given);
        let reason = if found.len() >= REQUIRED_MARKERS && !content {
            "That folder looks like a ComfyUI install, but folder_paths.py is missing the part ComfyUI needs. The folder may be incomplete.".to_string()
        } else if found.is_empty() {
            format!(
                "That folder is not a ComfyUI install, and it has no ComfyUI install inside it. The search went {MAX_SEARCH_DEPTH} folders deep."
            )
        } else {
            format!(
                "That folder has only {} of the {} files a ComfyUI install has. Missing: {}.",
                found.len(),
                ROOT_MARKERS.len(),
                missing.join(", ")
            )
        };
        let mut c = InstallCandidate::invalid(reason);
        c.markers_found = found;
        c.markers_missing = missing;
        c.content_check_passed = content;
        return Ok(c);
    };

    let nested_depth = root.components().count().saturating_sub(given.components().count());
    let (markers_found, markers_missing) = markers_in(&root);
    let (version, version_source) = match read_version(&root) {
        Some((v, s)) => (Some(v), Some(s)),
        None => (None, None),
    };

    let models_dir = root.join("models");
    let extra_file = root.join("extra_model_paths.yaml");
    let (extra_paths, extra_paths_problems, extra_paths_file) = if extra_file.is_file() {
        match extra_paths::load_file(&extra_file) {
            Ok(f) => (f.entries, f.problems, Some(extra_file)),
            Err(e) => (Vec::new(), vec![e.message.clone()], Some(extra_file)),
        }
    } else {
        (Vec::new(), Vec::new(), None)
    };

    Ok(InstallCandidate {
        valid: true,
        nested_depth,
        markers_found,
        markers_missing,
        content_check_passed: true,
        other_candidates: roots.into_iter().skip(1).collect(),
        version,
        version_source,
        models_dir_exists: models_dir.is_dir(),
        models_dir: Some(models_dir),
        extra_paths_file: extra_paths_file.clone(),
        extra_paths,
        extra_paths_problems,
        output_model_dirs: output_model_dirs(&root),
        reason: None,
        root: Some(root),
    })
}

/// Turns an inspection into an error when the folder is not an install.
pub fn require_valid(given: &Path) -> Result<InstallCandidate> {
    let c = inspect(given)?;
    if !c.valid {
        return Err(VaultError::new(
            ErrorCode::NotAComfyInstall,
            c.reason.clone().unwrap_or_else(|| "That folder is not a ComfyUI install.".into()),
        )
        .with_path(given));
    }
    Ok(c)
}

/// Every model root the engine walks for one install, with the category each
/// one belongs to when the folder itself names a category.
pub fn scan_roots(
    root: &Path,
    extra: &[ExtraPath],
    output_dirs: &[OutputModelDir],
    follow_extra: bool,
    include_output: bool,
) -> Vec<ScanRoot> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    let mut push = |path: PathBuf, category: Option<String>, origin: RootOrigin| {
        if seen.insert(path.clone()) {
            out.push(ScanRoot { path, category, origin });
        }
    };

    push(root.join("models"), None, RootOrigin::ModelsDir);

    if follow_extra {
        for e in extra {
            push(e.path.clone(), Some(e.category.clone()), RootOrigin::ExtraPath);
        }
    }
    if include_output {
        for d in output_dirs {
            push(d.path.clone(), Some(d.category.clone()), RootOrigin::OutputDir);
        }
    }
    out
}

/// Where a scanned folder came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RootOrigin {
    ModelsDir,
    ExtraPath,
    OutputDir,
    CustomNodes,
    HuggingFaceCache,
}

/// One folder the scan walks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRoot {
    pub path: PathBuf,
    /// Set when the folder itself names a category, which is the case for an
    /// extra path and for an output model folder. `None` means the category
    /// comes from the first folder below it.
    pub category: Option<String>,
    pub origin: RootOrigin,
}

#[cfg(any(test, feature = "testing"))]
pub mod fixtures {
    use super::*;

    /// Builds a folder that passes the install test.
    ///
    /// Every test tree is fabricated in a temporary folder. The engine never
    /// reads a real ComfyUI install.
    pub fn make_install(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        for f in ["main.py", "nodes.py", "execution.py", "server.py"] {
            std::fs::write(root.join(f), b"# stub\n").unwrap();
        }
        std::fs::write(
            root.join("folder_paths.py"),
            b"supported_pt_extensions = set()\nfolder_names_and_paths = {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("comfy")).unwrap();
        std::fs::create_dir_all(root.join("comfy_extras")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
    }

    pub fn set_version(root: &Path, version: &str) {
        std::fs::write(
            root.join("comfyui_version.py"),
            format!("# generated\n__version__ = \"{version}\"\n"),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_fabricated_install_is_recognized() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        assert!(is_root(&root));

        let c = inspect(&root).unwrap();
        assert!(c.valid);
        assert_eq!(c.nested_depth, 0);
        assert!(c.content_check_passed);
        assert_eq!(c.markers_found.len(), 7);
        assert!(c.models_dir_exists);
    }

    #[test]
    fn an_empty_folder_is_rejected_with_a_readable_reason() {
        let d = tmp();
        let c = inspect(d.path()).unwrap();
        assert!(!c.valid);
        let reason = c.reason.unwrap();
        assert!(reason.contains("not a ComfyUI install"), "got: {reason}");
    }

    #[test]
    fn a_folder_with_the_files_but_not_the_content_is_rejected() {
        // The false positive a marker count alone would accept: someone's own
        // project that happens to use the same file names.
        let d = tmp();
        let root = d.path().join("lookalike");
        make_install(&root);
        std::fs::write(root.join("folder_paths.py"), b"# my own project\n").unwrap();

        assert!(!is_root(&root));
        let c = inspect(&root).unwrap();
        assert!(!c.valid);
        assert!(c.reason.unwrap().contains("folder_paths.py"));
    }

    #[test]
    fn five_of_seven_markers_is_enough() {
        let d = tmp();
        let root = d.path().join("trimmed");
        make_install(&root);
        std::fs::remove_file(root.join("main.py")).unwrap();
        std::fs::remove_dir_all(root.join("comfy_extras")).unwrap();

        assert!(is_root(&root), "five markers plus the content check must pass");
        let c = inspect(&root).unwrap();
        assert!(c.valid);
        assert_eq!(c.markers_found.len(), 5);
        assert_eq!(c.markers_missing, vec!["main.py", "comfy_extras"]);
    }

    #[test]
    fn four_of_seven_markers_is_not_enough() {
        let d = tmp();
        let root = d.path().join("too_trimmed");
        make_install(&root);
        for f in ["main.py", "nodes.py", "execution.py"] {
            std::fs::remove_file(root.join(f)).unwrap();
        }
        assert!(!is_root(&root));
    }

    #[test]
    fn a_nested_install_is_found_and_the_depth_is_reported() {
        // The real layout this handles:
        // C:\ComfyUI-Main\ComfyUI-Easy-Install\ComfyUI\
        let d = tmp();
        let outer = d.path().join("ComfyUI-Main");
        let real = outer.join("ComfyUI-Easy-Install").join("ComfyUI");
        std::fs::create_dir_all(&outer).unwrap();
        make_install(&real);

        let c = inspect(&outer).unwrap();
        assert!(c.valid, "the nested install was not found");
        assert_eq!(c.root.as_deref(), Some(real.as_path()));
        assert_eq!(c.nested_depth, 2);
        assert!(c.other_candidates.is_empty());
    }

    #[test]
    fn the_shallowest_install_wins_and_the_others_are_reported() {
        let d = tmp();
        let outer = d.path().join("outer");
        let shallow = outer.join("ComfyUI");
        let deep = outer.join("branch").join("Another");
        make_install(&shallow);
        make_install(&deep);

        let c = inspect(&outer).unwrap();
        assert_eq!(c.root.as_deref(), Some(shallow.as_path()));
        assert_eq!(c.other_candidates, vec![deep]);
    }

    #[test]
    fn the_search_stops_at_the_documented_depth() {
        let d = tmp();
        let outer = d.path().join("outer");
        let too_deep = outer.join("a").join("b").join("c").join("d").join("ComfyUI");
        std::fs::create_dir_all(&outer).unwrap();
        make_install(&too_deep);

        let c = inspect(&outer).unwrap();
        assert!(!c.valid, "an install five levels down must not be found silently");
    }

    #[test]
    fn the_search_never_descends_into_the_models_folder() {
        // The whole point: models is the terabyte. Walking it during a folder
        // pick would freeze the interface.
        let d = tmp();
        let outer = d.path().join("outer");
        let hidden = outer.join("models").join("ComfyUI");
        std::fs::create_dir_all(&outer).unwrap();
        make_install(&hidden);

        assert!(find_roots(&outer, MAX_SEARCH_DEPTH).is_empty());
    }

    #[test]
    fn the_search_never_descends_into_custom_nodes() {
        let d = tmp();
        let outer = d.path().join("outer");
        let bundled = outer.join("custom_nodes").join("SomePack").join("ComfyUI");
        std::fs::create_dir_all(&outer).unwrap();
        make_install(&bundled);
        assert!(find_roots(&outer, MAX_SEARCH_DEPTH).is_empty());
    }

    #[test]
    fn a_copy_bundled_inside_a_real_install_is_not_a_second_install() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        make_install(&root.join("vendor").join("ComfyUI"));

        let roots = find_roots(&root, MAX_SEARCH_DEPTH);
        assert_eq!(roots, vec![root], "the search must stop at the first root");
    }

    #[test]
    fn the_version_comes_from_the_version_file_first() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        set_version(&root, "0.37.0");
        std::fs::write(root.join("pyproject.toml"), "[project]\nversion = \"0.0.1\"\n").unwrap();

        let c = inspect(&root).unwrap();
        assert_eq!(c.version.as_deref(), Some("0.37.0"));
        assert_eq!(c.version_source, Some(VersionSource::VersionFile));
    }

    #[test]
    fn the_version_falls_back_to_pyproject() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        std::fs::write(
            root.join("pyproject.toml"),
            "[build-system]\nversion = \"9.9.9\"\n\n[project]\nname = \"ComfyUI\"\nversion = \"0.30.2\"\n",
        )
        .unwrap();

        let c = inspect(&root).unwrap();
        assert_eq!(c.version.as_deref(), Some("0.30.2"), "the wrong table was read");
        assert_eq!(c.version_source, Some(VersionSource::PyProject));
    }

    #[test]
    fn a_missing_version_does_not_make_an_install_invalid() {
        // Installs older than ComfyUI 0.3.11 have no version on disk at all.
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        let c = inspect(&root).unwrap();
        assert!(c.valid);
        assert_eq!(c.version, None);
        assert_eq!(c.version_source, None);
    }

    #[test]
    fn a_version_in_single_quotes_is_read() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        std::fs::write(root.join("comfyui_version.py"), "__version__ = '0.28.0'\n").unwrap();
        assert_eq!(read_version(&root).unwrap().0, "0.28.0");
    }

    #[test]
    fn the_extra_paths_file_is_read_when_it_is_there() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        let shared = d.path().join("shared");
        std::fs::create_dir_all(shared.join("loras")).unwrap();
        std::fs::write(
            root.join("extra_model_paths.yaml"),
            format!("other:\n    base_path: {}\n    loras: loras\n", shared.display()),
        )
        .unwrap();

        let c = inspect(&root).unwrap();
        assert_eq!(c.extra_paths.len(), 1);
        assert_eq!(c.extra_paths[0].category, "loras");
        assert!(c.extra_paths[0].exists);
        assert!(c.extra_paths_problems.is_empty());
        assert!(c.extra_paths_file.is_some());
    }

    #[test]
    fn a_broken_extra_paths_file_is_reported_and_the_install_stays_valid() {
        // The install still works for everything else, so refusing to register
        // it would be the wrong answer. The person gets told instead.
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        std::fs::write(root.join("extra_model_paths.yaml"), "comfyui:\n  base: [unclosed\n").unwrap();

        let c = inspect(&root).unwrap();
        assert!(c.valid);
        assert!(!c.extra_paths_problems.is_empty());
        assert!(c.extra_paths.is_empty());
    }

    #[test]
    fn the_five_output_model_folders_are_reported() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        std::fs::create_dir_all(root.join("output").join("loras")).unwrap();

        let c = inspect(&root).unwrap();
        assert_eq!(c.output_model_dirs.len(), 5);
        let loras = c.output_model_dirs.iter().find(|o| o.path.ends_with("loras")).unwrap();
        assert!(loras.exists);
        let vae = c.output_model_dirs.iter().find(|o| o.path.ends_with("vae")).unwrap();
        assert!(!vae.exists);
        // output/clip is the text_encoders category, after the legacy rename.
        let clip = c.output_model_dirs.iter().find(|o| o.path.ends_with("clip")).unwrap();
        assert_eq!(clip.category, "text_encoders");
    }

    #[test]
    fn scan_roots_covers_models_extras_and_output_without_repeats() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        let c = inspect(&root).unwrap();

        let roots = scan_roots(&root, &c.extra_paths, &c.output_model_dirs, true, true);
        assert_eq!(roots.len(), 6, "models plus the five output folders");
        assert_eq!(roots[0].origin, RootOrigin::ModelsDir);
        assert!(roots[0].category.is_none(), "the models folder names no category itself");

        let only_models = scan_roots(&root, &c.extra_paths, &c.output_model_dirs, true, false);
        assert_eq!(only_models.len(), 1);
    }

    #[test]
    fn scan_roots_drops_a_duplicate_folder() {
        let d = tmp();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        // An extra path that points back at the models folder.
        let extra = vec![ExtraPath {
            section: "self".into(),
            category: "loras".into(),
            raw_category: "loras".into(),
            path: root.join("models"),
            is_default: false,
            exists: true,
        }];
        let roots = scan_roots(&root, &extra, &[], true, false);
        assert_eq!(roots.len(), 1, "the same folder must be walked once");
    }

    #[test]
    fn require_valid_turns_a_bad_folder_into_a_clear_error() {
        let d = tmp();
        let err = require_valid(d.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAComfyInstall);
        assert!(err.path.is_some());
    }

    #[test]
    fn inspecting_a_file_says_so() {
        let d = tmp();
        let f = d.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        let c = inspect(&f).unwrap();
        assert!(!c.valid);
        assert!(c.reason.unwrap().contains("file, not a folder"));
    }

    #[test]
    fn inspecting_a_missing_folder_says_so() {
        let d = tmp();
        let c = inspect(&d.path().join("nope")).unwrap();
        assert!(!c.valid);
        assert!(c.reason.unwrap().contains("does not exist"));
    }

    #[test]
    fn the_search_result_does_not_depend_on_directory_order() {
        // Two installs at the same depth must come back in a stable order, or
        // the interface would offer a different one on every pick.
        let d = tmp();
        let outer = d.path().join("outer");
        make_install(&outer.join("zzz"));
        make_install(&outer.join("aaa"));

        let first = find_roots(&outer, MAX_SEARCH_DEPTH);
        let second = find_roots(&outer, MAX_SEARCH_DEPTH);
        assert_eq!(first, second);
        assert!(first[0].ends_with("aaa"), "sorted order expected, got {first:?}");
    }
}
