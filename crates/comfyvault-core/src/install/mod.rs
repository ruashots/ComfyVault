//! Registered ComfyUI installs: recognizing them, reading their configuration,
//! and listing the folders a scan must walk.

pub mod detect;
pub mod extra_paths;
pub mod pypath;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use detect::{
    InstallCandidate, OutputModelDir, RootOrigin, ScanRoot, VersionSource, OUTPUT_MODEL_DIRS,
    ROOT_MARKERS,
};
pub use extra_paths::{ExtraPath, ExtraPathsFile};

/// A ComfyUI install the person registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Install {
    pub id: String,
    pub label: String,
    /// The folder the person picked, which is not always the real root.
    pub registered_path: PathBuf,
    /// The real ComfyUI root.
    pub root: PathBuf,
    pub models_dir: PathBuf,
    pub version: Option<String>,
    pub version_source: Option<VersionSource>,
    pub extra_paths: Vec<ExtraPath>,
    pub output_model_dirs: Vec<OutputModelDir>,
    pub added_at: crate::time_util::Timestamp,
    pub last_scan_at: Option<crate::time_util::Timestamp>,
    /// What the last scan found in this install. `None` until it is scanned.
    #[serde(default)]
    pub last_scan_totals: Option<crate::store::InstallScanTotals>,
}

impl Install {
    /// Builds a record from an inspection the caller already ran.
    pub fn from_candidate(
        id: String,
        label: String,
        registered_path: PathBuf,
        c: &InstallCandidate,
    ) -> crate::Result<Self> {
        let root = c.root.clone().ok_or_else(|| {
            crate::VaultError::new(
                crate::ErrorCode::NotAComfyInstall,
                "That folder is not a ComfyUI install.",
            )
        })?;
        Ok(Self {
            id,
            label,
            registered_path,
            models_dir: c.models_dir.clone().unwrap_or_else(|| root.join("models")),
            root,
            version: c.version.clone(),
            version_source: c.version_source,
            extra_paths: c.extra_paths.clone(),
            output_model_dirs: c.output_model_dirs.clone(),
            added_at: crate::time_util::Timestamp::now(),
            last_scan_at: None,
            last_scan_totals: None,
        })
    }

    /// The install as the disk shows it now, keeping this record's identity.
    ///
    /// A vault's database can come from another computer or from someone
    /// else, so a stored install is a claim, not a fact. Before a stored
    /// install decides where the engine reads, writes or deletes, its folder
    /// is inspected again, and its model folders come from that inspection.
    pub fn proved(&self) -> crate::Result<Self> {
        let c = detect::require_valid(&self.root)?;
        let root = c.root.clone().unwrap_or_default();
        if !crate::paths::same_path_lexically(&root, &self.root) {
            return Err(crate::VaultError::new(
                crate::ErrorCode::NotAComfyInstall,
                "That install's folder is not a ComfyUI install any more.",
            )
            .with_path(&self.root));
        }
        let mut fresh = Self::from_candidate(self.id.clone(), self.label.clone(), self.registered_path.clone(), &c)?;
        fresh.added_at = self.added_at;
        fresh.last_scan_at = self.last_scan_at;
        fresh.last_scan_totals = self.last_scan_totals.clone();
        Ok(fresh)
    }

    /// A label for a new install, when the person does not give one.
    ///
    /// A launcher keeps the real install in a folder called `ComfyUI` inside
    /// its own folder, so the root's name is `ComfyUI` for almost everyone,
    /// and every install ends up with the same name. The folder the person
    /// picked says which one it is, so that name comes first. If another
    /// install already has it, the next folder up is tried, and so on. The
    /// whole root is the last resort, and it is always different.
    pub fn unique_default_label(picked: &std::path::Path, root: &std::path::Path, taken: &[String]) -> String {
        let start = if crate::paths::same_path_lexically(picked, root) { root } else { picked };
        let is_taken = |name: &str| taken.iter().any(|t| t.trim().to_lowercase() == name.to_lowercase());
        start
            .ancestors()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .find(|n| !n.is_empty() && !is_taken(n))
            .unwrap_or_else(|| crate::paths::display_path(root))
    }

    /// Does this install's ComfyUI reject a per-file symbolic link on the model
    /// thumbnail route?
    ///
    /// ComfyUI 0.28.0 added a containment check to that route that resolves
    /// symbolic links and refuses one that leaves the model folder. The effect
    /// is limited to thumbnails in the model browser. Loading a model and
    /// running a workflow are not affected, because the file walk follows links
    /// on purpose.
    ///
    /// `None` means the version could not be read, so the answer is unknown and
    /// the interface must say "unknown" rather than "no".
    pub fn thumbnails_affected(&self) -> Option<bool> {
        let v = self.version.as_deref()?;
        Some(version_at_least(v, (0, 28, 0)))
    }

