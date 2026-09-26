//! Reading a pasted address.
//!
//! Only two sites, and only the address forms a person gets from their pages.
//! Everything here is pure text handling, so every form is tested without the
//! network, and an address that is not one of them never causes a request.

/// What a pasted address names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAddress {
    /// One file in a Hugging Face model repository.
    HuggingFace {
        owner: String,
        repo: String,
        revision: String,
        /// The file's path inside the repository, with `/` between folders.
        path: String,
    },
    /// A Civitai model, a version of it, or both.
    Civitai { model_id: Option<u64>, version_id: Option<u64> },
}

/// Why an address was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressProblem {
    /// Not a Hugging Face or Civitai address this app reads.
    Bad,
    /// A Hugging Face model's own page, not one of its files.
    HfRepoNotFile,
}

/// Reads one pasted address.
pub fn parse(text: &str) -> Result<ModelAddress, AddressProblem> {
    let text = text.trim();
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    let (host_and_path, query) = match rest.split_once(['?', '#']) {
        Some((a, _)) => (a, query_of(rest)),
        None => (rest, ""),
    };
    let (host, path) = host_and_path.split_once('/').unwrap_or((host_and_path, ""));
    let host = host.to_ascii_lowercase();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match host.as_str() {
        "huggingface.co" | "www.huggingface.co" => hugging_face(&segments),
        "civitai.com" | "www.civitai.com" => civitai(&segments, query),
        _ => Err(AddressProblem::Bad),
    }
}

fn query_of(rest: &str) -> &str {
    let Some((_, after)) = rest.split_once('?') else { return "" };
    after.split('#').next().unwrap_or("")
}

fn hugging_face(segments: &[&str]) -> Result<ModelAddress, AddressProblem> {
    // A dataset or a Space is not a model repository.
    if matches!(segments.first(), Some(&"datasets") | Some(&"spaces") | None) {
        return Err(AddressProblem::Bad);
    }
    let (Some(owner), Some(repo)) = (segments.first(), segments.get(1)) else {
        return Err(AddressProblem::Bad);
    };
    if !is_hf_name(owner) || !is_hf_name(repo) {
        return Err(AddressProblem::Bad);
    }
    match segments.get(2) {
        // The model's page, or one of its tabs.
        None | Some(&"tree") | Some(&"commits") | Some(&"discussions") => {
            return Err(AddressProblem::HfRepoNotFile)
        }
        Some(&"blob") | Some(&"resolve") => {}
        Some(_) => return Err(AddressProblem::Bad),
    }
    let revision = segments.get(3).map(|r| decode(r)).ok_or(AddressProblem::HfRepoNotFile)?;
    let parts: Vec<String> = segments[4.min(segments.len())..].iter().map(|s| decode(s)).collect();
    if parts.is_empty() {
        return Err(AddressProblem::HfRepoNotFile);
    }
    // Every part is one folder or file name. Anything that could climb out of
    // the repository, or that the site would never serve, is refused here.
    // A decoded `/` would add a folder the address did not show.
    let bad = |p: &str| p.is_empty() || p == "." || p == ".." || p.contains(['/', '\\', '\0']);
    if bad(&revision) || parts.iter().any(|p| bad(p)) {
        return Err(AddressProblem::Bad);
    }
    Ok(ModelAddress::HuggingFace {
        owner: owner.to_string(),
        repo: repo.to_string(),
        revision,
        path: parts.join("/"),
    })
}

fn civitai(segments: &[&str], query: &str) -> Result<ModelAddress, AddressProblem> {
    match segments {
        ["models", id, ..] => {
            let model_id = number(id).ok_or(AddressProblem::Bad)?;
            let version_id = match query_value(query, "modelVersionId") {
                Some(v) => Some(number(v).ok_or(AddressProblem::Bad)?),
                None => None,
            };
            Ok(ModelAddress::Civitai { model_id: Some(model_id), version_id })
        }
        ["api", "download", "models", v] => Ok(ModelAddress::Civitai {
            model_id: None,
            version_id: Some(number(v).ok_or(AddressProblem::Bad)?),
        }),
        _ => Err(AddressProblem::Bad),
    }
}

fn number(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 15 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| *n > 0)
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

