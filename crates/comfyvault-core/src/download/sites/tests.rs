//! Reading addresses against a server on this computer that answers the way
//! the two sites answered when they were checked on 2026-09-26.

use super::*;
use crate::download::address::parse;
use crate::download::http::UreqWeb;
use crate::download::test_server::{Canned, Req, Server};

const SHA: &str = "879db523c30d3b9017143d56705015e15a2cb5628762c11d086fed9538abd7fd";

fn is_model(name: &str) -> bool {
    name.ends_with(".safetensors") || name.ends_with(".ckpt")
}

fn read_at(server: &Server, address: &str, token: Option<&str>) -> Reading {
    let sites = Sites { hugging_face: server.base.clone(), civitai: server.base.clone() };
    let addr = parse(address).unwrap();
    read(&UreqWeb::new(), &sites, &addr, None, None, token, &[], &is_model).unwrap()
}

fn civitai_model(base: &str) -> String {
    format!(
        r#"{{"id":4384,"name":"DreamShaper","type":"Checkpoint","modelVersions":[
          {{"id":128713,"name":"8","baseModel":"SD 1.5","files":[
             {{"id":2,"name":"dreamshaper_8.yaml","sizeKB":1,"type":"Config","primary":false,"hashes":{{}},"downloadUrl":"{base}/api/download/models/128713?type=Config"}},
             {{"id":3,"name":"dreamshaper_8_inpaint.safetensors","sizeKB":10,"type":"Model","primary":false,"metadata":{{"fp":"fp32","size":"full","format":"SafeTensor"}},"hashes":{{"SHA256":"AAAA"}},"downloadUrl":"{base}/api/download/models/128713?type=Model&format=SafeTensor&size=full"}},
             {{"id":1,"name":"dreamshaper_8.safetensors","sizeKB":2048.5,"type":"Model","primary":true,"metadata":{{"fp":"fp16","size":"pruned","format":"SafeTensor"}},"hashes":{{"SHA256":"{sha}"}},"downloadUrl":"{base}/api/download/models/128713"}}
          ]}},
          {{"id":100,"name":"7","files":[{{"id":9,"name":"dreamshaper_7.safetensors","sizeKB":4,"primary":true,"hashes":{{}},"downloadUrl":"{base}/api/download/models/100"}}]}}
        ]}}"#,
        sha = SHA.to_uppercase()
    )
}

