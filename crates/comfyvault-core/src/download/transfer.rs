//! Moving the bytes: one request to the site, one to its storage, into a
//! part file that survives a stop, a dropped line and a closed app.
//!
//! Each start reads the site's own address again. A signed storage address
//! expires, so an old one is never asked twice. A kept part is continued with
//! an HTTP `Range` request, and `If-Range` names the version it came from, so
//! a file that changed on the site is sent whole rather than stitched onto
//! the old part.
//!
//! Nothing here retries on its own. A refusal, an expired address or a dropped
//! line ends the attempt with what happened, and the person decides.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::http::{Reply, Request, Web};
use super::sites::{refusal_from, Host, RemoteFile};
use crate::progress::CancelToken;

/// Why a transfer stopped short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The line dropped, or went silent.
    Connection,
    /// The site refused: a token it wants, or no access.
    Refused,
    /// The signed storage address was no longer valid.
    Expired,
    /// The vault drive is full.
    NoSpace,
    /// The downloaded file is not the one the site named.
    Mismatch,
    /// Writing, reading or moving on this computer failed.
    Disk,
    /// The file on the site is not the one this download started from.
    ChangedOnSite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    /// One sentence for the person.
    pub message: String,
    /// The site's own words, when it said something.
    pub service_message: Option<String>,
    /// What the connection or the disk reported, for a details panel. Never
    /// an address.
    pub detail: Option<String>,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), service_message: None, detail: None }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every byte is in the part file. `etag` names the version it came from.
    Complete { total: u64, etag: Option<String> },
    /// The person stopped it. The part is kept.
    Stopped { total: Option<u64>, etag: Option<String> },
}

/// What the transfer reports as it goes: bytes in the part, and the whole.
pub type OnBytes<'a> = dyn FnMut(u64, Option<u64>) + 'a;

