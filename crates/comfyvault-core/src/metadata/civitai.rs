//! The Civitai client.
//!
//! Two endpoints, both keyed by the SHA-256 of the whole file:
//!
//! * `GET /api/v1/model-versions/by-hash/{hash}` answers about one file.
//! * `POST /api/v1/model-versions/by-hash` with a JSON array answers about up
//!   to a hundred, which turns three hundred lookups into three requests.
//!
//! The batch form accepts SHA-256 and nothing else. That is the reason the
//! whole engine identifies files with SHA-256 and computes no second hash.
//!
//! Three behaviors this client has to absorb, all of them measured against the
//! live service rather than read from its documentation:
//!
//! * The batch answer comes back in an arbitrary order, so each result is
//!   matched to its input by the hash inside the response, never by position.
//! * A hash that matches nothing is simply absent from the batch answer, so
//!   every hash that does not come back is recorded as not found.
//! * One file can match several model versions, because people re-upload
//!   identical files under their own pages. The lowest version identifier is
//!   the original upload, so that one is kept and the record is marked
//!   ambiguous.

use serde::Deserialize;

use super::http::HttpTransport;
use super::{MetadataSource, MetadataSourceName, ModelMetadata};
use crate::error::{ErrorCode, Result, VaultError};
use crate::time_util::Timestamp;

pub const DEFAULT_BASE_URL: &str = "https://civitai.com";

/// The most hashes the batch endpoint accepts. Sending 101 answers 400.
pub const BATCH_LIMIT: usize = 100;

pub struct CivitaiClient<'a> {
    transport: &'a dyn HttpTransport,
    base_url: String,
    api_key: Option<String>,
}

impl<'a> CivitaiClient<'a> {
    pub fn new(transport: &'a dyn HttpTransport, api_key: Option<String>) -> Self {
        Self {
            transport,
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key,
        }
    }

    /// Points the client at another address. Tests use it to reach a local
    /// server, so the real request building is exercised.
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base_url = base.trim_end_matches('/').to_string();
        self
    }

    fn headers(&self) -> Vec<(String, String)> {
        match &self.api_key {
            Some(k) => vec![("Authorization".to_string(), format!("Bearer {k}"))],
            None => Vec::new(),
        }
    }
}

impl MetadataSource for CivitaiClient<'_> {
    fn fetch_one(&self, sha256: &str) -> Result<ModelMetadata> {
        let url = format!("{}/api/v1/model-versions/by-hash/{}", self.base_url, sha256);
        let resp = self.transport.get(&url, &self.headers())?;

        match resp.status {
            // The service answers the same 404 for an unknown file and for
            // nonsense, so the hash shape is checked before the request, not
            // inferred from the answer.
            404 => Ok(ModelMetadata::not_found(sha256)),
            429 => Err(VaultError::new(
                ErrorCode::NetworkUnavailable,
                "Civitai is asking the app to slow down. Try the details again in a moment.",
            )),
            s if !(200..300).contains(&s) => Err(VaultError::new(
                ErrorCode::NetworkUnavailable,
                "Civitai could not answer right now, so model details are not available.",
            )
            .with_detail(format!("HTTP {s}"))),
            _ => {
                let v: ApiVersion = serde_json::from_str(&resp.body).map_err(|e| {
                    VaultError::new(
                        ErrorCode::NetworkUnavailable,
                        "Civitai sent something this app could not read, so model details are not available.",
                    )
                    .with_detail(e.to_string())
                })?;
                Ok(convert(&v, sha256, false))
            }
        }
    }

    fn fetch_many(&self, hashes: &[String]) -> Result<Vec<ModelMetadata>> {
        let mut out: Vec<ModelMetadata> = Vec::new();

        for chunk in hashes.chunks(BATCH_LIMIT) {
            let url = format!("{}/api/v1/model-versions/by-hash", self.base_url);
            let body = serde_json::to_string(chunk)?;
            let resp = self.transport.post_json(&url, &self.headers(), &body)?;

            if resp.status == 429 {
                return Err(VaultError::new(
                    ErrorCode::NetworkUnavailable,
                    "Civitai is asking the app to slow down. Try the details again in a moment.",
                ));
            }
            if !(200..300).contains(&resp.status) {
                // The batch endpoint is not in Civitai's public documentation,
                // so it can change without notice. Falling back one at a time
                // keeps identification working rather than losing the feature.
                for h in chunk {
                    out.push(self.fetch_one(h)?);
                }
                continue;
            }

            let versions: Vec<ApiVersion> = serde_json::from_str(&resp.body).map_err(|e| {
                VaultError::new(
                    ErrorCode::NetworkUnavailable,
                    "Civitai sent something this app could not read, so model details are not available.",
                )
                .with_detail(e.to_string())
            })?;

            out.extend(match_batch(chunk, &versions));
        }
        Ok(out)
    }
}

