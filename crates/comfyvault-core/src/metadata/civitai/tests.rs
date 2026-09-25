//! Civitai client tests.
//!
//! The network tests drive the real [`UreqTransport`] at a real server on a
//! real socket, so the code under test is the request building and response
//! reading that ships, not a stand-in.

use super::*;
use crate::metadata::http::test_server::{Request, TestServer};
use crate::metadata::http::UreqTransport;
use std::sync::Arc;

const SHA_A: &str = "879DB523C30D0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0B0BABD7FD";
const SHA_B: &str = "47AAAF0D29AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA6E2F70";
const SHA_C: &str = "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";

/// A trimmed version of a real Civitai answer.
fn version_json(id: u64, model_id: u64, sha: &str, name: &str) -> String {
    format!(
        r#"{{
        "id": {id},
        "modelId": {model_id},
        "name": "v1.0",
        "nsfwLevel": 3,
        "trainedWords": ["abstractionism, brush stroke, traditional media, "],
        "baseModel": "Pony",
        "description": null,
        "model": {{ "name": "{name}", "type": "LORA", "nsfw": false, "poi": false }},
        "files": [{{
            "id": 1,
            "sizeKB": 56075.02,
            "name": "{name}.safetensors",
            "type": "Model",
            "metadata": {{ "format": "SafeTensor", "size": null, "fp": null }},
            "hashes": {{
                "AutoV1": "709B4299",
                "AutoV2": "DF7C757437",
                "SHA256": "{sha}",
                "CRC32": "E98D67FC",
                "BLAKE3": "CB5461E0"
            }},
            "primary": true,
            "downloadUrl": "https://civitai.com/api/download/models/{id}?fileId=1"
        }}],
        "images": [
            {{ "url": "https://image.civitai.com/a/original=true/1.jpeg", "nsfwLevel": 1 }},
            {{ "url": "https://image.civitai.com/a/original=true/2.jpeg", "nsfwLevel": 1 }}
        ],
        "downloadUrl": "https://civitai.com/api/download/models/{id}"
    }}"#
    )
}

fn parse_version(json: &str) -> ApiVersion {
    serde_json::from_str(json).expect("parse a version")
}

// --- parsing ---------------------------------------------------------------

#[test]
fn a_real_shaped_answer_is_read_into_the_fields_the_ui_shows() {
    let v = parse_version(&version_json(1558543, 264290, SHA_A, "Abstract Painting"));
    let m = convert(&v, SHA_A, false);

    assert!(m.found);
    assert_eq!(m.sha256, SHA_A);
    assert_eq!(m.model_name.as_deref(), Some("Abstract Painting"));
    assert_eq!(m.model_type.as_deref(), Some("LORA"));
    assert_eq!(m.version_name.as_deref(), Some("v1.0"));
    assert_eq!(m.base_model.as_deref(), Some("Pony"));
    assert_eq!(m.civitai_model_id, Some(264290));
    assert_eq!(m.civitai_version_id, Some(1558543));
    assert_eq!(m.nsfw_level, 3);
    assert!(!m.nsfw);
    assert_eq!(m.preview_image_urls.len(), 2);
    assert_eq!(
        m.page_url.as_deref(),
        Some("https://civitai.com/models/264290?modelVersionId=1558543")
    );
    assert!(m.download_url.unwrap().contains("fileId=1"));
}

#[test]
fn trigger_words_crammed_into_one_string_are_split_into_usable_words() {
    // Civitai's field is not a clean list. One element often holds several
    // triggers with a trailing comma. Showing that unchanged puts one long
    // string in front of the person where there are three separate words.
    let v = parse_version(&version_json(1, 2, SHA_A, "X"));
    let m = convert(&v, SHA_A, false);
    assert_eq!(
        m.trigger_words,
        vec!["abstractionism", "brush stroke", "traditional media"]
    );
}

