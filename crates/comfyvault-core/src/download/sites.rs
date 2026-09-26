//! Asking Hugging Face and Civitai what an address names, and whether this
//! person may download it.
//!
//! Every answer from a site is data, not instructions. A file name the site
//! gives must be a plain model file name, and a download address the site
//! gives must be on the site itself, or it is not used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::address::{encode_segment, ModelAddress};
use super::http::{unreachable_site, Reply, Request, Web};
use crate::error::{Result, VaultError};

/// Where the two sites are. Tests point both at a server on this computer.
#[derive(Debug, Clone)]
pub struct Sites {
    pub hugging_face: String,
    pub civitai: String,
}

impl Default for Sites {
    fn default() -> Self {
        Self {
            hugging_face: "https://huggingface.co".to_string(),
            civitai: "https://civitai.com".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Host {
    HuggingFace,
    Civitai,
}

impl Host {
    pub fn name(self) -> &'static str {
        match self {
            Host::HuggingFace => "Hugging Face",
            Host::Civitai => "Civitai",
        }
    }
}

/// A Hugging Face repository, for "Open the model's page".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HfPage {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionChoice {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChoice {
    pub id: u64,
    pub name: String,
    pub size_bytes: u64,
    /// What Civitai says about it, for example "pruned fp16 SafeTensor".
    pub detail: String,
}

/// Why a site will not hand this person the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefusalKind {
    BadAddress,
    HfRepoNotFile,
    TokenMissing,
    TokenRejected,
    NoAccess,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub kind: RefusalKind,
    pub host: Option<Host>,
    /// The site's own words, exactly as it sent them.
    pub service_message: Option<String>,
    pub page: Option<HfPage>,
}

/// One file a site will hand over, as the site describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub host: Host,
    pub title: String,
    pub subtitle: String,
    pub versions: Vec<VersionChoice>,
    pub version_id: Option<u64>,
    pub files: Vec<FileChoice>,
    pub file_id: Option<u64>,
    pub file_name: String,
    /// Civitai gives the size in whole kilobytes, so for Civitai this is
    /// close, not exact. The transfer uses the exact size the storage sends.
    pub size_bytes: u64,
    /// Uppercase. `None` for a small Hugging Face file stored without LFS.
    pub sha256: Option<String>,
    pub suggested_category: Option<String>,
    pub suggested_because: Option<String>,
    pub page: Option<HfPage>,
    pub model_id: Option<u64>,
    /// The site's own download address. Never a signed storage address.
    pub fetch_url: String,
}

/// What reading an address found.
#[derive(Debug, Clone)]
pub enum Reading {
    File(RemoteFile),
    Refused(Refusal),
}

/// Asks the site about an address, with this person's token when there is
/// one, and checks that the site would hand the file over.
pub fn read(
    web: &dyn Web,
    sites: &Sites,
    address: &ModelAddress,
    version_id: Option<u64>,
    file_id: Option<u64>,
    token: Option<&str>,
    known_categories: &[String],
    is_model_name: &dyn Fn(&str) -> bool,
) -> Result<Reading> {
    let found = match address {
        ModelAddress::HuggingFace { owner, repo, revision, path } => {
            hugging_face(web, sites, owner, repo, revision, path, token, known_categories, is_model_name)?
        }
        ModelAddress::Civitai { model_id, version_id: v } => civitai(
            web,
            sites,
            *model_id,
            version_id.or(*v),
            file_id,
            token,
            is_model_name,
        )?,
    };
    let Reading::File(file) = found else { return Ok(found) };
    match check_access(web, &file, token)? {
        Some(refusal) => Ok(Reading::Refused(refusal)),
        None => Ok(Reading::File(file)),
    }
}

/// Asks for the file itself without following the redirect, and reads the
/// answer. A refusal shows here, before anything is downloaded.
pub fn check_access(web: &dyn Web, file: &RemoteFile, token: Option<&str>) -> Result<Option<Refusal>> {
    let req = match file.host {
        Host::HuggingFace => Request::head(&file.fetch_url),
        // Civitai's download address answers a plain request with its
        // redirect, and nothing is transferred until that is followed.
        Host::Civitai => Request::get(&file.fetch_url),
    }
    .bearer(token);
    let reply = web.send(&req, None)?;
    if reply.is_redirect() || (200..300).contains(&reply.status) {
        return Ok(None);
    }
    Ok(Some(refusal_from(file.host, reply, token.is_some(), file.page.clone())?))
}