fn civitai_server(download: impl Fn(&Req) -> Canned + Send + Sync + 'static) -> Server {
    let base = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let b2 = base.clone();
    let server = Server::start(move |req| {
        let base = b2.lock().unwrap().clone();
        match req.path() {
            "/api/v1/models/4384" => Canned::json(200, &civitai_model(&base)),
            "/api/v1/model-versions/128713" => Canned::json(200, r#"{"id":128713,"modelId":4384}"#),
            p if p.starts_with("/api/download/models/") => download(req),
            _ => Canned::json(404, r#"{"error":"No model with id 1"}"#),
        }
    });
    *base.lock().unwrap() = server.base.clone();
    server
}

#[test]
fn a_civitai_model_page_chooses_the_newest_version_and_its_primary_file() {
    let s = civitai_server(|_| Canned::new(307).with_header("location", "https://storage.example/x?sig=1"));
    let Reading::File(f) = read_at(&s, "https://civitai.com/models/4384/dreamshaper", None) else {
        panic!("refused")
    };
    assert_eq!(f.host, Host::Civitai);
    assert_eq!(f.title, "DreamShaper");
    assert_eq!(f.version_id, Some(128713));
    assert_eq!(f.versions.iter().map(|v| v.id).collect::<Vec<_>>(), vec![128713, 100]);
    assert_eq!(f.file_name, "dreamshaper_8.safetensors");
    assert_eq!(f.files[0].name, "dreamshaper_8.safetensors", "the primary file comes first");
    assert!(!f.files.iter().any(|x| x.name.ends_with(".yaml")), "a config file is not a model");
    assert_eq!(f.files[0].detail, "pruned fp16 SafeTensor");
    assert_eq!(f.size_bytes, 2_097_664);
    assert_eq!(f.sha256.as_deref(), Some(SHA.to_uppercase().as_str()));
    assert_eq!(f.suggested_category.as_deref(), Some("checkpoints"));
    assert_eq!(f.suggested_because.as_deref(), Some("Civitai calls it a Checkpoint"));
    assert_eq!(f.model_id, Some(4384));
}

#[test]
fn a_civitai_version_and_file_can_be_chosen() {
    let s = civitai_server(|_| Canned::new(307).with_header("location", "https://storage.example/x"));
    let sites = Sites { hugging_face: s.base.clone(), civitai: s.base.clone() };
    let addr = parse("https://civitai.com/models/4384").unwrap();
    let Reading::File(f) = read(&UreqWeb::new(), &sites, &addr, Some(128713), Some(3), None, &[], &is_model).unwrap()
    else {
        panic!()
    };
    assert_eq!(f.file_name, "dreamshaper_8_inpaint.safetensors");
    let Reading::File(f) = read(&UreqWeb::new(), &sites, &addr, Some(100), None, None, &[], &is_model).unwrap() else {
        panic!()
    };
    assert_eq!(f.file_name, "dreamshaper_7.safetensors");
    assert_eq!(f.sha256, None, "Civitai gave no hash for this one");
}

#[test]
fn a_civitai_download_address_finds_its_model() {
    let s = civitai_server(|_| Canned::new(307).with_header("location", "https://storage.example/x"));
    let Reading::File(f) = read_at(&s, "https://civitai.com/api/download/models/128713", None) else { panic!() };
    assert_eq!(f.model_id, Some(4384));
    assert_eq!(f.version_id, Some(128713));
}

#[test]
fn civitai_asking_for_a_token_is_a_refusal_with_its_own_words() {
    let s = civitai_server(|_| Canned::json(401, r#"{"message":"You must be logged in to download this model."}"#));
    let Reading::Refused(r) = read_at(&s, "https://civitai.com/models/4384", None) else { panic!("read") };
    assert_eq!(r.kind, RefusalKind::TokenMissing);
    assert_eq!(r.host, Some(Host::Civitai));
    assert_eq!(r.service_message.as_deref(), Some("You must be logged in to download this model."));
    assert_eq!(r.title.as_deref(), Some("DreamShaper"), "the name Civitai gave before it refused");
    assert!(r.subtitle.as_deref().unwrap().ends_with("/models/4384"));

    let Reading::Refused(r) = read_at(&s, "https://civitai.com/models/4384", Some("bad")) else { panic!() };
    assert_eq!(r.kind, RefusalKind::TokenRejected);
}

#[test]
fn a_civitai_model_that_does_not_exist_is_not_found() {
    let s = civitai_server(|_| Canned::new(307));
    let Reading::Refused(r) = read_at(&s, "https://civitai.com/models/1", None) else { panic!() };
    assert_eq!(r.kind, RefusalKind::NotFound);
    assert_eq!(r.service_message.as_deref(), Some("No model with id 1"));
}

#[test]
fn a_download_address_elsewhere_in_civitais_answer_is_never_used() {
    let s = Server::start(|req| match req.path() {
        "/api/v1/models/4384" => Canned::json(
            200,
            r#"{"name":"X","type":"LORA","modelVersions":[{"id":1,"name":"1","files":[{"id":1,"name":"x.safetensors","sizeKB":1,"primary":true,"downloadUrl":"https://evil.example/x"}]}]}"#,
        ),
        _ => Canned::new(500),
    });
    let sites = Sites { hugging_face: s.base.clone(), civitai: s.base.clone() };
    let addr = parse("https://civitai.com/models/4384").unwrap();
    let err = read(&UreqWeb::new(), &sites, &addr, None, None, None, &[], &is_model).unwrap_err();
    assert!(err.message.contains("somewhere else"), "{}", err.message);
    assert!(s.requests().iter().all(|r| r.path() == "/api/v1/models/4384"), "the other address was asked");
}

fn hf_server(lfs: bool, access: u16, headers: Vec<(&'static str, &'static str)>) -> Server {
    Server::start(move |req| match (req.method.as_str(), req.path()) {
        ("HEAD", "/Comfy-Org/flux1-dev/resolve/main/split_files/text_encoders/t5.safetensors") => {
            let mut c = Canned::new(access);
            for (k, v) in &headers {
                c = c.with_header(k, v);
            }
            c
        }
        ("POST", "/api/models/Comfy-Org/flux1-dev/paths-info/main") => {
            assert_eq!(req.body, "paths=split_files%2Ftext_encoders%2Ft5.safetensors");
            if lfs {
                Canned::json(
                    200,
                    &format!(r#"[{{"type":"file","path":"split_files/text_encoders/t5.safetensors","size":17246524772,"lfs":{{"oid":"{SHA}","size":17246524772}}}}]"#),
                )
            } else {
                Canned::json(200, r#"[{"type":"file","path":"split_files/text_encoders/t5.safetensors","size":1234}]"#)
            }
        }
        _ => Canned::new(404),
    })
}

const HF_FILE: &str = "https://huggingface.co/Comfy-Org/flux1-dev/blob/main/split_files/text_encoders/t5.safetensors";

#[test]
fn a_hugging_face_file_gives_its_size_hash_and_folder() {
    let s = hf_server(true, 302, vec![("location", "https://cas.example/x?X-Amz-Signature=1")]);
    let Reading::File(f) = read_at(&s, HF_FILE, None) else { panic!() };
    assert_eq!(f.host, Host::HuggingFace);
    assert_eq!(f.title, "t5.safetensors");
    assert_eq!(f.subtitle, "Comfy-Org/flux1-dev");
    assert_eq!(f.size_bytes, 17_246_524_772);
    assert_eq!(f.sha256.as_deref(), Some(SHA.to_uppercase().as_str()));
    assert_eq!(f.suggested_category.as_deref(), Some("text_encoders"));
    assert_eq!(f.page, Some(HfPage { owner: "Comfy-Org".into(), repo: "flux1-dev".into() }));
    assert!(f.fetch_url.ends_with("/Comfy-Org/flux1-dev/resolve/main/split_files/text_encoders/t5.safetensors"));
}

#[test]
fn a_small_hugging_face_file_has_no_hash_before_the_download() {
    let s = hf_server(false, 200, vec![]);
    let Reading::File(f) = read_at(&s, HF_FILE, None) else { panic!() };
    assert_eq!(f.sha256, None);
    assert_eq!(f.size_bytes, 1234);
}

#[test]
fn a_gated_model_without_a_token_asks_for_one_in_the_sites_words() {
    let msg = "Access to model black-forest-labs/FLUX.1-dev is restricted. You must have access to it and be authenticated to access it. Please log in.";
    let s = hf_server(true, 401, vec![("x-error-code", "GatedRepo"), ("x-error-message", msg)]);
    let Reading::Refused(r) = read_at(&s, HF_FILE, None) else { panic!() };
    assert_eq!(r.kind, RefusalKind::TokenMissing);
    assert_eq!(r.service_message.as_deref(), Some(msg));
    assert_eq!(r.page, Some(HfPage { owner: "Comfy-Org".into(), repo: "flux1-dev".into() }));
    assert_eq!(r.title.as_deref(), Some("t5.safetensors"));
    assert_eq!(r.subtitle.as_deref(), Some("Comfy-Org/flux1-dev"));

    let Reading::Refused(r) = read_at(&s, HF_FILE, Some("hf_x")) else { panic!() };
    assert_eq!(r.kind, RefusalKind::TokenRejected);
}

#[test]
fn a_gated_model_the_account_has_no_access_to_says_so() {
    let s = hf_server(true, 403, vec![("x-error-code", "GatedRepo"), ("x-error-message", "Accept the terms first.")]);
    let Reading::Refused(r) = read_at(&s, HF_FILE, Some("hf_x")) else { panic!() };
    assert_eq!(r.kind, RefusalKind::NoAccess);
    assert_eq!(r.service_message.as_deref(), Some("Accept the terms first."));
}

#[test]
fn a_missing_repository_is_not_found_even_when_the_site_says_401() {
    // Hugging Face answers 401 for a repository that does not exist, so a
    // private one cannot be told apart from a missing one.
    let s = hf_server(true, 401, vec![("x-error-code", "RepoNotFound"), ("x-error-message", "Repository not found")]);
    let Reading::Refused(r) = read_at(&s, HF_FILE, None) else { panic!() };
    assert_eq!(r.kind, RefusalKind::NotFound);
}

#[test]
fn a_token_goes_only_to_the_site_and_never_into_an_address() {
    let s = hf_server(true, 302, vec![("location", "https://cas.example/x")]);
    read_at(&s, HF_FILE, Some("hf_secret"));
    for r in s.requests() {
        assert_eq!(r.header("authorization"), Some("Bearer hf_secret"));
        assert!(!r.target.contains("hf_secret"));
    }
}

#[test]
fn a_file_that_is_not_a_model_is_refused_before_any_request() {
    let s = hf_server(true, 302, vec![]);
    let sites = Sites { hugging_face: s.base.clone(), civitai: s.base.clone() };
    let addr = parse("https://huggingface.co/o/r/blob/main/run.bat").unwrap();
    let err = read(&UreqWeb::new(), &sites, &addr, None, None, None, &[], &is_model).unwrap_err();
    assert_eq!(err.code, crate::ErrorCode::InvalidArgument);
    assert!(s.requests().is_empty());
}

#[test]
fn a_token_is_checked_with_the_site_and_the_account_is_named() {
    let s = Server::start(|req| match (req.path(), req.header("authorization")) {
        ("/api/whoami-v2", Some("Bearer good")) => Canned::json(200, r#"{"name":"sam","type":"user"}"#),
        ("/api/whoami-v2", _) => Canned::json(401, r#"{"error":"Invalid username or password."}"#),
        ("/api/v1/me", Some("Bearer good")) => Canned::json(200, r#"{"id":1}"#),
        ("/api/v1/me", _) => Canned::json(401, r#"{"error":"Unauthorized"}"#),
        _ => Canned::new(404),
    });
    let sites = Sites { hugging_face: s.base.clone(), civitai: s.base.clone() };
    let web = UreqWeb::new();
    assert_eq!(check_token(&web, &sites, Host::HuggingFace, "good").unwrap(), Ok(Some("sam".into())));
    assert_eq!(
        check_token(&web, &sites, Host::HuggingFace, "bad").unwrap(),
        Err("Invalid username or password.".into())
    );
    assert_eq!(check_token(&web, &sites, Host::Civitai, "good").unwrap(), Ok(None));
    assert_eq!(check_token(&web, &sites, Host::Civitai, "bad").unwrap(), Err("Unauthorized".into()));
}

#[test]
fn a_civitai_file_with_no_size_or_a_size_below_nothing_is_not_offered() {
    let s = Server::start(|req| match req.path() {
        "/api/v1/models/4384" => Canned::json(
            200,
            r#"{"name":"X","type":"LORA","modelVersions":[{"id":1,"name":"1","files":[
                {"id":1,"name":"neg.safetensors","sizeKB":-5,"primary":true,"downloadUrl":"BASE/api/download/models/1"},
                {"id":2,"name":"none.safetensors","primary":false,"downloadUrl":"BASE/api/download/models/1"}
            ]}]}"#,
        ),
        _ => Canned::new(404),
    });
    let sites = Sites { hugging_face: s.base.clone(), civitai: s.base.clone() };
    let addr = parse("https://civitai.com/models/4384").unwrap();
    let err = read(&UreqWeb::new(), &sites, &addr, None, None, None, &[], &is_model).unwrap_err();
    assert!(err.message.contains("no model file"), "{}", err.message);
}

