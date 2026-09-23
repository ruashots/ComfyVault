//! Settings the person can change, and the defaults the engine ships with.
//!
//! # There is no Civitai key here, on purpose
//!
//! Looking a model up by hash needs no key. That was checked against the live
//! service: the same request unauthenticated, with a bogus bearer token, and
//! with a token on the query string all return the same answer, byte for byte.
//! Civitai's gate is on *downloading*, and this version does not download.
//!
//! So a key field would store a credential for a feature nobody can reach. It
//! would sit at rest in the vault database, and this product tells the person
//! to carry that vault on a portable drive they might lend, sell, or back up
//! somewhere shared. That is a liability with nothing on the other side of it.
//!
//! Whoever adds downloading adds the key then, and puts it in the operating
//! system's credential store, where it is bound to the machine and the
//! account. Not in here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The default weight extensions a scan takes.
///
/// The first seven are the product requirement. The last three come from
/// ComfyUI's own `supported_pt_extensions`, which treats `.pt2`, `.sft` and
/// `.pkl` as model weights. Leaving them out would let real duplicates stay on
/// disk, so they are included and the whole list is a setting.
pub const DEFAULT_EXTENSIONS: [&str; 10] = [
    ".safetensors",
    ".ckpt",
    ".pt",
    ".pth",
    ".bin",
    ".gguf",
    ".onnx",
    ".pt2",
    ".sft",
    ".pkl",
];

/// Files at or below this size are ignored. One megabyte.
pub const DEFAULT_MIN_FILE_SIZE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub metadata_lookups_enabled: bool,
    pub hash_cache_enabled: bool,
    pub scan_extensions: Vec<String>,
    pub min_file_size_bytes: u64,
    pub follow_extra_model_paths: bool,
    pub scan_output_model_dirs: bool,
    /// Where the Hugging Face libraries keep their downloaded models.
    ///
    /// `None` means work it out from the environment, which is what a person
    /// wants by default. An explicit list covers a cache moved to another
    /// drive, and an empty list means do not look at all.
    ///
    /// It is a setting rather than something read from the environment deep
    /// inside a scan, because a scan's result must not depend on machine state
    /// nobody can see. Reading the environment mid-scan also meant the tests
    /// walked whatever cache the machine really had.
    ///
    /// The name is spelled out because `rename_all = "camelCase"` reads
    /// "huggingface" as one word and would put `huggingfaceCacheDirs` on the
    /// wire. The product writes it `huggingFaceCacheDirs`.
    #[serde(default, rename = "huggingFaceCacheDirs")]
    pub huggingface_cache_dirs: Option<Vec<PathBuf>>,
    /// Read a duplicate's bytes again, immediately before deleting it, and
    /// compare them against the copy being kept.
    ///
    /// On by default. Deleting is the one thing this app does that cannot be
    /// undone, and without this the proof that two files are identical is a
    /// hash from an earlier scan, which may itself have come from a cache row
    /// rather than from the file. A drive with coarse timestamps, which is
    /// what external model drives often have, can hide a difference from the
    /// size and time alone.
    ///
    /// Turning it off makes a consolidation faster and makes the delete a
    /// matter of trust rather than proof.
    #[serde(default = "default_true")]
    pub verify_before_delete: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            metadata_lookups_enabled: true,
            hash_cache_enabled: true,
            scan_extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            min_file_size_bytes: DEFAULT_MIN_FILE_SIZE,
            follow_extra_model_paths: true,
            scan_output_model_dirs: true,
            huggingface_cache_dirs: None,
            verify_before_delete: true,
        }
    }
}

impl Settings {
    /// Does this file name carry an extension the scan takes?
    pub fn matches_extension(&self, file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        self.scan_extensions
            .iter()
            .any(|e| lower.ends_with(&e.to_lowercase()))
    }

    /// Applies a partial update. Absent fields are left alone.
    ///
    /// A key of `"***"` means the interface sent back what it was shown, so the
    /// stored key is kept. An empty string clears it.
    pub fn apply_patch(&mut self, patch: &SettingsPatch) {
        if let Some(v) = patch.metadata_lookups_enabled {
            self.metadata_lookups_enabled = v;
        }
        if let Some(v) = patch.hash_cache_enabled {
            self.hash_cache_enabled = v;
        }
        if let Some(v) = &patch.scan_extensions {
            self.scan_extensions = normalize_extensions(v);
        }
        if let Some(v) = patch.min_file_size_bytes {
            self.min_file_size_bytes = v;
        }
        if let Some(v) = patch.follow_extra_model_paths {
            self.follow_extra_model_paths = v;
        }
        if let Some(v) = patch.scan_output_model_dirs {
            self.scan_output_model_dirs = v;
        }
        if let Some(v) = &patch.huggingface_cache_dirs {
            self.huggingface_cache_dirs = Some(v.clone());
        }
        if let Some(v) = patch.verify_before_delete {
            self.verify_before_delete = v;
        }
    }

    /// The folders to count Hugging Face's cached models in.
    ///
    /// Falls back to the usual places only when nothing was set.
    pub fn resolved_huggingface_dirs(&self) -> Vec<PathBuf> {
        match &self.huggingface_cache_dirs {
            Some(dirs) => dirs.iter().filter(|d| d.is_dir()).cloned().collect(),
            None => crate::scan::huggingface_cache_dirs(),
        }
    }
}

/// Puts every extension in the one form the matcher expects: lower case, with a
/// leading dot, no blanks, no repeats.
fn normalize_extensions(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in raw {
        let t = e.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        let with_dot = if t.starts_with('.') { t } else { format!(".{t}") };
        if !out.contains(&with_dot) {
            out.push(with_dot);
        }
    }
    out
}