#[test]
fn trigger_words_that_are_already_a_clean_list_are_left_alone() {
    let json = r#"{"id":1,"modelId":2,"trainedWords":["realistic","detailed"],"files":[],"images":[]}"#;
    let m = convert(&parse_version(json), SHA_A, false);
    assert_eq!(m.trigger_words, vec!["realistic", "detailed"]);
}

#[test]
fn a_repeated_trigger_word_appears_once() {
    let json = r#"{"id":1,"modelId":2,"trainedWords":["a, b","b, c"],"files":[],"images":[]}"#;
    let m = convert(&parse_version(json), SHA_A, false);
    assert_eq!(m.trigger_words, vec!["a", "b", "c"]);
}

#[test]
fn an_answer_with_every_optional_field_null_still_parses() {
    // Several fields are null on most real records. A strict reader would fail
    // on the majority of the library.
    let json = r#"{
        "id": 7, "modelId": null, "name": null, "baseModel": null,
        "trainedWords": [], "nsfwLevel": null, "model": null,
        "files": [], "images": [], "downloadUrl": null
    }"#;
    let m = convert(&parse_version(json), SHA_A, false);
    assert!(m.found);
    assert_eq!(m.model_name, None);
    assert_eq!(m.base_model, None);
    assert_eq!(m.nsfw_level, 0);
    assert!(m.trigger_words.is_empty());
    assert!(m.preview_image_urls.is_empty());
    assert_eq!(m.page_url, None, "no model id means no page to link to");
}

#[test]
fn an_answer_with_fields_this_app_has_never_seen_still_parses() {
    // The service adds fields without notice. A reader that refused unknown
    // fields would break the feature on a Tuesday for no reason.
    let json = r#"{"id":1,"modelId":2,"files":[],"images":[],
        "somethingBrandNew": {"nested": [1,2,3]}, "anotherOne": "x"}"#;
    assert!(serde_json::from_str::<ApiVersion>(json).is_ok());
}

// --- batch matching --------------------------------------------------------

#[test]
fn a_batch_answer_is_matched_by_hash_not_by_position() {
    // Civitai returns the matches in an arbitrary order. Matching by position
    // would put one model's name on another model's file.
    let versions = vec![
        parse_version(&version_json(200, 20, SHA_B, "Model B")),
        parse_version(&version_json(100, 10, SHA_A, "Model A")),
    ];
    let got = match_batch(&[SHA_A.to_string(), SHA_B.to_string()], &versions);

    assert_eq!(got.len(), 2);
    assert_eq!(got[0].sha256, SHA_A);
    assert_eq!(got[0].model_name.as_deref(), Some("Model A"));
    assert_eq!(got[1].sha256, SHA_B);
    assert_eq!(got[1].model_name.as_deref(), Some("Model B"));
}

#[test]
fn a_hash_that_is_simply_absent_from_the_answer_becomes_a_miss() {
    // The batch endpoint drops misses rather than returning a null for them.
    let versions = vec![parse_version(&version_json(100, 10, SHA_A, "Model A"))];
    let got = match_batch(
        &[SHA_A.to_string(), SHA_C.to_string(), SHA_B.to_string()],
        &versions,
    );

    assert_eq!(got.len(), 3, "one record per hash asked about, always");
    assert!(got[0].found);
    assert!(!got[1].found);
    assert_eq!(got[1].sha256, SHA_C);
    assert!(!got[2].found);
}

#[test]
fn one_file_matching_several_models_keeps_the_original_upload_and_says_it_is_ambiguous() {
    // People re-upload identical files under their own pages. The lowest
    // version identifier is the original, which is also what the single lookup
    // returns, so the two paths agree.
    let versions = vec![
        parse_version(&version_json(3200512, 2825890, SHA_A, "A Re-upload")),
        parse_version(&version_json(290640, 257749, SHA_A, "The Original")),
        parse_version(&version_json(2979646, 2653610, SHA_A, "Another Re-upload")),
    ];
    let got = match_batch(&[SHA_A.to_string()], &versions);

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].model_name.as_deref(), Some("The Original"));
    assert_eq!(got[0].civitai_version_id, Some(290640));
    assert!(got[0].ambiguous, "the person must be told the match is not unique");
}