/// Turns a site's refusal into the kind the interface explains, keeping the
/// site's own words.
pub fn refusal_from(host: Host, reply: Reply, had_token: bool, page: Option<HfPage>) -> Result<Refusal> {
    let status = reply.status;
    let code = reply.header("x-error-code").map(str::to_string);
    let header_message = reply.header("x-error-message").map(str::to_string);
    let body = reply.text().unwrap_or_default();
    let service_message = header_message.or_else(|| message_in(&body));

    let not_found = matches!(
        code.as_deref(),
        Some("RepoNotFound") | Some("EntryNotFound") | Some("RevisionNotFound")
    ) || status == 404;
    let kind = if not_found {
        RefusalKind::NotFound
    } else {
        match (status, had_token) {
            (401, false) | (403, false) => RefusalKind::TokenMissing,
            (401, true) => RefusalKind::TokenRejected,
            (403, true) => RefusalKind::NoAccess,
            _ => {
                return Err(VaultError::new(
                    crate::ErrorCode::NetworkUnavailable,
                    format!("{} answered in a way ComfyVault does not know, so nothing was downloaded.", host.name()),
                )
                .with_detail(format!("status {status}: {}", service_message.unwrap_or_default())))
            }
        }
    };
    Ok(Refusal { kind, host: Some(host), service_message, page })
}

/// The human message in a site's answer, as the site wrote it.
fn message_in(body: &str) -> Option<String> {
    let text = body.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        for key in ["message", "error"] {
            if let Some(s) = v.get(key).and_then(Value::as_str) {
                return Some(s.to_string());
            }
        }
    }
    // Not JSON: the text itself, kept short, and only if it is not a page.
    (!text.starts_with('<')).then(|| text.chars().take(500).collect())
}

#[allow(clippy::too_many_arguments)]
fn hugging_face(
    web: &dyn Web,
    sites: &Sites,
    owner: &str,
    repo: &str,
    revision: &str,
    path: &str,
    token: Option<&str>,
    known_categories: &[String],
    is_model_name: &dyn Fn(&str) -> bool,
) -> Result<Reading> {
    let page = HfPage { owner: owner.to_string(), repo: repo.to_string() };
    let file_name = path.rsplit('/').next().unwrap_or(path).to_string();
    if crate::paths::validate_file_name(&file_name).is_err() || !is_model_name(&file_name) {
        return Err(not_a_model(&file_name));
    }
    let encoded_path: Vec<String> = path.split('/').map(encode_segment).collect();
    let fetch_url = format!(
        "{}/{owner}/{repo}/resolve/{}/{}",
        sites.hugging_face,
        encode_segment(revision),
        encoded_path.join("/")
    );

    // The access question first: a gated or missing file stops here.
    let head = web.send(&Request::head(&fetch_url).bearer(token), None)?;
    if !(head.is_redirect() || (200..300).contains(&head.status)) {
        return Ok(Reading::Refused(refusal_from(Host::HuggingFace, head, token.is_some(), Some(page))?));
    }
    let linked_size = head.header("x-linked-size").and_then(|s| s.parse::<u64>().ok());
    let linked_sha = head.header("x-linked-etag").and_then(sha_in);
    let plain_size = head.header("content-length").and_then(|s| s.parse::<u64>().ok());

    // `paths-info` is where the site states the size and, for a file kept
    // in LFS, its SHA-256.
    let info_url = format!(
        "{}/api/models/{owner}/{repo}/paths-info/{}",
        sites.hugging_face,
        encode_segment(revision)
    );
    let form = format!("paths={}", encode_segment(path));
    let mut info_req = Request::get(&info_url).bearer(token);
    info_req.method = super::http::Method::PostForm;
    let info = web.send(&info_req, Some(&form))?;
    let (mut size, mut sha) = (None, None);
    if (200..300).contains(&info.status) {
        let body = info.text()?;
        if let Some(entry) = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .and_then(|a| a.into_iter().find(|e| e.get("path").and_then(Value::as_str) == Some(path)))
        {
            size = entry.get("size").and_then(Value::as_u64);
            sha = entry.get("lfs").and_then(|l| l.get("oid")).and_then(Value::as_str).and_then(sha_in);
        }
    }
    let size_bytes = size.or(linked_size).or(plain_size).ok_or_else(|| {
        VaultError::new(
            crate::ErrorCode::NetworkUnavailable,
            "Hugging Face did not say how big this file is, so it was not downloaded.",
        )
    })?;

    let (suggested_category, suggested_because) = match hf_category(path, known_categories) {
        Some(c) => (Some(c.clone()), Some(format!("Hugging Face keeps it in a folder called {c}"))),
        None => (None, None),
    };
    Ok(Reading::File(RemoteFile {
        host: Host::HuggingFace,
        title: file_name.clone(),
        subtitle: format!("{owner}/{repo}"),
        versions: Vec::new(),
        version_id: None,
        files: Vec::new(),
        file_id: None,
        file_name,
        size_bytes,
        sha256: sha.or(linked_sha),
        suggested_category,
        suggested_because,
        page: Some(page),
        model_id: None,
        fetch_url,
    }))
}

