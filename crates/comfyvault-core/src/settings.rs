//! Settings the person can change, and the defaults the engine ships with.

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
    /// Never returned by `get_settings`, never written to a log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub civitai_api_key: Option<String>,
    pub hash_cache_enabled: bool,
    pub scan_extensions: Vec<String>,
    pub min_file_size_bytes: u64,
    pub follow_extra_model_paths: bool,
    pub scan_output_model_dirs: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            metadata_lookups_enabled: true,
            civitai_api_key: None,
            hash_cache_enabled: true,
            scan_extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            min_file_size_bytes: DEFAULT_MIN_FILE_SIZE,
            follow_extra_model_paths: true,
            scan_output_model_dirs: true,
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

    /// The shape sent to the interface, with the key replaced by a marker.
    ///
    /// The key is a credential. It never leaves the engine, so it can never be
    /// read out of a window, a log file, or a crash report.
    pub fn redacted(&self) -> Self {
        Self {
            civitai_api_key: self.civitai_api_key.as_ref().map(|_| "***".to_string()),
            ..self.clone()
        }
    }

    /// Applies a partial update. Absent fields are left alone.
    ///
    /// A key of `"***"` means the interface sent back what it was shown, so the
    /// stored key is kept. An empty string clears it.
    pub fn apply_patch(&mut self, patch: &SettingsPatch) {
        if let Some(v) = patch.metadata_lookups_enabled {
            self.metadata_lookups_enabled = v;
        }
        if let Some(v) = &patch.civitai_api_key {
            match v.as_str() {
                "***" => {}
                "" => self.civitai_api_key = None,
                k => self.civitai_api_key = Some(k.to_string()),
            }
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
    pub civitai_api_key: Option<String>,
    pub hash_cache_enabled: Option<bool>,
    pub scan_extensions: Option<Vec<String>>,
    pub min_file_size_bytes: Option<u64>,
    pub follow_extra_model_paths: Option<bool>,
    pub scan_output_model_dirs: Option<bool>,
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
    fn the_api_key_never_appears_in_the_redacted_form() {
        let s = Settings {
            civitai_api_key: Some("secret-key-value".into()),
            ..Default::default()
        };
        let r = s.redacted();
        assert_eq!(r.civitai_api_key.as_deref(), Some("***"));

        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("secret-key-value"), "the key leaked into JSON");
    }

    #[test]
    fn sending_the_redaction_marker_back_keeps_the_stored_key() {
        // The interface shows "***" and sends the whole settings object back.
        // Treating that as the new key would destroy the real one.
        let mut s = Settings {
            civitai_api_key: Some("real-key".into()),
            ..Default::default()
        };
        s.apply_patch(&SettingsPatch {
            civitai_api_key: Some("***".into()),
            ..Default::default()
        });
        assert_eq!(s.civitai_api_key.as_deref(), Some("real-key"));
    }

    #[test]
    fn an_empty_key_clears_the_stored_key() {
        let mut s = Settings {
            civitai_api_key: Some("real-key".into()),
            ..Default::default()
        };
        s.apply_patch(&SettingsPatch {
            civitai_api_key: Some(String::new()),
            ..Default::default()
        });
        assert_eq!(s.civitai_api_key, None);
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
    fn settings_round_trip_through_json_with_camel_case_names() {
        let s = Settings::default();
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("metadataLookupsEnabled").is_some());
        assert!(v.get("minFileSizeBytes").is_some());
        assert!(v.get("scanOutputModelDirs").is_some());
        assert!(v.get("civitaiApiKey").is_none(), "an unset key must not appear at all");

        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }
}
