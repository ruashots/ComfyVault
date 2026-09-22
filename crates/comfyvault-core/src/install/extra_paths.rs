//! Reads `extra_model_paths.yaml` exactly the way ComfyUI reads it.
//!
//! ComfyUI's whole parser is `utils/extra_config.py`, and this module
//! reproduces its behavior rule for rule, including the surprising parts.
//! Agreement matters: if the engine resolved a folder differently, it would
//! scan a folder ComfyUI never reads, and the person's duplicates would stay on
//! disk.
//!
//! The rules that catch people out, all reproduced here:
//!
//! * `~` and environment variables are expanded on `base_path` **only**. A
//!   category value written as `~/models` stays a literal folder named `~`.
//! * Several folders in one entry are split on a newline and on nothing else.
//!   A comma or a semicolon is part of the path.
//! * Whitespace is not stripped, so a trailing space stays in the folder name.
//! * A category path that is absolute discards `base_path` entirely.
//! * A relative path without `base_path` resolves against the folder holding
//!   the YAML file, not the ComfyUI root and not the working directory.
//! * `base_path: ""` counts as no base path at all, because Python treats an
//!   empty string as false.
//! * `unet` means `diffusion_models` and `clip` means `text_encoders`.
//! * The only reserved keys are `base_path` and `is_default`. Every other key
//!   names a model category, and an unknown category is accepted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::pypath::{self, PathStyle};
use crate::error::{ErrorCode, Result, VaultError};

/// One folder ComfyUI adds to a model category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraPath {
    /// The top-level key in the YAML file. ComfyUI ignores it; people use it as
    /// a label, so the interface shows it.
    pub section: String,
    /// The category after legacy renaming. This is what ComfyUI actually uses.
    pub category: String,
    /// The category as written in the file.
    pub raw_category: String,
    pub path: PathBuf,
    pub is_default: bool,
    pub exists: bool,
}

/// The result of reading one file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraPathsFile {
    pub path: PathBuf,
    pub entries: Vec<ExtraPath>,
    /// Entries this file contains that ComfyUI cannot read. ComfyUI stops with
    /// an error on these, so the person needs to know.
    pub problems: Vec<String>,
}

/// Everything the resolver needs from outside, so tests can fix it.
#[derive(Debug, Clone)]
pub struct ResolveContext {
    pub style: PathStyle,
    /// The folder holding the YAML file. Relative paths resolve against it.
    pub yaml_dir: String,
    pub home: Option<String>,
    pub env: HashMap<String, String>,
}

impl ResolveContext {
    /// Reads the real environment.
    pub fn for_file(yaml_path: &Path) -> Self {
        let yaml_dir = yaml_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            style: PathStyle::host(),
            yaml_dir,
            home: std::env::var("USERPROFILE").ok().or_else(|| std::env::var("HOME").ok()),
            env: std::env::vars().collect(),
        }
    }
}

/// ComfyUI renames two category names on the way in. Reproduced from
/// `folder_paths.map_legacy`.
pub fn map_legacy(name: &str) -> &str {
    match name {
        "unet" => "diffusion_models",
        "clip" => "text_encoders",
        other => other,
    }
}

const RESERVED_KEYS: [&str; 2] = ["base_path", "is_default"];

/// Reads and resolves a file from disk.
pub fn load_file(yaml_path: &Path) -> Result<ExtraPathsFile> {
    let text = std::fs::read_to_string(yaml_path)
        .map_err(|e| VaultError::from_io(&e, yaml_path, "reading the extra model paths file"))?;
    let ctx = ResolveContext::for_file(yaml_path);
    let mut parsed = parse(&text, &ctx)?;
    parsed.path = yaml_path.to_path_buf();
    for e in &mut parsed.entries {
        e.exists = e.path.is_dir();
    }
    Ok(parsed)
}