#[test]
fn a_batch_answer_holding_more_records_than_hashes_still_gives_one_record_per_hash() {
    let versions = vec![
        parse_version(&version_json(100, 10, SHA_A, "One")),
        parse_version(&version_json(101, 11, SHA_A, "Two")),
        parse_version(&version_json(200, 20, SHA_B, "Three")),
    ];
    let got = match_batch(&[SHA_A.to_string(), SHA_B.to_string()], &versions);
    assert_eq!(got.len(), 2);
}

#[test]
fn hash_matching_ignores_case() {
    let versions = vec![parse_version(&version_json(100, 10, &SHA_A.to_lowercase(), "A"))];
    let got = match_batch(&[SHA_A.to_string()], &versions);
    assert!(got[0].found);
    assert_eq!(got[0].sha256, SHA_A, "the stored hash is always upper case");
}

#[test]
fn an_empty_batch_answer_makes_every_hash_a_miss() {
    let got = match_batch(&[SHA_A.to_string(), SHA_B.to_string()], &[]);
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|m| !m.found));
}

// --- over a real socket ----------------------------------------------------

#[test]
fn a_single_lookup_asks_the_documented_url_and_reads_the_answer() {
    let server = TestServer::start(Arc::new(|req: &Request| {
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, format!("/api/v1/model-versions/by-hash/{SHA_A}"));
        (200, version_json(100, 10, SHA_A, "Found It"))
    }));

    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);
    let m = c.fetch_one(SHA_A).unwrap();

    assert!(m.found);
    assert_eq!(m.model_name.as_deref(), Some("Found It"));
}

#[test]
fn a_404_means_no_match_and_is_not_an_error() {
    let server = TestServer::start(Arc::new(|_: &Request| {
        (404, "{\"error\":\"Model not found\"}".to_string())
    }));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);

    let m = c.fetch_one(SHA_A).unwrap();
    assert!(!m.found, "a file nobody uploaded is normal, not a failure");
    assert_eq!(m.sha256, SHA_A);
}

#[test]
fn a_lookup_carries_no_credential_at_all() {
    // Verified against the live service: the same request with no header, a
    // bogus bearer token, and a token on the query string all return the same
    // answer byte for byte. Civitai gates downloading, not looking up. So the
    // client sends nothing, and there is no credential to leak.
    let server = TestServer::start(Arc::new(|req: &Request| {
        let auth = req.header("authorization").unwrap_or("none").to_string();
        assert!(!req.path.contains("token"), "a credential reached the query string");
        (200, format!(r#"{{"id":1,"modelId":2,"name":"{auth}","files":[],"images":[]}}"#))
    }));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);

    assert_eq!(c.fetch_one(SHA_A).unwrap().version_name.as_deref(), Some("none"));
}

#[test]
fn a_batch_sends_one_request_for_many_hashes() {
    // Three hundred files become three requests instead of three hundred.
    let server = TestServer::start(Arc::new(|req: &Request| {
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/v1/model-versions/by-hash");
        let sent: Vec<String> = serde_json::from_str(&req.body).expect("a JSON array of hashes");
        let body: Vec<String> = sent
            .iter()
            .filter(|h| h.as_str() == SHA_A)
            .map(|h| version_json(100, 10, h, "Only A"))
            .collect();
        (200, format!("[{}]", body.join(",")))
    }));

    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);
    let got = c.fetch_many(&[SHA_A.to_string(), SHA_B.to_string()]).unwrap();

    assert_eq!(server.request_count(), 1, "both hashes must go in one request");
    assert_eq!(got.len(), 2);
    assert!(got[0].found);
    assert!(!got[1].found);
}