/// A partial update. Every field is optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub metadata_lookups_enabled: Option<bool>,
    pub hash_cache_enabled: Option<bool>,
    pub scan_extensions: Option<Vec<String>>,
    pub min_file_size_bytes: Option<u64>,
    pub follow_extra_model_paths: Option<bool>,
    pub scan_output_model_dirs: Option<bool>,
    #[serde(rename = "huggingFaceCacheDirs")]
    pub huggingface_cache_dirs: Option<Vec<PathBuf>>,
    pub verify_before_delete: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_extension_list_covers_the_product_list_and_comfyuis_own() {
        let s = Settings::default();
        for required in [".safetensors", ".ckpt", ".pt", ".pth", ".bin", ".gguf", ".onnx"] {
            assert!(s.scan_extensions.iter().any(|e| e == required), "missing {required}");
        }
        for comfy_only in [".pt2", ".sft", ".pkl"] {
            assert!(s.scan_extensions.iter().any(|e| e == comfy_only), "missing {comfy_only}");
        }
    }

    #[test]
    fn extension_matching_ignores_case() {
        let s = Settings::default();
        assert!(s.matches_extension("model.safetensors"));
        assert!(s.matches_extension("MODEL.SAFETENSORS"));
        assert!(s.matches_extension("Model.SafeTensors"));
        assert!(s.matches_extension("a.b.c.ckpt"));
    }

    #[test]
    fn extension_matching_rejects_everything_else() {
        let s = Settings::default();
        assert!(!s.matches_extension("readme.txt"));
        assert!(!s.matches_extension("workflow.json"));
        assert!(!s.matches_extension("image.png"));
        assert!(!s.matches_extension("noextension"));
        // A name that merely contains the text is not a match.
        assert!(!s.matches_extension("safetensors_notes.md"));
    }

    #[test]
    fn a_custom_extension_list_is_normalized() {
        let mut s = Settings::default();
        s.apply_patch(&SettingsPatch {
            scan_extensions: Some(vec![
                "SAFETENSORS".into(),
                ".ckpt".into(),
                "  .GGUF  ".into(),
                "".into(),
                "ckpt".into(),
            ]),
            ..Default::default()
        });
        assert_eq!(s.scan_extensions, vec![".safetensors", ".ckpt", ".gguf"]);
        assert!(s.matches_extension("x.safetensors"));
        assert!(!s.matches_extension("x.bin"), "the list was replaced, not extended");
    }

    #[test]
    fn a_patch_sent_by_the_interface_reaches_every_field() {
        // The interface sends JSON, not a Rust value. A name serde does not
        // recognise is dropped without an error, so the setting silently
        // stays as it was. Every field is checked by the name on the wire.
        let patch: SettingsPatch = serde_json::from_str(
            r#"{
                "metadataLookupsEnabled": false,
                "hashCacheEnabled": false,
                "scanExtensions": [".safetensors"],
                "minFileSizeBytes": 7,
                "followExtraModelPaths": false,
                "scanOutputModelDirs": false,
                "huggingFaceCacheDirs": ["D:\\hf"],
                "verifyBeforeDelete": false
            }"#,
        )
        .unwrap();

        let mut s = Settings::default();
        s.apply_patch(&patch);

        assert_eq!(s.metadata_lookups_enabled, false);
        assert_eq!(s.hash_cache_enabled, false);
        assert_eq!(s.scan_extensions, vec![".safetensors".to_string()]);
        assert_eq!(s.min_file_size_bytes, 7);
        assert_eq!(s.follow_extra_model_paths, false);
        assert_eq!(s.scan_output_model_dirs, false);
        assert_eq!(
            s.huggingface_cache_dirs,
            Some(vec![PathBuf::from("D:\\hf")]),
            "the Hugging Face folders did not survive the journey through JSON"
        );
        assert_eq!(s.verify_before_delete, false);
    }

    #[test]
    fn a_patch_leaves_absent_fields_alone() {
        let mut s = Settings::default();
        s.metadata_lookups_enabled = false;
        s.min_file_size_bytes = 42;

        s.apply_patch(&SettingsPatch {
            hash_cache_enabled: Some(false),
            ..Default::default()
        });

        assert!(!s.hash_cache_enabled, "the named field changed");
        assert!(!s.metadata_lookups_enabled, "an absent field must not be reset");
        assert_eq!(s.min_file_size_bytes, 42);
    }

    #[test]
    fn the_settings_hold_no_credential_at_all() {
        // Not redacted: absent. A key field would store a credential for a
        // feature this version cannot reach, in a file the person is told to
        // carry on a portable drive.
        let json = serde_json::to_string(&Settings::default()).unwrap();
        for word in ["apiKey", "api_key", "civitai", "token", "secret", "password"] {
            assert!(
                !json.to_lowercase().contains(&word.to_lowercase()),
                "the settings carry {word}, which is a credential in a file people carry: {json}"
            );
        }
    }

    #[test]
    fn settings_round_trip_through_json_with_camel_case_names() {
        let s = Settings::default();
        let v = serde_json::to_value(&s).unwrap();

        // Every name, checked as a set. Naming three of them let
        // `huggingfaceCacheDirs` reach a shipped build, because the field the
        // test stepped over was the one serde had spelled its own way.
        let mut got: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            [
                "followExtraModelPaths",
                "hashCacheEnabled",
                "huggingFaceCacheDirs",
                "metadataLookupsEnabled",
                "minFileSizeBytes",
                "scanExtensions",
                "scanOutputModelDirs",
                "verifyBeforeDelete",
            ]
        );

        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }
}