/// The deepest folder in a repository path that names a category.
fn hf_category(path: &str, known: &[String]) -> Option<String> {
    let folders: Vec<&str> = path.split('/').collect();
    folders[..folders.len().saturating_sub(1)].iter().rev().find_map(|f| {
        let name = crate::install::extra_paths::map_legacy(f);
        let listed = super::folders::built_in_categories().any(|c| c == name) || known.iter().any(|k| k == name);
        listed.then(|| name.to_string())
    })
}

/// A SHA-256 as the site writes it, in quotes or not, as uppercase hex.
fn sha_in(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("W/").trim_matches('"');
    (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_uppercase())
}

fn not_a_model(name: &str) -> VaultError {
    VaultError::invalid(format!(
        "{name} is not a model file ComfyVault can link, so it was not read. Choose a .safetensors or another model file."
    ))
}

/// Civitai's kind of model, as a vault folder.
pub fn civitai_category(kind: &str) -> Option<&'static str> {
    match kind {
        "Checkpoint" => Some("checkpoints"),
        "LORA" | "LoCon" | "DoRA" => Some("loras"),
        "TextualInversion" => Some("embeddings"),
        "VAE" => Some("vae"),
        "Controlnet" => Some("controlnet"),
        "Upscaler" => Some("upscale_models"),
        _ => None,
    }
}

