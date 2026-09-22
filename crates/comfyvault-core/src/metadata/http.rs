//! The HTTP transport, behind a trait.
//!
//! The trait exists so the Civitai client's real logic, which is URL building,
//! response parsing, batching and back off, is tested without the network.
//! The tests do not use a hand-written fake response though: they start a real
//! server on a real socket and drive the real [`UreqTransport`] at it, so the
//! client under test is the one that ships.
//!
//! # Why the operating system's TLS
//!
//! On Windows this resolves to the system's own TLS, which uses the certificate
//! store Windows already trusts. A company proxy or a custom certificate
//! authority therefore works without the person configuring anything.

use std::time::Duration;

use crate::error::{ErrorCode, Result, VaultError};

/// What came back.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Somewhere to send a request.
pub trait HttpTransport: Send + Sync {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse>;
    fn post_json(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<HttpResponse>;
}

/// How long to wait before giving up on a lookup.
///
/// Short on purpose. A metadata lookup is optional, so a slow network must
/// never leave the person looking at a frozen window.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The real transport.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("ComfyVault/", env!("CARGO_PKG_VERSION")))
            .build();
        Self { agent: config.into() }
    }
}

/// Turns any transport failure into the one optional-feature error.
///
/// Nothing here ever stops a scan, a plan or an apply. A lookup that fails is a
/// lookup that did not happen.
fn offline(detail: impl std::fmt::Display) -> VaultError {
    VaultError::new(
        ErrorCode::NetworkUnavailable,
        "Could not reach Civitai, so model details are not available right now. Everything else still works.",
    )
    .with_detail(detail.to_string())
}

impl HttpTransport for UreqTransport {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse> {
        let mut req = self.agent.get(url);
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        match req.call() {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let body = resp.body_mut().read_to_string().map_err(offline)?;
                Ok(HttpResponse { status, body })
            }
            // A 404 is an answer, not a failure: it means no model matches.
            Err(ureq::Error::StatusCode(code)) => Ok(HttpResponse {
                status: code,
                body: String::new(),
            }),
            Err(e) => Err(offline(e)),
        }
    }

    fn post_json(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<HttpResponse> {
        let mut req = self.agent.post(url).header("content-type", "application/json");
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        match req.send(body) {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let body = resp.body_mut().read_to_string().map_err(offline)?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::StatusCode(code)) => Ok(HttpResponse {
                status: code,
                body: String::new(),
            }),
            Err(e) => Err(offline(e)),
        }
    }
}

/// A transport that always fails, for proving the offline path.
pub struct OfflineTransport;

impl HttpTransport for OfflineTransport {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpResponse> {
        Err(offline("the network is switched off for this test"))
    }
    fn post_json(&self, _url: &str, _h: &[(String, String)], _b: &str) -> Result<HttpResponse> {
        Err(offline("the network is switched off for this test"))
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod test_server {
    //! A real HTTP server on a real socket.
    //!
    //! The Civitai client is tested against this rather than against a fake
    //! transport, so the code under test includes the actual request building,
    //! the actual socket, and the actual response reading.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// What the server answers with, chosen by the test.
    pub type Handler = Arc<dyn Fn(&Request) -> (u16, String) + Send + Sync>;

    #[derive(Debug, Clone)]
    pub struct Request {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Request {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    pub struct TestServer {
        pub base_url: String,
        requests: Arc<Mutex<Vec<Request>>>,
        hits: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        pub fn start(handler: Handler) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a test port");
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();

            let requests = Arc::new(Mutex::new(Vec::new()));
            let hits = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));

            let handle = {
                let (requests, hits, stop) = (requests.clone(), hits.clone(), stop.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                stream.set_nonblocking(false).ok();
                                if let Some(req) = read_request(&stream) {
                                    requests.lock().unwrap().push(req.clone());
                                    hits.fetch_add(1, Ordering::SeqCst);
                                    let (status, body) = handler(&req);
                                    write_response(stream, status, &body);
                                }
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(std::time::Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };

            Self {
                base_url: format!("http://127.0.0.1:{port}"),
                requests,
                hits,
                stop,
                handle: Some(handle),
            }
        }

        pub fn request_count(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }

        pub fn requests(&self) -> Vec<Request> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    fn read_request(stream: &TcpStream) -> Option<Request> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut parts = line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();

        let mut headers = Vec::new();
        let mut content_length = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).ok()? == 0 {
                break;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                let (k, v) = (k.trim().to_string(), v.trim().to_string());
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse().unwrap_or(0);
                }
                headers.push((k, v));
            }
        }

        let mut body = String::new();
        if content_length > 0 {
            let mut buf = vec![0u8; content_length];
            reader.read_exact(&mut buf).ok()?;
            body = String::from_utf8_lossy(&buf).to_string();
        }
        Some(Request { method, path, headers, body })
    }