/// A Hugging Face owner or repository name, in the characters the site allows.
pub fn is_hf_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s != "."
        && s != ".."
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Undoes percent encoding. A broken escape is kept as written.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes one path segment for a request address.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hf(owner: &str, repo: &str, rev: &str, path: &str) -> ModelAddress {
        ModelAddress::HuggingFace {
            owner: owner.into(),
            repo: repo.into(),
            revision: rev.into(),
            path: path.into(),
        }
    }

    #[test]
    fn every_hugging_face_file_form_is_read() {
        let want = hf("Comfy-Org", "flux1-dev", "main", "flux1-dev-fp8.safetensors");
        for a in [
            "https://huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors",
            "https://huggingface.co/Comfy-Org/flux1-dev/resolve/main/flux1-dev-fp8.safetensors",
            "https://huggingface.co/Comfy-Org/flux1-dev/resolve/main/flux1-dev-fp8.safetensors?download=true",
            "huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors",
            "  https://www.huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors  ",
        ] {
            assert_eq!(parse(a), Ok(want.clone()), "{a}");
        }
        assert_eq!(
            parse("https://huggingface.co/o/r/blob/main/split_files/text_encoders/t5xxl_fp16.safetensors"),
            Ok(hf("o", "r", "main", "split_files/text_encoders/t5xxl_fp16.safetensors"))
        );
        assert_eq!(
            parse("https://huggingface.co/o/r/blob/main/my%20model.safetensors"),
            Ok(hf("o", "r", "main", "my model.safetensors")),
            "a space in a name arrives encoded"
        );
    }

    #[test]
    fn every_civitai_form_is_read() {
        let model = |m, v| ModelAddress::Civitai { model_id: Some(m), version_id: v };
        assert_eq!(parse("https://civitai.com/models/4384"), Ok(model(4384, None)));
        assert_eq!(parse("https://civitai.com/models/4384/dreamshaper"), Ok(model(4384, None)));
        assert_eq!(
            parse("https://civitai.com/models/4384?modelVersionId=128713"),
            Ok(model(4384, Some(128713)))
        );
        assert_eq!(
            parse("https://civitai.com/models/4384/dreamshaper?modelVersionId=128713"),
            Ok(model(4384, Some(128713)))
        );
        assert_eq!(
            parse("https://civitai.com/api/download/models/128713"),
            Ok(ModelAddress::Civitai { model_id: None, version_id: Some(128713) })
        );
        assert_eq!(
            parse("https://civitai.com/api/download/models/128713?type=Model&format=SafeTensor"),
            Ok(ModelAddress::Civitai { model_id: None, version_id: Some(128713) })
        );
    }

    #[test]
    fn a_whole_hugging_face_model_is_not_a_file() {
        for a in [
            "https://huggingface.co/Comfy-Org/flux1-dev",
            "https://huggingface.co/Comfy-Org/flux1-dev/",
            "https://huggingface.co/Comfy-Org/flux1-dev/tree/main",
            "https://huggingface.co/Comfy-Org/flux1-dev/tree/main/split_files",
            "https://huggingface.co/Comfy-Org/flux1-dev/blob/main",
        ] {
            assert_eq!(parse(a), Err(AddressProblem::HfRepoNotFile), "{a}");
        }
    }

    #[test]
    fn anything_else_is_refused_before_any_request() {
        for a in [
            "",
            "https://drive.google.com/file/d/1abc/view",
            "https://example.com/models/4384",
            "https://civitai.com/",
            "https://civitai.com/models/abc",
            "https://civitai.com/models/4384?modelVersionId=x",
            "https://civitai.com/images/123",
            "https://huggingface.co/datasets/o/r/blob/main/a.safetensors",
            "https://huggingface.co/spaces/o/r/blob/main/a.safetensors",
            "https://huggingface.co/o/r/blob/main/../../x.safetensors",
            "https://huggingface.co/o/r/blob/main/a%2F..%2F..%2Fb",
            "https://huggingface.co/o/r/blob/main/a%5Cb.safetensors",
            "https://huggingface.co/o/r/raw/main/a.safetensors",
            "https://huggingface.co/o%2F/r/blob/main/a.safetensors",
            "https://huggingface.co.evil.com/o/r/blob/main/a.safetensors",
            "https://civitai.com.evil.com/models/4384",
            "file:///C:/models/a.safetensors",
        ] {
            assert_eq!(parse(a), Err(AddressProblem::Bad), "{a}");
        }
    }

    #[test]
    fn a_folder_hidden_in_an_escape_is_refused() {
        // `%2F` decodes to `/`. Kept, it would add folders the address did not
        // show, and with `..` it would climb.
        assert_eq!(parse("https://huggingface.co/o/r/blob/main/a%2Fb.safetensors"), Err(AddressProblem::Bad));
        assert_eq!(parse("https://huggingface.co/o/r/blob/refs%2Fpr%2F1/a.safetensors"), Err(AddressProblem::Bad));
    }
}
