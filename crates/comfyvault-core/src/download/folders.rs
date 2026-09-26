//! Where ComfyUI looks for a category's models, and so where a link goes.
//!
//! This follows ComfyUI's own `folder_paths.py` and `utils/extra_config.py`,
//! read on 2026-09-26:
//!
//! * Each built-in category starts with a fixed list of folders under
//!   `models`. Two of them have a second, older name: `diffusion_models`
//!   lists `models/unet` first, and `text_encoders` lists `models/clip`
//!   second.
//! * `extra_model_paths.yaml` is read in file order. A folder marked
//!   `is_default: true` is put at the front of its category's list, so the
//!   last one read ends up first. Any other folder is added at the end.
//! * The five output model folders are added after that.
//! * To load a model by name, ComfyUI takes the first folder in the list that
//!   holds a file of that name. So of two files with one name, only one loads.
//!
//! From that, two rules:
//!
//! * **The link goes where ComfyUI saves new files of that category**: the
//!   folder `extra_model_paths.yaml` marks `is_default` for it, if there is
//!   one, and otherwise `models\{category}`. A category ComfyUI does not know
//!   itself, which only the YAML file names, goes into the first folder the
//!   YAML file gives it.
//! * **A name is taken when any folder ComfyUI searches for that category
//!   already holds it.** Wherever the link went, one of the two would never
//!   load.

use std::path::{Path, PathBuf};

use crate::install::extra_paths::map_legacy;
use crate::install::Install;

/// The folders ComfyUI lists for its own categories, under `models`, in its
/// order.
const BUILT_IN: &[(&str, &[&str])] = &[
    ("checkpoints", &["checkpoints"]),
    ("loras", &["loras"]),
    ("vae", &["vae"]),
    ("text_encoders", &["text_encoders", "clip"]),
    ("diffusion_models", &["unet", "diffusion_models"]),
    ("clip_vision", &["clip_vision"]),
    ("style_models", &["style_models"]),
    ("embeddings", &["embeddings"]),
    ("vae_approx", &["vae_approx"]),
    ("controlnet", &["controlnet", "t2i_adapter"]),
    ("gligen", &["gligen"]),
    ("upscale_models", &["upscale_models"]),
    ("latent_upscale_models", &["latent_upscale_models"]),
    ("hypernetworks", &["hypernetworks"]),
    ("photomaker", &["photomaker"]),
    ("classifiers", &["classifiers"]),
    ("model_patches", &["model_patches"]),
    ("audio_encoders", &["audio_encoders"]),
    ("background_removal", &["background_removal"]),
    ("frame_interpolation", &["frame_interpolation"]),
    ("geometry_estimation", &["geometry_estimation"]),
    ("optical_flow", &["optical_flow"]),
    ("detection", &["detection"]),
];

/// The categories ComfyUI knows itself, for the folder menu.
pub fn built_in_categories() -> impl Iterator<Item = &'static str> {
    BUILT_IN.iter().map(|(c, _)| *c)
}

/// Every folder ComfyUI searches for `category` in this install, in its order.
pub fn searched(install: &Install, category: &str) -> Vec<PathBuf> {
    let name = map_legacy(category);
    let mut list: Vec<PathBuf> = BUILT_IN
        .iter()
        .find(|(c, _)| *c == name)
        .map(|(_, dirs)| dirs.iter().map(|d| install.models_dir.join(d)).collect())
        .unwrap_or_default();
    for e in install.extra_paths.iter().filter(|e| e.category == name) {
        let already = list.iter().position(|p| crate::paths::same_path_lexically(p, &e.path));
        if e.is_default {
            if let Some(i) = already {
                list.remove(i);
            }
            list.insert(0, e.path.clone());
        } else if already.is_none() {
            list.push(e.path.clone());
        }
    }
    for o in install.output_model_dirs.iter().filter(|o| map_legacy(&o.category) == name) {
        if !list.iter().any(|p| crate::paths::same_path_lexically(p, &o.path)) {
            list.push(o.path.clone());
        }
    }
    // A category nothing registers is, by custom node convention, a folder of
    // that name under `models`.
    let own = install.models_dir.join(category);
    if !list.iter().any(|p| crate::paths::same_path_lexically(p, &own))
        && (list.is_empty() || is_built_in(name))
    {
        list.push(own);
    }
    list
}