#[test]
fn more_than_a_hundred_hashes_are_split_into_chunks_the_service_accepts() {
    // The batch endpoint answers 400 for 101 hashes.
    let server = TestServer::start(Arc::new(|req: &Request| {
        let sent: Vec<String> = serde_json::from_str(&req.body).unwrap();
        assert!(sent.len() <= BATCH_LIMIT, "sent {} hashes in one request", sent.len());
        (200, "[]".to_string())
    }));

    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);
    let hashes: Vec<String> = (0..250).map(|i| format!("{i:064X}")).collect();
    let got = c.fetch_many(&hashes).unwrap();

    assert_eq!(got.len(), 250, "one record per hash, across every chunk");
    assert_eq!(server.request_count(), 3, "250 hashes is three requests of at most 100");
}

#[test]
fn a_batch_the_service_refuses_falls_back_to_one_at_a_time() {
    // The batch endpoint is undocumented, so it can change without notice.
    // Losing it must degrade identification, not break it.
    let server = TestServer::start(Arc::new(|req: &Request| {
        if req.method == "POST" {
            return (400, r#"{"error":"gone"}"#.to_string());
        }
        (200, version_json(100, 10, SHA_A, "Found One By One"))
    }));

    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);
    let got = c.fetch_many(&[SHA_A.to_string(), SHA_B.to_string()]).unwrap();

    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|m| m.found), "the single lookups answered");
    assert_eq!(server.request_count(), 3, "one failed batch, then two single lookups");
}

#[test]
fn being_asked_to_slow_down_is_reported_as_a_temporary_network_problem() {
    let server = TestServer::start(Arc::new(|_: &Request| (429, "{}".to_string())));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);

    let err = c.fetch_one(SHA_A).unwrap_err();
    assert_eq!(err.code, crate::ErrorCode::NetworkUnavailable);
    assert!(err.message.contains("slow down"));
}

#[test]
fn a_server_failure_is_a_network_problem_and_never_a_crash() {
    let server = TestServer::start(Arc::new(|_: &Request| (500, "oops".to_string())));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);
    assert_eq!(c.fetch_one(SHA_A).unwrap_err().code, crate::ErrorCode::NetworkUnavailable);
}

#[test]
fn an_answer_that_is_not_json_is_reported_rather_than_crashing() {
    let server = TestServer::start(Arc::new(|_: &Request| {
        (200, "<html>we are down for maintenance</html>".to_string())
    }));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);

    let err = c.fetch_one(SHA_A).unwrap_err();
    assert_eq!(err.code, crate::ErrorCode::NetworkUnavailable);
    assert!(err.message.contains("could not read"));
}

#[test]
fn an_empty_batch_asks_nothing() {
    let server = TestServer::start(Arc::new(|_: &Request| (200, "[]".to_string())));
    let t = UreqTransport::new();
    let c = CivitaiClient::new(&t).with_base_url(&server.base_url);

    assert!(c.fetch_many(&[]).unwrap().is_empty());
    assert_eq!(server.request_count(), 0);
}

#[test]
fn each_picture_keeps_its_own_rating_and_kind_in_civitais_order() {
    // Civitai rates each picture on its own. Measured on the batch answer for
    // DreamShaper 8: 2, 1, 1, 1, 1, 1, 8, 1, 1, 1, while the version says 11.
    // A picture with no rating is kept with 0, which is not a rating.
    let json = r#"{"id":1,"modelId":2,"files":[],"images":[
        {"url":"https://image.civitai.com/a/1.jpeg","nsfwLevel":2,"type":"image"},
        {"url":"https://image.civitai.com/a/2.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://image.civitai.com/a/3.mp4","nsfwLevel":8,"type":"video"},
        {"url":"https://image.civitai.com/a/4.jpeg"},
        {"nsfwLevel":1,"type":"image"}
    ]}"#;
    let m = convert(&parse_version(json), SHA_A, false);
    let got: Vec<(&str, u32, &str)> =
        m.preview_images.iter().map(|p| (p.url.as_str(), p.nsfw_level, p.kind.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("https://image.civitai.com/a/1.jpeg", 2, "image"),
            ("https://image.civitai.com/a/2.jpeg", 1, "image"),
            ("https://image.civitai.com/a/3.mp4", 8, "video"),
            ("https://image.civitai.com/a/4.jpeg", 0, ""),
        ]
    );
    assert_eq!(m.preview_image_urls.len(), 4, "the plain list stays as it was");
}

