//! The transfer against a server on this computer that behaves like a site
//! and its storage, including the ways they go wrong.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::download::http::UreqWeb;
use crate::download::sites::Host;
use crate::download::test_server::{Canned, Req, Server};

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 % 251) as u8).collect()
}

thread_local! {
    /// The size of the file the test's world serves, which is the size the
    /// site states for it.
    static SIZE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A site that redirects to storage, and storage that serves `content`.
/// `storage` can replace the storage's answer, per request number.
fn world(content: Vec<u8>, storage: impl Fn(usize, &Req, &[u8]) -> Option<Canned> + Send + Sync + 'static) -> (Server, Arc<AtomicUsize>) {
    SIZE.with(|c| c.set(content.len() as u64));
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let server = Server::start(move |req| match req.path() {
        "/site/file" => Canned::redirect("/store/file?X-Amz-Signature=abc"),
        "/store/file" => {
            let n = h.fetch_add(1, Ordering::SeqCst);
            storage(n, req, &content).unwrap_or_else(|| Canned::file(req, &content, "\"v1\""))
        }
        _ => Canned::new(404),
    });
    (server, hits)
}

fn file(server: &Server) -> RemoteFile {
    RemoteFile {
        host: Host::HuggingFace,
        title: "m".into(),
        subtitle: "o/r".into(),
        versions: vec![],
        version_id: None,
        files: vec![],
        file_id: None,
        file_name: "m.safetensors".into(),
        size_bytes: SIZE.with(|c| c.get()),
        sha256: None,
        suggested_category: None,
        suggested_because: None,
        page: None,
        model_id: None,
        fetch_url: format!("{}/site/file", server.base),
    }
}

fn go(server: &Server, part: &Path, etag: Option<&str>, token: Option<&str>) -> Result<Outcome, Failure> {
    run(&UreqWeb::new(), &file(server), token, part, etag, &mut None, &CancelToken::new(), &mut |_, _| {})
}

#[test]
fn a_whole_file_arrives_through_the_redirect() {
    let content = data(300_000);
    let (s, _) = world(content.clone(), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let out = go(&s, &part, None, None).unwrap();
    assert_eq!(out, Outcome::Complete { total: 300_000, etag: Some("\"v1\"".into()) });
    assert_eq!(std::fs::read(&part).unwrap(), content);
}

#[test]
fn a_token_goes_to_the_site_and_never_to_the_storage() {
    let (s, _) = world(data(1000), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    go(&s, &dir.path().join("x.part"), None, Some("hf_secret")).unwrap();
    for r in s.requests() {
        let expected = if r.path() == "/site/file" { Some("Bearer hf_secret") } else { None };
        assert_eq!(r.header("authorization"), expected, "{}", r.target);
    }
}

#[test]
fn a_dropped_line_keeps_the_part_and_continue_finishes_it_from_there() {
    let content = data(500_000);
    let (s, hits) = world(content.clone(), |n, req, full| {
        (n == 0).then(|| Canned { cut_after: Some(200_000), ..Canned::file(req, full, "\"v1\"") })
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");

    let mut version = None;
    let err = run(&UreqWeb::new(), &file(&s), None, &part, None, &mut version, &CancelToken::new(), &mut |_, _| {})
        .unwrap_err();
    assert_eq!(err.kind, FailureKind::Connection);
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 200_000, "the part is kept");
    assert_eq!(version.as_deref(), Some("\"v1\""), "a failed attempt still names the version it kept");

    let out = go(&s, &part, Some("\"v1\""), None).unwrap();
    assert_eq!(out, Outcome::Complete { total: 500_000, etag: Some("\"v1\"".into()) });
    assert_eq!(std::fs::read(&part).unwrap(), content, "the two halves join into the file");
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let storage: Vec<Req> = s.requests().into_iter().filter(|r| r.path() == "/store/file").collect();
    assert_eq!(storage[1].header("range"), Some("bytes=200000-"));
    assert_eq!(storage[1].header("if-range"), Some("\"v1\""));
    // Each attempt asked the site again for a fresh storage address.
    assert_eq!(s.requests().iter().filter(|r| r.path() == "/site/file").count(), 2);
}

#[test]
fn storage_that_ignores_the_range_sends_it_whole_and_nothing_is_doubled() {
    let content = data(100_000);
    let (s, _) = world(content.clone(), |_, _, full| Some(Canned::new(200).with_header("etag", "\"v1\"").with_body(full)));
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, &content[..40_000]).unwrap();
    go(&s, &part, Some("\"v1\""), None).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), content);
}

#[test]
fn a_file_that_changed_on_the_site_starts_again_from_nothing() {
    let old = data(80_000);
    let new: Vec<u8> = data(90_000).into_iter().map(|b| b ^ 0xff).collect();
    let (s, _) = world(new.clone(), |_, req, full| Some(Canned::file(req, full, "\"v2\"")));
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, &old[..30_000]).unwrap();
    go(&s, &part, Some("\"v1\""), None).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), new, "an old part is never stitched onto a new file");
}

#[test]
fn a_kept_part_with_no_version_name_is_not_continued_blindly() {
    let content = data(50_000);
    let (s, _) = world(content.clone(), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, b"bytes from who knows where").unwrap();
    go(&s, &part, None, None).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), content);
}