fn civitai(
    web: &dyn Web,
    sites: &Sites,
    model_id: Option<u64>,
    version_id: Option<u64>,
    file_id: Option<u64>,
    token: Option<&str>,
    is_model_name: &dyn Fn(&str) -> bool,
) -> Result<Reading> {
    let base = &sites.civitai;
    let api = |path: String| -> Result<std::result::Result<Value, Refusal>> {
        let reply = web.send(&Request::get(format!("{base}{path}")).bearer(token), None)?;
        if !(200..300).contains(&reply.status) {
            return Ok(Err(refusal_from(Host::Civitai, reply, token.is_some(), None)?));
        }
        let body = reply.text()?;
        serde_json::from_str(&body).map(Ok).map_err(|_| {
            VaultError::new(crate::ErrorCode::NetworkUnavailable, "Civitai sent an answer ComfyVault could not read.")
        })
    };

    // A version alone says which model it belongs to.
    let model_id = match (model_id, version_id) {
        (Some(m), _) => m,
        (None, Some(v)) => match api(format!("/api/v1/model-versions/{v}"))? {
            Ok(ver) => ver.get("modelId").and_then(Value::as_u64).ok_or_else(|| {
                VaultError::new(crate::ErrorCode::NetworkUnavailable, "Civitai did not say which model this version belongs to.")
            })?,
            Err(r) => return Ok(Reading::Refused(r)),
        },
        (None, None) => return Err(VaultError::invalid("That Civitai address names no model.")),
    };
    let model = match api(format!("/api/v1/models/{model_id}"))? {
        Ok(m) => m,
        Err(r) => return Ok(Reading::Refused(r)),
    };

    let name = model.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let kind = model.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let all_versions: Vec<Value> = model.get("modelVersions").and_then(Value::as_array).cloned().unwrap_or_default();
    let versions: Vec<VersionChoice> = all_versions
        .iter()
        .filter_map(|v| {
            Some(VersionChoice {
                id: v.get("id")?.as_u64()?,
                name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            })
        })
        .collect();
    let chosen = match version_id {
        Some(v) => all_versions.iter().find(|x| x.get("id").and_then(Value::as_u64) == Some(v)),
        None => all_versions.first(),
    }
    .cloned();
    let Some(version) = chosen else {
        return Ok(Reading::Refused(Refusal {
            kind: RefusalKind::NotFound,
            host: Some(Host::Civitai),
            service_message: None,
            page: None,
        }));
    };
    let version_id = version.get("id").and_then(Value::as_u64);

    // Only files that can be linked as a model. A training set or a config
    // file is offered by Civitai too, and is not one.
    let mut files: Vec<(FileChoice, bool, Option<String>, String)> = version
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?.to_string();
            if crate::paths::validate_file_name(&name).is_err() || !is_model_name(&name) {
                return None;
            }
            let meta = f.get("metadata");
            let detail = ["size", "fp", "format"]
                .iter()
                .filter_map(|k| meta.and_then(|m| m.get(*k)).and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let size_bytes = (f.get("sizeKB").and_then(Value::as_f64).unwrap_or(0.0) * 1024.0).round() as u64;
            let sha = f.get("hashes").and_then(|h| h.get("SHA256")).and_then(Value::as_str).and_then(sha_in);
            let url = f.get("downloadUrl").and_then(Value::as_str).unwrap_or("").to_string();
            let primary = f.get("primary").and_then(Value::as_bool).unwrap_or(false);
            Some((FileChoice { id: f.get("id")?.as_u64()?, name, size_bytes, detail }, primary, sha, url))
        })
        .collect();
    files.sort_by_key(|(_, primary, _, _)| !primary);
    let pick = match file_id {
        Some(id) => files.iter().find(|(f, _, _, _)| f.id == id),
        None => files.first(),
    };
    let Some((file, _, sha, url)) = pick.cloned() else {
        return Err(VaultError::invalid(
            "That version on Civitai has no model file ComfyVault can link, so nothing was read.",
        ));
    };

    // The download address comes from the site's answer, so it is held to
    // the site's own download path before it is ever asked for.
    let expected = format!("{base}/api/download/models/");
    if !url.starts_with(&expected) {
        return Err(VaultError::new(
            crate::ErrorCode::NetworkUnavailable,
            "Civitai gave a download address somewhere else, so nothing was downloaded.",
        )
        .with_detail(super::http::without_query(&url).to_string()));
    }

    let category = civitai_category(&kind);
    Ok(Reading::File(RemoteFile {
        host: Host::Civitai,
        title: name,
        subtitle: format!("{base}/models/{model_id}"),
        versions,
        version_id,
        files: files.into_iter().map(|(f, _, _, _)| f).collect(),
        file_id: Some(file.id),
        file_name: file.name.clone(),
        size_bytes: file.size_bytes,
        sha256: sha,
        suggested_category: category.map(str::to_string),
        suggested_because: category.map(|_| format!("Civitai calls it a {kind}")),
        page: None,
        model_id: Some(model_id),
        fetch_url: url,
    }))
}

/// A Hugging Face model's page, from two names that must be plain Hugging
/// Face names. The window names a page this way, never by its address.
pub fn model_page_url(owner: &str, repo: &str) -> Result<String> {
    if !super::address::is_hf_name(owner) || !super::address::is_hf_name(repo) {
        return Err(VaultError::invalid("That is not a Hugging Face model."));
    }
    Ok(format!("https://huggingface.co/{owner}/{repo}"))
}

/// Asks a site whether a token is good, and for whose account.
pub fn check_token(web: &dyn Web, sites: &Sites, host: Host, token: &str) -> Result<std::result::Result<Option<String>, String>> {
    let url = match host {
        Host::HuggingFace => format!("{}/api/whoami-v2", sites.hugging_face),
        Host::Civitai => format!("{}/api/v1/me", sites.civitai),
    };
    let reply = web.send(&Request::get(url).bearer(Some(token)), None)?;
    let status = reply.status;
    let body = reply.text().unwrap_or_default();
    if (200..300).contains(&status) {
        let account = match host {
            Host::HuggingFace => serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_string)),
            Host::Civitai => None,
        };
        return Ok(Ok(account));
    }
    if status == 401 || status == 403 {
        return Ok(Err(message_in(&body).unwrap_or_else(|| format!("{} refused it.", host.name()))));
    }
    Err(unreachable_site(&format!("status {status}")))
}

#[cfg(test)]
mod tests;