/// Downloads `file` into `part`, continuing from what `part` already holds.
///
/// `etag` is the storage's name for the version the kept part came from, or
/// `None` when nothing is kept. `version` receives the name of the version
/// now in the part as soon as bytes of it arrive, so a transfer that fails
/// part way can still be continued.
#[allow(clippy::too_many_arguments)]
pub fn run(
    web: &dyn Web,
    file: &RemoteFile,
    token: Option<&str>,
    part: &Path,
    etag: Option<&str>,
    version: &mut Option<String>,
    cancel: &CancelToken,
    on_bytes: &mut OnBytes<'_>,
) -> Result<Outcome, Failure> {
    let host = file.host.name();
    let limit = most_bytes(file);
    let mut have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if have > limit {
        truncate(part)?;
        have = 0;
    }
    let ranged = |req: Request, have: u64| match (have, etag) {
        (0, _) => req,
        (n, Some(tag)) => req.header("range", format!("bytes={n}-")).header("if-range", tag),
        // A part with no version name cannot be continued safely.
        (_, None) => req,
    };
    if have > 0 && etag.is_none() {
        truncate(part)?;
        have = 0;
    }

    // The site's own address, with the token, and never followed blindly.
    let first = web
        .send(&ranged(Request::get(&file.fetch_url).bearer(token), have), None)
        .map_err(|e| dropped(host, &e.message))?;
    let mut from_storage = false;
    let reply = if first.is_redirect() {
        let to = first
            .header("location")
            .map(|l| absolute(&file.fetch_url, l))
            .filter(|l| l.starts_with("https://") || l.starts_with("http://"))
            .ok_or_else(|| Failure::new(FailureKind::Connection, format!("{host} sent the download somewhere ComfyVault cannot follow.")))?;
        if !may_follow(file.host, &file.fetch_url, &to, file.trust_local_storage) {
            return Err(Failure::new(
                FailureKind::Connection,
                format!("{host} sent the download to an address ComfyVault does not trust, so nothing was downloaded."),
            )
            .with_detail(origin(&to).map(|o| o.host).unwrap_or_default()));
        }
        // The token goes along only to the site itself: Hugging Face sends a
        // file kept outside LFS to its own cache address, which a gated model
        // guards too. Storage carries its own signature, and gets no token.
        let same_site = origin(&to).is_some() && origin(&to) == origin(&file.fetch_url);
        let req = if same_site { Request::get(&to).bearer(token) } else { Request::get(&to) };
        let reply = web.send(&ranged(req, have), None)
            .map_err(|e| dropped(host, e.detail.as_deref().unwrap_or(&e.message)))?;
        from_storage = !same_site;
        reply
    } else {
        first
    };

    let (start, total) = match reply.status {
        200 => (0, reply.header("content-length").and_then(|v| v.parse::<u64>().ok())),
        206 => match content_range(reply.header("content-range")) {
            Some((s, t)) if s == have => (s, Some(t)),
            _ => {
                return Err(Failure::new(
                    FailureKind::Connection,
                    format!("{host} sent a different part of the file than the one asked for. The part already downloaded is kept."),
                ))
            }
        },
        416 => {
            return match content_range_total(reply.header("content-range")) {
                Some(t) if t == have => Ok(Outcome::Complete { total: t, etag: etag.map(str::to_string) }),
                _ => {
                    truncate(part)?;
                    Err(Failure::new(
                        FailureKind::ChangedOnSite,
                        format!("The file on {host} is not the one this download started from, so the part already downloaded was deleted."),
                    ))
                }
            }
        }
        // Storage refuses a signature that ran out, whatever code it picks.
        400 | 401 | 403 | 404 | 410 if from_storage => return Err(expired(host, reply)),
        401 | 403 | 404 => return Err(refused(file.host, reply, token.is_some())),
        s => {
            return Err(Failure::new(
                FailureKind::Connection,
                format!("{host} answered with an error ({s}) during the download. The part already downloaded is kept."),
            ))
        }
    };
    // A file is sent as it is. ComfyVault asks for no compression, and an
    // answer that is compressed anyway could unpack to any size.
    if reply.header("content-encoding").map(|e| !e.trim().eq_ignore_ascii_case("identity")).unwrap_or(false) {
        return Err(Failure::new(
            FailureKind::Connection,
            format!("{host} sent the file compressed, which ComfyVault does not accept. Nothing was written."),
        ));
    }
    // More than the site said the file is: refused before a byte is kept.
    if total.map(|t| t > limit).unwrap_or(false) {
        let _ = std::fs::remove_file(part);
        return Err(too_big(host));
    }
    let new_etag = reply.header("etag").map(str::to_string);
    *version = new_etag.clone();
    if start == 0 && have > 0 {
        // Sent whole: the storage ignored the range, or the file changed.
        truncate(part)?;
        have = 0;
    }
    let total = total.map(|t| if start > 0 { t } else { t + have });

    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(part)
        .map_err(|e| disk(&e, part))?;
    let mut body = reply.body;
    let mut buf = vec![0u8; 1 << 20];
    on_bytes(have, total);
    loop {
        if cancel.is_cancelled() {
            out.sync_all().map_err(|e| disk(&e, part))?;
            return Ok(Outcome::Stopped { total, etag: new_etag });
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = out.sync_all();
                return Err(dropped(host, &e.to_string()));
            }
        };
        // Held to the size the site gave, whatever the storage says.
        if have + n as u64 > limit {
            drop(out);
            let _ = std::fs::remove_file(part);
            return Err(too_big(host));
        }
        out.write_all(&buf[..n]).map_err(|e| disk(&e, part))?;
        have += n as u64;
        on_bytes(have, total);
    }
    out.sync_all().map_err(|e| disk(&e, part))?;
    if let Some(t) = total {
        if have != t {
            return Err(dropped(host, &format!("ended at {have} of {t} bytes")));
        }
    }
    // Hugging Face states the exact size, so anything short is not the file.
    if file.host == Host::HuggingFace && have != file.size_bytes {
        let _ = std::fs::remove_file(part);
        return Err(Failure::new(
            FailureKind::Mismatch,
            format!("The download was {have} bytes, and {host} said the file is {} bytes, so it was deleted. Nothing went into the vault and nothing was linked.", file.size_bytes),
        ));
    }
    Ok(Outcome::Complete { total: have, etag: new_etag })
}

/// The most bytes a download of this file may bring. Hugging Face states
/// sizes exactly. Civitai states whole kilobytes, so one more is allowed.
fn most_bytes(file: &RemoteFile) -> u64 {
    match file.host {
        Host::HuggingFace => file.size_bytes,
        Host::Civitai => file.size_bytes + 1024,
    }
}

fn too_big(host: &str) -> Failure {
    Failure::new(
        FailureKind::Mismatch,
        format!("The download was larger than the size {host} gave, so it was stopped and deleted. Nothing went into the vault and nothing was linked."),
    )
}

fn refused(host: Host, reply: Reply, had_token: bool) -> Failure {
    let refusal = refusal_from(host, reply, had_token, None).ok();
    Failure {
        kind: FailureKind::Refused,
        message: format!("{} refused the download part way.", host.name()),
        service_message: refusal.and_then(|r| r.service_message),
        detail: None,
    }
}