    /// Every folder a scan walks for this install.
    pub fn scan_roots(&self, follow_extra: bool, include_output: bool) -> Vec<ScanRoot> {
        detect::scan_roots(
            &self.root,
            &self.extra_paths,
            &self.output_model_dirs,
            follow_extra,
            include_output,
        )
    }

    /// The category folders directly inside the scanned `models` folders,
    /// such as `models\loras`, with where each really is.
    ///
    /// ComfyUI reads each of them for its category wherever it leads, so a
    /// category folder that is a junction to another drive holds the
    /// install's own models. Only these folders count. A link further down
    /// leads somewhere the person never declared.
    ///
    /// A folder whose real place is one of `fences`, holds one, or is inside
    /// one is left out. The fences are every registered install's folder and
    /// its `custom_nodes`, and the vault ([`Install::fences`]): a link into
    /// another install would make that install's files, bundled weights
    /// included, look like this one's models.
    pub fn category_folders(&self, follow_extra: bool, include_output: bool, fences: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
        let real = |p: &Path| crate::paths::canonicalize_clean(p).ok();
        let mut out = Vec::new();
        for root in self.scan_roots(follow_extra, include_output) {
            if root.origin != RootOrigin::ModelsDir {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&root.path) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                let Some(place) = real(&path).filter(|p| p.is_dir()) else { continue };
                if fences.iter().any(|f| f.starts_with(&place) || place.starts_with(f)) {
                    continue;
                }
                out.push((path, place));
            }
        }
        out
    }

    /// Where every registered install's folder and `custom_nodes` really are,
    /// and the vault: the places no category folder may lead to, hold, or sit
    /// inside.
    pub fn fences(installs: &[Install], vault: &Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = installs
            .iter()
            .flat_map(|i| [i.root.clone(), i.custom_nodes_dir()])
            .chain(std::iter::once(vault.to_path_buf()))
            .filter_map(|p| crate::paths::canonicalize_clean(&p).ok())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Every registered install's `custom_nodes`, as written and where it
    /// really is. A file inside any of them is counted and never moved,
    /// whichever install's folders reached it.
    pub fn all_custom_nodes(installs: &[Install]) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for i in installs {
            let written = i.custom_nodes_dir();
            if let Ok(real) = crate::paths::canonicalize_clean(&written) {
                if real != written {
                    out.push(real);
                }
            }
            out.push(written);
        }
        out
    }

    /// The folder that holds bundled weights, which the scan counts and never
    /// moves.
    pub fn custom_nodes_dir(&self) -> PathBuf {
        self.root.join("custom_nodes")
    }

    /// Roots that can receive a link the person creates by hand.
    ///
    /// A link outside these folders would be invisible to ComfyUI, so the
    /// engine refuses to create one there.
    ///
    /// An extra path wide enough to contain the install itself is dropped. A
    /// line like `base_path: C:\ComfyUI` with `loras: .` makes the whole
    /// install a declared model folder, and `custom_nodes` is then inside the
    /// boundary. ComfyUI imports `custom_nodes/<pack>/__init__.py` at startup,
    /// so a link written there is a link the engine put on the import path. An
    /// extra path is a model folder; a folder that contains the whole install
    /// is not one.
    pub fn link_boundaries(&self) -> Vec<PathBuf> {
        let custom_nodes = self.custom_nodes_dir();
        let mut out = vec![self.models_dir.clone()];
        out.extend(
            self.extra_paths
                .iter()
                .map(|e| e.path.clone())
                .filter(|p| !custom_nodes.starts_with(p) && !p.starts_with(&custom_nodes) && !self.root.starts_with(p)),
        );
        out.extend(self.output_model_dirs.iter().map(|o| o.path.clone()));
        out
    }
}