/// The folder a new link for `category` goes into in this install.
pub fn link_folder(install: &Install, category: &str) -> PathBuf {
    let name = map_legacy(category);
    let own = install.models_dir.join(category);
    let boundaries = install.link_boundaries();
    let allowed = |p: &Path| boundaries.iter().any(|b| p.starts_with(b));

    let has_default = install.extra_paths.iter().any(|e| e.category == name && e.is_default);
    let only_yaml = !is_built_in(name) && install.extra_paths.iter().any(|e| e.category == name);
    if has_default || only_yaml {
        if let Some(first) = searched(install, category).into_iter().next() {
            if allowed(&first) {
                return first;
            }
        }
    }
    own
}

fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(c, _)| *c == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::ExtraPath;

    fn install(root: &Path, extra: Vec<(&str, &str, bool)>) -> Install {
        Install {
            id: "i".into(),
            label: "A".into(),
            registered_path: root.into(),
            root: root.into(),
            models_dir: root.join("models"),
            version: None,
            version_source: None,
            extra_paths: extra
                .into_iter()
                .map(|(cat, path, is_default)| ExtraPath {
                    section: "s".into(),
                    category: map_legacy(cat).into(),
                    raw_category: cat.into(),
                    path: PathBuf::from(path),
                    is_default,
                    exists: true,
                })
                .collect(),
            output_model_dirs: vec![crate::install::OutputModelDir {
                path: root.join("output/checkpoints"),
                category: "checkpoints".into(),
                exists: true,
            }],
            added_at: crate::time_util::Timestamp::now(),
            last_scan_at: None,
            last_scan_totals: None,
        }
    }

    #[test]
    fn with_no_yaml_the_link_goes_in_the_models_folder_of_that_name() {
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![]);
        assert_eq!(link_folder(&i, "checkpoints"), r.join("models/checkpoints"));
        // ComfyUI lists models/unet first, but the vault's name is the one
        // the person reads, and both are searched.
        assert_eq!(link_folder(&i, "diffusion_models"), r.join("models/diffusion_models"));
        assert_eq!(
            searched(&i, "diffusion_models"),
            vec![r.join("models/unet"), r.join("models/diffusion_models")]
        );
        assert_eq!(
            searched(&i, "checkpoints"),
            vec![r.join("models/checkpoints"), r.join("output/checkpoints")]
        );
    }

    #[test]
    fn a_folder_marked_default_in_the_yaml_gets_the_link() {
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("checkpoints", "/shared/ckpt", true)]);
        assert_eq!(link_folder(&i, "checkpoints"), PathBuf::from("/shared/ckpt"));
        assert_eq!(searched(&i, "checkpoints")[0], PathBuf::from("/shared/ckpt"));
        assert!(searched(&i, "checkpoints").contains(&r.join("models/checkpoints")));
    }

    #[test]
    fn the_last_default_folder_read_is_the_first_one_searched() {
        // Each `is_default` folder is put at the front in turn.
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("loras", "/a", true), ("loras", "/b", true)]);
        assert_eq!(link_folder(&i, "loras"), PathBuf::from("/b"));
        assert_eq!(searched(&i, "loras")[..3], [PathBuf::from("/b"), PathBuf::from("/a"), r.join("models/loras")]);
    }

    #[test]
    fn a_folder_not_marked_default_is_searched_last_and_gets_no_link() {
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("loras", "/shared/loras", false)]);
        assert_eq!(link_folder(&i, "loras"), r.join("models/loras"));
        assert_eq!(searched(&i, "loras"), vec![r.join("models/loras"), PathBuf::from("/shared/loras")]);
    }

    #[test]
    fn an_old_category_name_in_the_yaml_counts_as_the_new_one() {
        // `unet:` in the file is `diffusion_models` to ComfyUI.
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("unet", "/shared/unet", true)]);
        assert_eq!(link_folder(&i, "diffusion_models"), PathBuf::from("/shared/unet"));
    }

    #[test]
    fn a_category_only_the_yaml_names_goes_into_its_first_folder() {
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("ipadapter", "/shared/ipadapter", false)]);
        assert_eq!(link_folder(&i, "ipadapter"), PathBuf::from("/shared/ipadapter"));
        assert_eq!(searched(&i, "ipadapter"), vec![PathBuf::from("/shared/ipadapter")]);
        // Nothing names it at all: the custom node convention.
        let bare = install(r, vec![]);
        assert_eq!(link_folder(&bare, "ipadapter"), r.join("models/ipadapter"));
        assert_eq!(searched(&bare, "ipadapter"), vec![r.join("models/ipadapter")]);
    }

    #[test]
    fn a_default_folder_that_holds_the_whole_install_never_gets_a_link() {
        // Such a folder is not a model folder: custom_nodes is inside it.
        let r = Path::new("/c/ComfyUI");
        let i = install(r, vec![("checkpoints", "/c", true)]);
        assert_eq!(link_folder(&i, "checkpoints"), r.join("models/checkpoints"));
    }
}