fn expired(host: &str, reply: Reply) -> Failure {
    let body = reply.text().unwrap_or_default();
    // Storage services answer in XML: `<Message>Request has expired</Message>`.
    let said = body
        .split_once("<Message>")
        .and_then(|(_, rest)| rest.split_once("</Message>"))
        .map(|(m, _)| m.trim().to_string())
        .filter(|m| !m.is_empty());
    Failure {
        kind: FailureKind::Expired,
        message: format!("The download address {host} gave is no longer valid. Continue reads the address again, and the part already downloaded is kept."),
        service_message: said,
        detail: None,
    }
}

fn dropped(host: &str, detail: &str) -> Failure {
    Failure::new(
        FailureKind::Connection,
        format!("The connection to {host} dropped. The part already downloaded is kept."),
    )
    .with_detail(detail)
}

fn disk(e: &std::io::Error, part: &Path) -> Failure {
    let full = matches!(e.raw_os_error(), Some(112) | Some(39) | Some(28)) || e.kind() == std::io::ErrorKind::StorageFull;
    if full {
        return Failure::new(
            FailureKind::NoSpace,
            "The vault's drive is full. Free some space, then continue. The part already downloaded is kept.",
        )
        .with_detail(e.to_string());
    }
    Failure::new(
        FailureKind::Disk,
        format!("Writing the download failed: {e}. The part already downloaded is kept at {}.", crate::paths::display_path(part)),
    )
}

fn truncate(part: &Path) -> Result<(), Failure> {
    match OpenOptions::new().write(true).truncate(true).open(part) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(disk(&e, part)),
    }
}

/// The scheme, host and port of an address. `None` for anything that is not
/// a plain `http` or `https` address, including one with a user name in it,
/// which a browser would read as a different host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

pub(crate) fn origin(url: &str) -> Option<Origin> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let port = authority[end + 1..].strip_prefix(':').map(|p| p.parse().ok()).unwrap_or(Some(default))?;
        (authority[..=end].to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (authority.to_string(), default),
        }
    };
    Some(Origin { scheme, host: host.to_ascii_lowercase(), port })
}

/// Whether a site's redirect may be followed.
///
/// Only over `https`, only to a named host, never to this computer or an
/// address on the network, and only to the site's own hosts or the storage
/// each site is known to use (checked on 2026-09-26: Hugging Face sends to
/// `*.hf.co`, Civitai to `b2.civitai.com` and to its own buckets on
/// `*.r2.cloudflarestorage.com`). Anything else is refused rather than asked.
///
/// `trust_local` lets a site on this computer send to this computer. Only a
/// test sets it; the real sites never get it.
pub(crate) fn may_follow(host: Host, site: &str, to: &str, trust_local: bool) -> bool {
    let (Some(site), Some(to)) = (origin(site), origin(to)) else { return false };
    if trust_local && site.host == "127.0.0.1" && to.host == "127.0.0.1" {
        return true;
    }
    if to.scheme != "https" {
        return false;
    }
    let h = to.host.as_str();
    let named = h.contains('.')
        && h != "localhost"
        && !h.ends_with(".localhost")
        && !h.starts_with('[')
        && !h.split('.').all(|p| p.bytes().all(|b| b.is_ascii_digit()));
    if !named {
        return false;
    }
    let under = |domain: &str| h == domain || h.ends_with(&format!(".{domain}"));
    match host {
        Host::HuggingFace => under("huggingface.co") || under("hf.co"),
        Host::Civitai => {
            under("civitai.com") || (h.starts_with("civitai-") && h.ends_with(".r2.cloudflarestorage.com"))
        }
    }
}

/// `bytes 100-199/1000` as (100, 1000).
fn content_range(v: Option<&str>) -> Option<(u64, u64)> {
    let rest = v?.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    Some((start.parse().ok()?, total.parse().ok()?))
}

/// `bytes */1000` as 1000.
fn content_range_total(v: Option<&str>) -> Option<u64> {
    v?.trim().strip_prefix("bytes */")?.parse().ok()
}

/// A redirect's target, made absolute against the address that sent it.
fn absolute(from: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    let scheme_end = from.find("://").map(|i| i + 3).unwrap_or(0);
    let origin_end = from[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(from.len());
    if location.starts_with('/') {
        format!("{}{location}", &from[..origin_end])
    } else {
        // Relative to the folder of `from`. Neither site does this today.
        let dir_end = from.rfind('/').filter(|i| *i >= origin_end).unwrap_or(origin_end);
        format!("{}/{location}", &from[..dir_end])
    }
}

#[cfg(test)]
mod tests;