/// Parses YAML text. Does not touch the disk, so it is fully unit tested.
pub fn parse(text: &str, ctx: &ResolveContext) -> Result<ExtraPathsFile> {
    use serde_yaml_ng::Value;

    let doc: Value = serde_yaml_ng::from_str(text).map_err(|e| {
        VaultError::new(
            ErrorCode::ParseError,
            "That extra model paths file is not valid YAML, so ComfyUI cannot read it either.",
        )
        .with_detail(e.to_string())
    })?;

    let mut out = ExtraPathsFile::default();

    // An empty file parses to null. ComfyUI's `for c in config` would raise, so
    // the engine treats it as an empty file rather than an error.
    let Value::Mapping(sections) = doc else {
        if matches!(doc, Value::Null) {
            return Ok(out);
        }
        return Err(VaultError::new(
            ErrorCode::ParseError,
            "That extra model paths file must be a list of named sections.",
        )
        .with_detail(format!("the file's top level is {}", value_kind(&doc))));
    };

    for (section_key, section_val) in sections {
        let section = scalar_to_string(&section_key).unwrap_or_else(|| "?".to_string());

        // `if conf is None: continue` - an empty section is skipped in silence.
        if matches!(section_val, Value::Null) {
            continue;
        }
        let Value::Mapping(conf) = section_val else {
            out.problems.push(format!(
                "Section \"{section}\" is not a list of folders, so ComfyUI stops with an error when it reads this file."
            ));
            continue;
        };

        // base_path: expanded, then made absolute against the YAML folder.
        let base_path: Option<String> = conf
            .get(Value::from("base_path"))
            .and_then(scalar_to_string)
            .map(|raw| {
                let expanded = pypath::expandvars(
                    ctx.style,
                    &pypath::expanduser(ctx.style, &raw, ctx.home.as_deref()),
                    &ctx.env,
                );
                if pypath::isabs(ctx.style, &expanded) {
                    expanded
                } else {
                    pypath::abspath(ctx.style, &ctx.yaml_dir, &expanded)
                }
            })
            // Python treats an empty string as false, so `base_path: ""` is the
            // same as writing no base path at all.
            .filter(|s| !s.is_empty());

        let is_default = conf
            .get(Value::from("is_default"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        for (cat_key, cat_val) in &conf {
            let Some(raw_category) = scalar_to_string(cat_key) else {
                continue;
            };
            if RESERVED_KEYS.contains(&raw_category.as_str()) {
                continue;
            }

            // ComfyUI calls `.split("\n")` straight on the value. Anything that
            // is not a string stops ComfyUI with an error.
            let Some(raw_value) = cat_val.as_str() else {
                out.problems.push(format!(
                    "In section \"{section}\", the entry \"{raw_category}\" is {}, but ComfyUI needs text. ComfyUI stops with an error when it reads this file.",
                    value_kind(cat_val)
                ));
                continue;
            };

            // A category names a folder inside the vault, so it must be one
            // safe path component. ComfyUI accepts any key here, and this file
            // is edited by hand and written by launchers and node packs, so a
            // key like "..\\..\\x" or a pasted Windows path reaches the engine
            // as a folder name. Without this, that name is joined onto the
            // vault root and the person's weights land anywhere they can write.
            if let Err(e) = crate::paths::validate_file_name(&raw_category) {
                out.problems.push(format!(
                    "In section \"{section}\", \"{raw_category}\" cannot be a model folder name, so it was skipped. {}",
                    e.message
                ));
                continue;
            }

            let category = map_legacy(&raw_category).to_string();

            for line in raw_value.split('\n') {
                // `if len(y) == 0: continue`. Note that a line of spaces is not
                // empty, and ComfyUI keeps it.
                if line.is_empty() {
                    continue;
                }
                let full = match &base_path {
                    Some(base) => pypath::join(ctx.style, base, line),
                    None => {
                        if pypath::isabs(ctx.style, line) {
                            line.to_string()
                        } else {
                            pypath::abspath(ctx.style, &ctx.yaml_dir, line)
                        }
                    }
                };
                let normalized = pypath::normpath(ctx.style, &full);

                out.entries.push(ExtraPath {
                    section: section.clone(),
                    category: category.clone(),
                    raw_category: raw_category.clone(),
                    path: PathBuf::from(normalized),
                    is_default,
                    exists: false,
                });
            }
        }
    }

    Ok(out)
}

fn scalar_to_string(v: &serde_yaml_ng::Value) -> Option<String> {
    use serde_yaml_ng::Value;
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_kind(v: &serde_yaml_ng::Value) -> &'static str {
    use serde_yaml_ng::Value;
    match v {
        Value::Null => "empty",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Sequence(_) => "a list",
        Value::Mapping(_) => "a group of entries",
        Value::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::pypath::PathStyle::{Posix, Windows};

    fn ctx(style: PathStyle, yaml_dir: &str) -> ResolveContext {
        ResolveContext {
            style,
            yaml_dir: yaml_dir.to_string(),
            home: Some(if style == Windows { r"C:\Users\alex".into() } else { "/home/alex".into() }),
            env: [("MODELS".to_string(), if style == Windows { r"D:\weights".to_string() } else { "/weights".to_string() })]
                .into_iter()
                .collect(),
        }
    }

    fn paths_for(f: &ExtraPathsFile, category: &str) -> Vec<String> {
        f.entries
            .iter()
            .filter(|e| e.category == category)
            .map(|e| e.path.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn resolves_a_plain_section_against_its_base_path() {
        let yaml = "
comfyui:
    base_path: /opt/comfy
    checkpoints: models/checkpoints
    loras: models/loras
";
        let f = parse(yaml, &ctx(Posix, "/etc/comfy")).unwrap();
        assert_eq!(paths_for(&f, "checkpoints"), vec!["/opt/comfy/models/checkpoints"]);
        assert_eq!(paths_for(&f, "loras"), vec!["/opt/comfy/models/loras"]);
        assert!(f.problems.is_empty());
    }

    #[test]
    fn splits_several_folders_on_newlines_only() {
        // The block scalar form people actually write.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    loras: |
        models/loras
        models/loras2

        models/loras3
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(
            paths_for(&f, "loras"),
            vec!["/opt/comfy/models/loras", "/opt/comfy/models/loras2", "/opt/comfy/models/loras3"],
            "blank lines are skipped, every other line is a folder"
        );
    }

    #[test]
    fn a_comma_is_part_of_the_path_not_a_separator() {
        let yaml = "
comfyui:
    base_path: /opt/comfy
    loras: a,b
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec!["/opt/comfy/a,b"]);
    }

    #[test]
    fn tilde_is_expanded_on_base_path_and_not_on_a_category() {
        // The rule people get wrong most often. A `~` in a category value is a
        // real folder named `~`, and ComfyUI looks for exactly that.
        let yaml = "
withbase:
    base_path: ~/comfy
    checkpoints: models/checkpoints
nobase:
    embeddings: ~/tildetest/y
";
        let f = parse(yaml, &ctx(Posix, "/etc/comfy")).unwrap();
        assert_eq!(paths_for(&f, "checkpoints"), vec!["/home/alex/comfy/models/checkpoints"]);
        assert_eq!(
            paths_for(&f, "embeddings"),
            vec!["/etc/comfy/~/tildetest/y"],
            "a tilde in a category value must stay a literal folder"
        );
    }

    #[test]
    fn environment_variables_expand_on_base_path_and_not_on_a_category() {
        let yaml = "
withbase:
    base_path: $MODELS/comfy
    loras: models/loras
nobase:
    vae: $MODELS/vae
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec!["/weights/comfy/models/loras"]);
        assert_eq!(paths_for(&f, "vae"), vec!["/etc/$MODELS/vae"]);
    }

    #[test]
    fn an_absolute_category_path_discards_the_base_path() {
        let yaml = "
comfyui:
    base_path: /opt/ignored
    checkpoints: /mnt/big/checkpoints
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "checkpoints"), vec!["/mnt/big/checkpoints"]);
    }

    #[test]
    fn a_relative_path_without_a_base_resolves_against_the_yaml_folder() {
        // Not the ComfyUI root, and not the working directory.
        let yaml = "
comfyui:
    loras: rel/from/yaml
";
        let f = parse(yaml, &ctx(Posix, "/somewhere/else")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec!["/somewhere/else/rel/from/yaml"]);
    }

    #[test]
    fn an_empty_base_path_counts_as_no_base_path() {
        // Python treats "" as false, so the `if base_path:` branch is skipped.
        let yaml = "
comfyui:
    base_path: ''
    loras: models/loras
";
        let f = parse(yaml, &ctx(Posix, "/yamldir")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec!["/yamldir/models/loras"]);
    }

    #[test]
    fn trailing_whitespace_is_kept_in_the_folder_name() {
        // ComfyUI does not strip it, so a path with a trailing space is what it
        // looks for. The engine must agree, or it scans a different folder.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    vae: \"trailing_space_here \"
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "vae"), vec!["/opt/comfy/trailing_space_here "]);
    }

    #[test]
    fn legacy_category_names_are_renamed_and_the_original_is_kept() {
        let yaml = "
comfyui:
    base_path: /opt/comfy
    unet: models/unet
    clip: models/clip
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "diffusion_models"), vec!["/opt/comfy/models/unet"]);
        assert_eq!(paths_for(&f, "text_encoders"), vec!["/opt/comfy/models/clip"]);

        let unet = f.entries.iter().find(|e| e.raw_category == "unet").unwrap();
        assert_eq!(unet.category, "diffusion_models");
    }

    #[test]
    fn the_two_reserved_keys_never_become_categories() {
        let yaml = "
comfyui:
    base_path: /opt/comfy
    is_default: true
    loras: models/loras
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(f.entries.len(), 1, "base_path and is_default must not become folders");
        assert!(f.entries[0].is_default);
    }

    #[test]
    fn is_default_applies_to_every_category_in_its_section() {
        let yaml = "
a:
    base_path: /opt/a
    is_default: true
    loras: l
    vae: v
b:
    base_path: /opt/b
    loras: l
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        let defaults: Vec<bool> = f.entries.iter().map(|e| e.is_default).collect();
        assert_eq!(defaults, vec![true, true, false]);
    }

    #[test]
    fn an_unknown_category_is_accepted() {
        // ComfyUI creates the category with an empty extension set, which then
        // matches every file. The engine records it so the scan can see it.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    totally_made_up_category: models/whatever
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "totally_made_up_category"), vec!["/opt/comfy/models/whatever"]);
    }

    #[test]
    fn an_empty_section_is_skipped_without_complaint() {
        let yaml = "
emptysection:
comfyui:
    base_path: /opt/comfy
    loras: models/loras
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(f.entries.len(), 1);
        assert!(f.problems.is_empty(), "an empty section is legal, not a problem");
    }

    #[test]
    fn a_list_value_is_reported_because_comfyui_cannot_read_it() {
        // ComfyUI calls .split on the value and stops with an error. The engine
        // keeps going and tells the person which line breaks their install.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    loras:
        - one
        - two
    vae: models/vae
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(f.problems.len(), 1);
        assert!(f.problems[0].contains("loras"));
        assert!(f.problems[0].contains("ComfyUI stops with an error"));
        assert_eq!(paths_for(&f, "vae"), vec!["/opt/comfy/models/vae"], "the rest still parses");
    }

    #[test]
    fn invalid_yaml_is_a_parse_error_with_a_readable_message() {
        let err = parse("comfyui:\n  base_path: [unclosed\n", &ctx(Posix, "/etc")).unwrap_err();
        assert_eq!(err.code, ErrorCode::ParseError);
        assert!(err.message.contains("not valid YAML"));
    }

    #[test]
    fn an_empty_file_produces_no_entries_and_no_error() {
        let f = parse("", &ctx(Posix, "/etc")).unwrap();
        assert!(f.entries.is_empty());
        assert!(f.problems.is_empty());
    }

    #[test]
    fn a_comment_only_file_produces_no_entries() {
        let f = parse("# nothing here\n", &ctx(Posix, "/etc")).unwrap();
        assert!(f.entries.is_empty());
    }

    // --- Windows rules, exercised from Linux ------------------------------

    #[test]
    fn windows_paths_resolve_with_windows_rules() {
        let yaml = "
comfyui:
    base_path: C:\\ComfyUI
    checkpoints: models\\checkpoints
    loras: |
        models\\loras
        D:\\shared\\loras
";
        let f = parse(yaml, &ctx(Windows, r"C:\ComfyUI")).unwrap();
        assert_eq!(paths_for(&f, "checkpoints"), vec![r"C:\ComfyUI\models\checkpoints"]);
        assert_eq!(
            paths_for(&f, "loras"),
            vec![r"C:\ComfyUI\models\loras", r"D:\shared\loras"],
            "a path on another drive replaces the base path"
        );
    }

    #[test]
    fn a_rooted_windows_category_path_keeps_the_base_drive() {
        // os.path.join("D:\\base", "\\models") is "D:\\models", not
        // "D:\\base\\models". Getting this wrong points the scan at a folder
        // that does not exist.
        let yaml = "
comfyui:
    base_path: D:\\base
    loras: \\models\\loras
";
        let f = parse(yaml, &ctx(Windows, r"C:\etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec![r"D:\models\loras"]);
    }

    #[test]
    fn windows_user_profile_expands_on_base_path() {
        let yaml = "
comfyui:
    base_path: ~\\ComfyUI
    loras: models\\loras
";
        let f = parse(yaml, &ctx(Windows, r"C:\etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec![r"C:\Users\alex\ComfyUI\models\loras"]);
    }

    #[test]
    fn a_windows_percent_variable_expands_on_base_path_only() {
        let yaml = "
withbase:
    base_path: '%MODELS%\\comfy'
    loras: models\\loras
nobase:
    vae: '%MODELS%\\vae'
";
        let f = parse(yaml, &ctx(Windows, r"C:\etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec![r"D:\weights\comfy\models\loras"]);
        assert_eq!(paths_for(&f, "vae"), vec![r"C:\etc\%MODELS%\vae"]);
    }

    #[test]
    fn reading_a_real_file_marks_which_folders_exist() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real_models");
        std::fs::create_dir_all(&real).unwrap();

        let yaml_path = d.path().join("extra_model_paths.yaml");
        std::fs::write(
            &yaml_path,
            format!(
                "comfyui:\n    base_path: {}\n    loras: real_models\n    vae: missing_models\n",
                d.path().display()
            ),
        )
        .unwrap();

        let f = load_file(&yaml_path).unwrap();
        assert_eq!(f.path, yaml_path);
        let loras = f.entries.iter().find(|e| e.category == "loras").unwrap();
        let vae = f.entries.iter().find(|e| e.category == "vae").unwrap();
        assert!(loras.exists, "an existing folder must be marked as present");
        assert!(!vae.exists, "a missing folder must be marked as absent");
    }

    #[test]
    fn a_category_that_is_not_a_safe_folder_name_is_refused_and_reported() {
        // A category names a folder inside the vault. ComfyUI accepts any key
        // here, and this file is edited by hand and written by launchers and
        // node packs, so a key like "..\\..\\x" or a pasted Windows path
        // arrives as a folder name and gets joined onto the vault root.
        // Single-quoted in the YAML, because a double-quoted scalar processes
        // backslash escapes and would turn "a\\b" into a backspace character
        // rather than the two characters being tested.
        for hostile in [
            "../../ESCAPED",
            "..",
            ".",
            "a/b",
            "a\\b",
            "/tmp/ANYWHERE",
            "C:\\Users\\Public",
            "CON",
            "trailing ",
        ] {
            let yaml = format!("pack:\n    base_path: /opt/comfy\n    '{hostile}': models/x\n");
            let f = parse(&yaml, &ctx(Posix, "/etc")).unwrap();

            assert!(
                f.entries.is_empty(),
                "{hostile:?} became a model folder: {:?}",
                f.entries.first().map(|e| &e.category)
            );
            assert_eq!(f.problems.len(), 1, "{hostile:?} was dropped without telling anyone");
            assert!(
                f.problems[0].contains(hostile),
                "the person cannot fix what the message does not name: {}",
                f.problems[0]
            );
        }
    }

    #[test]
    fn a_bad_category_does_not_take_the_good_ones_with_it() {
        // One wrong line in a hand-edited file must not lose the rest.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    loras: models/loras
    \"../escape\": models/x
    checkpoints: models/checkpoints
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(paths_for(&f, "loras"), vec!["/opt/comfy/models/loras"]);
        assert_eq!(paths_for(&f, "checkpoints"), vec!["/opt/comfy/models/checkpoints"]);
        assert_eq!(f.entries.len(), 2);
        assert_eq!(f.problems.len(), 1);
    }

    #[test]
    fn an_ordinary_category_is_still_accepted() {
        // The control, so the test above cannot pass by refusing everything.
        let yaml = "
comfyui:
    base_path: /opt/comfy
    loras: models/loras
    diffusion_models: models/unet
    some_pack_category: models/pack
";
        let f = parse(yaml, &ctx(Posix, "/etc")).unwrap();
        assert_eq!(f.entries.len(), 3);
        assert!(f.problems.is_empty());
    }

    #[test]
    fn reading_a_missing_file_is_a_clear_error() {
        let d = tempfile::tempdir().unwrap();
        let err = load_file(&d.path().join("nope.yaml")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }
}