    fn write_response(mut stream: TcpStream, status: u16, body: &str) {
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Status",
        };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::*;
    use super::*;
    use std::sync::Arc;

    #[test]
    fn the_real_client_talks_to_a_real_server() {
        let server = TestServer::start(Arc::new(|req: &Request| {
            assert_eq!(req.method, "GET");
            (200, format!("{{\"path\":\"{}\"}}", req.path))
        }));
        let t = UreqTransport::new();
        let r = t.get(&format!("{}/api/v1/hello", server.base_url), &[]).unwrap();
        assert_eq!(r.status, 200);
        assert!(r.body.contains("/api/v1/hello"));
        assert!(r.is_success());
    }

    #[test]
    fn a_404_comes_back_as_an_answer_not_an_error() {
        // Civitai answers 404 for a file it does not know. That is the normal
        // outcome for a local model, so it must not read as a failure.
        let server = TestServer::start(Arc::new(|_: &Request| {
            (404, "{\"error\":\"Model not found\"}".to_string())
        }));
        let t = UreqTransport::new();
        let r = t.get(&format!("{}/missing", server.base_url), &[]).unwrap();
        assert_eq!(r.status, 404);
        assert!(!r.is_success());
    }

    #[test]
    fn headers_reach_the_server() {
        let server = TestServer::start(Arc::new(|req: &Request| {
            let auth = req.header("authorization").unwrap_or("none").to_string();
            (200, format!("{{\"auth\":\"{auth}\"}}"))
        }));
        let t = UreqTransport::new();
        let r = t
            .get(
                &format!("{}/x", server.base_url),
                &[("Authorization".to_string(), "Bearer abc123".to_string())],
            )
            .unwrap();
        assert!(r.body.contains("Bearer abc123"));
    }

    #[test]
    fn a_post_sends_its_body_and_its_content_type() {
        let server = TestServer::start(Arc::new(|req: &Request| {
            assert_eq!(req.method, "POST");
            let ct = req.header("content-type").unwrap_or("").to_string();
            (200, format!("{{\"ct\":\"{ct}\",\"body\":{}}}", req.body))
        }));
        let t = UreqTransport::new();
        let r = t
            .post_json(&format!("{}/batch", server.base_url), &[], "[\"AA\",\"BB\"]")
            .unwrap();
        assert!(r.body.contains("application/json"), "the content type is required by the API");
        assert!(r.body.contains("\"AA\""));
    }

    #[test]
    fn a_server_that_is_not_there_reports_a_network_problem_not_a_crash() {
        let t = UreqTransport::new();
        // Port 1 is reserved and nothing listens on it.
        let err = t.get("http://127.0.0.1:1/nothing", &[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
        assert!(err.message.contains("Everything else still works"));
    }

    #[test]
    fn the_offline_transport_always_reports_being_offline() {
        let t = OfflineTransport;
        assert_eq!(t.get("http://x/", &[]).unwrap_err().code, ErrorCode::NetworkUnavailable);
        assert_eq!(
            t.post_json("http://x/", &[], "[]").unwrap_err().code,
            ErrorCode::NetworkUnavailable
        );
    }
}