/// Matches each requested hash to its answer.
///
/// Pure logic, so every trap in it is unit tested: the answer arrives in an
/// arbitrary order, misses are missing rather than null, and one hash can match
/// several versions.
pub fn match_batch(requested: &[String], versions: &[ApiVersion]) -> Vec<ModelMetadata> {
    requested
        .iter()
        .map(|want| {
            let mut matches: Vec<&ApiVersion> = versions
                .iter()
                .filter(|v| {
                    v.files.iter().any(|f| {
                        f.hashes
                            .sha256
                            .as_deref()
                            .map(|h| h.eq_ignore_ascii_case(want))
                            .unwrap_or(false)
                    })
                })
                .collect();

            if matches.is_empty() {
                return ModelMetadata::not_found(want);
            }
            let ambiguous = matches.len() > 1;
            // The lowest identifier is the original upload, which is also what
            // the single lookup returns. The two paths must agree.
            matches.sort_by_key(|v| v.id);
            convert(matches[0], want, ambiguous)
        })
        .collect()
}

fn convert(v: &ApiVersion, sha256: &str, ambiguous: bool) -> ModelMetadata {
    let file = v
        .files
        .iter()
        .find(|f| {
            f.hashes
                .sha256
                .as_deref()
                .map(|h| h.eq_ignore_ascii_case(sha256))
                .unwrap_or(false)
        })
        .or_else(|| v.files.iter().find(|f| f.primary.unwrap_or(false)))
        .or_else(|| v.files.first());

    ModelMetadata {
        sha256: sha256.to_uppercase(),
        source: MetadataSourceName::Civitai,
        fetched_at: Timestamp::now(),
        found: true,
        model_name: v.model.as_ref().and_then(|m| m.name.clone()),
        model_type: v.model.as_ref().and_then(|m| m.r#type.clone()),
        version_name: v.name.clone(),
        base_model: v.base_model.clone(),
        trigger_words: split_trigger_words(&v.trained_words),
        nsfw: v.model.as_ref().and_then(|m| m.nsfw).unwrap_or(false),
        nsfw_level: v.nsfw_level.unwrap_or(0),
        civitai_model_id: v.model_id,
        civitai_version_id: Some(v.id),
        page_url: v
            .model_id
            .map(|m| format!("{DEFAULT_BASE_URL}/models/{m}?modelVersionId={}", v.id)),
        download_url: file
            .and_then(|f| f.download_url.clone())
            .or_else(|| v.download_url.clone()),
        preview_image_urls: v.images.iter().filter_map(|i| i.url.clone()).collect(),
        ambiguous,
    }
}

/// Splits the trigger words into usable tokens.
///
/// The field is not a clean list. One element often holds several triggers
/// separated by commas, sometimes with a trailing comma and a space. Handing
/// that to the person unchanged shows one long string where there are three
/// separate words.
fn split_trigger_words(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in raw {
        for part in entry.split(',') {
            let t = part.trim();
            if !t.is_empty() && !out.iter().any(|e| e == t) {
                out.push(t.to_string());
            }
        }
    }
    out
}

// --- the shapes Civitai returns -------------------------------------------
// Only the fields the product uses. Everything is optional, because several
// fields are routinely null and the service adds new ones without notice.

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiVersion {
    pub id: u64,
    pub model_id: Option<u64>,
    pub name: Option<String>,
    pub base_model: Option<String>,
    #[serde(default)]
    pub trained_words: Vec<String>,
    pub nsfw_level: Option<u32>,
    pub model: Option<ApiModel>,
    #[serde(default)]
    pub files: Vec<ApiFile>,
    #[serde(default)]
    pub images: Vec<ApiImage>,
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiModel {
    pub name: Option<String>,
    pub r#type: Option<String>,
    pub nsfw: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiFile {
    #[serde(default)]
    pub hashes: ApiHashes,
    pub download_url: Option<String>,
    pub primary: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiHashes {
    #[serde(rename = "SHA256")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiImage {
    pub url: Option<String>,
}

#[cfg(test)]
mod tests;