#[test]
fn stopping_keeps_the_part() {
    let content = data(3_000_000);
    let (s, _) = world(content.clone(), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let out = run(&UreqWeb::new(), &file(&s), None, &part, None, &mut None, &cancel, &mut |done, _| {
        if done > 0 {
            c2.cancel();
        }
    })
    .unwrap();
    assert!(matches!(out, Outcome::Stopped { .. }), "{out:?}");
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(kept > 0 && kept < 3_000_000, "kept {kept}");
    assert_eq!(std::fs::read(&part).unwrap(), content[..kept as usize]);
}

#[test]
fn a_site_that_refuses_part_way_is_a_refusal_in_its_own_words_and_no_retry() {
    let s = Server::start(|req| match req.path() {
        "/site/file" => Canned::json(401, r#"{"error":"Invalid credentials in Authorization header"}"#),
        _ => Canned::new(404),
    });
    let dir = tempfile::tempdir().unwrap();
    let err = go(&s, &dir.path().join("x.part"), None, Some("hf_old")).unwrap_err();
    assert_eq!(err.kind, FailureKind::Refused);
    assert_eq!(err.service_message.as_deref(), Some("Invalid credentials in Authorization header"));
    assert_eq!(s.requests().len(), 1, "asked once, never again on its own");
}

#[test]
fn an_expired_storage_address_says_so_in_the_storages_words() {
    let (s, _) = world(data(10), |_, _, _| {
        Some(
            Canned::new(403)
                .with_header("content-type", "application/xml")
                .with_body(b"<?xml version=\"1.0\"?><Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>"),
        )
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, b"kept").unwrap();
    let err = go(&s, &part, Some("\"v1\""), None).unwrap_err();
    assert_eq!(err.kind, FailureKind::Expired);
    assert_eq!(err.service_message.as_deref(), Some("Request has expired"));
    assert_eq!(std::fs::read(&part).unwrap(), b"kept", "the part is kept");
    assert_eq!(s.requests().len(), 2, "one attempt, no retry loop");
}

#[test]
fn a_line_that_goes_silent_counts_as_dropped() {
    // The product waits thirty seconds. The test uses the same connection
    // with a shorter limit, and a server that stalls far longer than it.
    let content = data(100_000);
    let (s, _) = world(content, |_, req, full| {
        Some(Canned { stall_after: Some((10_000, std::time::Duration::from_secs(20))), ..Canned::file(req, full, "\"v1\"") })
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let started = std::time::Instant::now();
    let web = UreqWeb::with_idle(std::time::Duration::from_secs(2));
    let err = run(&web, &file(&s), None, &part, None, &mut None, &CancelToken::new(), &mut |_, _| {}).unwrap_err();
    assert_eq!(err.kind, FailureKind::Connection);
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 10_000);
}

#[test]
fn a_part_that_already_holds_every_byte_is_complete() {
    let content = data(5_000);
    let (s, _) = world(content.clone(), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, &content).unwrap();
    let out = go(&s, &part, Some("\"v1\""), None).unwrap();
    assert_eq!(out, Outcome::Complete { total: 5_000, etag: Some("\"v1\"".into()) });
    assert_eq!(std::fs::read(&part).unwrap(), content);
}

#[test]
fn progress_is_reported_as_bytes_arrive() {
    let (s, _) = world(data(2_500_000), |_, _, _| None);
    let dir = tempfile::tempdir().unwrap();
    let seen = Mutex::new(Vec::new());
    run(&UreqWeb::new(), &file(&s), None, &dir.path().join("x.part"), None, &mut None, &CancelToken::new(), &mut |d, t| {
        seen.lock().unwrap().push((d, t))
    })
    .unwrap();
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.first(), Some(&(0, Some(2_500_000))));
    assert_eq!(seen.last(), Some(&(2_500_000, Some(2_500_000))));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
}

#[test]
fn a_storage_that_sends_more_than_the_file_is_stopped_at_its_size() {
    // No length up front: the size is only known by counting.
    let (s, _) = world(data(10_000), |_, _, _| {
        Some(Canned { chunked: true, ..Canned::new(200).with_body(&data(3_000_000)) })
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let most = std::sync::atomic::AtomicU64::new(0);
    let err = run(&UreqWeb::new(), &file(&s), None, &part, None, &mut None, &CancelToken::new(), &mut |d, _| {
        most.fetch_max(d, Ordering::SeqCst);
    })
    .unwrap_err();
    assert_eq!(err.kind, FailureKind::Mismatch, "{err:?}");
    assert!(most.load(Ordering::SeqCst) <= 10_000, "the part grew past the file's size");
    assert!(!part.exists(), "the oversized part is deleted");
}

#[test]
fn a_stated_length_over_the_files_size_is_refused_before_a_byte_is_kept() {
    let (s, _) = world(data(10_000), |_, _, _| Some(Canned::new(200).with_body(&data(20_000))));
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let mut wrote = false;
    let err = run(&UreqWeb::new(), &file(&s), None, &part, None, &mut None, &CancelToken::new(), &mut |_, _| wrote = true)
        .unwrap_err();
    assert_eq!(err.kind, FailureKind::Mismatch);
    assert!(!wrote && !part.exists());
}

#[test]
fn a_compressed_answer_is_refused_and_never_unpacked() {
    // A small compressed answer could unpack to fill the drive.
    let (s, _) = world(data(10_000), |_, _, _| {
        Some(Canned::new(200).with_header("content-encoding", "gzip").with_body(&data(500)))
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let err = go(&s, &part, None, None).unwrap_err();
    assert!(err.message.contains("compressed"), "{}", err.message);
    assert!(!part.exists() || std::fs::metadata(&part).unwrap().len() == 0);
}

#[test]
fn a_hugging_face_file_shorter_than_its_stated_size_is_not_the_file() {
    let (s, _) = world(data(10_000), |_, _, _| Some(Canned::new(200).with_body(&data(9_000))));
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let err = go(&s, &part, None, None).unwrap_err();
    assert_eq!(err.kind, FailureKind::Mismatch);
    assert!(!part.exists());
}