/// Compares a dotted version against a minimum.
///
/// Anything after the numbers, such as `0.28.0rc1`, is ignored. An unreadable
/// version answers `false`, because promising a person that nothing changes is
/// worse than warning them for nothing.
pub fn version_at_least(v: &str, min: (u32, u32, u32)) -> bool {
    let nums: Vec<u32> = v
        .split(['.', '-', '+'])
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .unwrap_or(0)
        })
        .collect();
    let got = (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
        nums.get(2).copied().unwrap_or(0),
    );
    got >= min
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::detect::fixtures::*;

    #[test]
    fn version_comparison_handles_the_shapes_comfyui_uses() {
        assert!(version_at_least("0.28.0", (0, 28, 0)));
        assert!(version_at_least("0.37.0", (0, 28, 0)));
        assert!(version_at_least("1.0.0", (0, 28, 0)));
        assert!(!version_at_least("0.27.9", (0, 28, 0)));
        assert!(!version_at_least("0.3.11", (0, 28, 0)), "0.3.11 is older than 0.28.0");
        assert!(version_at_least("0.28", (0, 28, 0)));
        assert!(version_at_least("0.28.0rc1", (0, 28, 0)));
        assert!(!version_at_least("", (0, 28, 0)));
        assert!(!version_at_least("unknown", (0, 28, 0)));
    }

    #[test]
    fn the_thumbnail_answer_is_unknown_when_the_version_is_unknown() {
        // Saying "no" here would promise the person something the engine cannot
        // check. The interface has to show "unknown".
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate("id".into(), "label".into(), root.clone(), &c).unwrap();
        assert_eq!(i.thumbnails_affected(), None);
    }

    #[test]
    fn a_recent_comfyui_is_flagged_for_the_thumbnail_change() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        set_version(&root, "0.37.0");
        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate("id".into(), "label".into(), root.clone(), &c).unwrap();
        assert_eq!(i.thumbnails_affected(), Some(true));
    }

    #[test]
    fn an_older_comfyui_is_not_flagged() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        set_version(&root, "0.27.5");
        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate("id".into(), "label".into(), root.clone(), &c).unwrap();
        assert_eq!(i.thumbnails_affected(), Some(false));
    }

    #[test]
    fn link_boundaries_cover_every_folder_comfyui_reads() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        let shared = d.path().join("shared");
        std::fs::create_dir_all(shared.join("loras")).unwrap();
        std::fs::write(
            root.join("extra_model_paths.yaml"),
            format!("other:\n    base_path: {}\n    loras: loras\n", shared.display()),
        )
        .unwrap();

        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate("id".into(), "label".into(), root.clone(), &c).unwrap();
        let b = i.link_boundaries();
        assert!(b.contains(&root.join("models")));
        assert!(b.contains(&shared.join("loras")));
        assert_eq!(b.len(), 1 + 1 + 5);
    }

    #[test]
    fn a_new_install_is_named_after_the_folder_the_person_picked() {
        // A launcher layout: the real root is always a folder called ComfyUI.
        let picked = std::path::Path::new("/AI/ComfyUI-Easy-Install");
        let root = picked.join("ComfyUI");
        assert_eq!(Install::unique_default_label(picked, &root, &[]), "ComfyUI-Easy-Install");
    }

    #[test]
    fn a_name_another_install_has_moves_up_one_folder() {
        let root = std::path::Path::new("/ComfyUI_windows_portable/ComfyUI");
        let taken = vec!["ComfyUI".to_string()];
        assert_eq!(Install::unique_default_label(root, root, &taken), "ComfyUI_windows_portable");
        // Windows does not tell the two apart, and neither does a person.
        let taken = vec!["comfyui".to_string()];
        assert_eq!(Install::unique_default_label(root, root, &taken), "ComfyUI_windows_portable");
        // The control: nothing taken, and the root's own name stays.
        assert_eq!(Install::unique_default_label(root, root, &[]), "ComfyUI");
    }

    #[test]
    fn when_every_folder_name_is_taken_the_whole_path_is_the_name() {
        let root = std::path::Path::new("/A/ComfyUI");
        let taken = vec!["ComfyUI".to_string(), "A".to_string()];
        assert_eq!(
            Install::unique_default_label(root, root, &taken),
            crate::paths::display_path(root)
        );
    }

    #[test]
    fn the_install_record_round_trips_through_json() {
        // The UI receives exactly this shape.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ComfyUI");
        make_install(&root);
        set_version(&root, "0.37.0");
        let c = detect::inspect(&root).unwrap();
        let i = Install::from_candidate("id-1".into(), "Production".into(), root.clone(), &c).unwrap();

        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["id"], "id-1");
        assert_eq!(json["label"], "Production");
        assert_eq!(json["version"], "0.37.0");
        assert_eq!(json["versionSource"], "comfyui_version.py");
        assert!(json.get("modelsDir").is_some(), "field names must be camelCase");
        assert!(json.get("outputModelDirs").is_some());

        let back: Install = serde_json::from_value(json).unwrap();
        assert_eq!(back.id, i.id);
        assert_eq!(back.root, i.root);
    }
}