#[test]
fn a_cached_answer_from_an_older_build_reads_with_no_pictures_rated() {
    let mut v = serde_json::to_value(crate::metadata::ModelMetadata::not_found(SHA_A)).unwrap();
    v.as_object_mut().unwrap().remove("previewImages");
    let m: crate::metadata::ModelMetadata = serde_json::from_value(v).unwrap();
    assert!(m.preview_images.is_empty());
}

#[test]
fn only_pictures_on_civitais_image_host_over_https_are_kept() {
    let json = r#"{"id":1,"modelId":2,"files":[],"images":[
        {"url":"https://image.civitai.com/a/1.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"http://image.civitai.com/a/2.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://evil.example/a/3.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://image.civitai.com.evil.example/4.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://image.civitai.com@evil.example/5.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://image.civitai.com:8443/6.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://civitai.com/7.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://IMAGE.civitai.com/a/8.jpeg","nsfwLevel":2,"type":"image"},
        {"url":"https://blobs-b2.civitai.com/a/9.jpeg","nsfwLevel":1,"type":"image"},
        {"url":"https://blobs-b3.civitai.com/a/10.jpeg","nsfwLevel":1,"type":"image"}
    ]}"#;
    let m = convert(&parse_version(json), SHA_A, false);
    let kept: Vec<&str> = m.preview_images.iter().map(|p| p.url.as_str()).collect();
    assert_eq!(
        kept,
        vec![
            "https://image.civitai.com/a/1.jpeg",
            "https://IMAGE.civitai.com/a/8.jpeg",
            "https://blobs-b2.civitai.com/a/9.jpeg",
        ]
    );
    assert_eq!(m.preview_image_urls, kept);
}

#[test]
fn a_cached_answer_is_held_to_the_same_rules_when_it_is_read() {
    // A row cached before the picture check existed, or written by whoever
    // made the vault, reached the window as it was stored.
    let w = crate::testkit::TestWorld::new();
    let mut m = crate::metadata::ModelMetadata::not_found(SHA_A);
    m.found = true;
    m.page_url = Some("https://evil.example/phish".into());
    m.download_url = Some("https://evil.example/file".into());
    m.preview_image_urls = vec!["https://evil.example/1.jpeg".into(), "https://image.civitai.com/2.jpeg".into()];
    m.preview_images = vec![
        crate::metadata::PreviewImage { url: "https://evil.example/1.jpeg".into(), nsfw_level: 1, kind: "image".into() },
        crate::metadata::PreviewImage { url: "https://image.civitai.com/2.jpeg".into(), nsfw_level: 1, kind: "image".into() },
    ];
    w.store.put_metadata(&m).unwrap();

    let read = w.store.metadata(SHA_A).unwrap().unwrap();
    assert_eq!(read.page_url, None);
    assert_eq!(read.download_url, None);
    assert_eq!(read.preview_image_urls, vec!["https://image.civitai.com/2.jpeg".to_string()]);
    assert_eq!(read.preview_images.len(), 1);

    // A page the engine builds itself survives.
    m.page_url = Some("https://civitai.com/models/4384?modelVersionId=128713".into());
    w.store.put_metadata(&m).unwrap();
    assert_eq!(w.store.metadata(SHA_A).unwrap().unwrap().page_url, m.page_url);
    for bad in ["https://civitai.com/models/4384/../../x", "https://civitai.com/models/", "https://civitai.com/models/1?modelVersionId=2&x=3"] {
        m.page_url = Some(bad.into());
        w.store.put_metadata(&m).unwrap();
        assert_eq!(w.store.metadata(SHA_A).unwrap().unwrap().page_url, None, "{bad}");
    }
}
