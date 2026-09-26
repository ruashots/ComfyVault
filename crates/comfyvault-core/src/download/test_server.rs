//! A web server on this computer, standing in for both sites and their
//! storage.
//!
//! The download code is tested against it over a real socket, so what runs is
//! the connection that ships: real headers, real ranges, real redirects, and a
//! connection that really drops part way.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    /// The path and query, as sent.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
}

/// An answer, chosen by the test.
#[derive(Debug, Clone, Default)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Promise the whole body, send this many bytes of it, then close.
    pub cut_after: Option<usize>,
    /// Send this many bytes, then say nothing for this long.
    pub stall_after: Option<(usize, std::time::Duration)>,
}

impl Canned {
    pub fn new(status: u16) -> Self {
        Self { status, ..Default::default() }
    }

    pub fn json(status: u16, body: &str) -> Self {
        Self::new(status).with_header("content-type", "application/json").with_body(body.as_bytes())
    }

    pub fn redirect(to: &str) -> Self {
        Self::new(302).with_header("location", to)
    }

    pub fn with_header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }

    pub fn with_body(mut self, b: &[u8]) -> Self {
        self.body = b.to_vec();
        self
    }

    /// Serves `full` the way a storage service does: a range when asked for
    /// one, the whole file when `If-Range` names another version.
    pub fn file(req: &Req, full: &[u8], etag: &str) -> Self {
        let wanted_start = req
            .header("range")
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.strip_suffix('-'))
            .and_then(|n| n.parse::<usize>().ok());
        let same_version = req.header("if-range").map(|v| v == etag).unwrap_or(true);
        match wanted_start {
            Some(start) if same_version && start < full.len() => Self::new(206)
                .with_header("etag", etag)
                .with_header("content-range", &format!("bytes {start}-{}/{}", full.len() - 1, full.len()))
                .with_body(&full[start..]),
            Some(start) if same_version && start >= full.len() => {
                Self::new(416).with_header("content-range", &format!("bytes */{}", full.len()))
            }
            _ => Self::new(200).with_header("etag", etag).with_body(full),
        }
    }
}

pub type Handler = Arc<dyn Fn(&Req) -> Canned + Send + Sync>;

pub struct Server {
    pub base: String,
    requests: Arc<Mutex<Vec<Req>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start(handler: impl Fn(&Req) -> Canned + Send + Sync + 'static) -> Self {
        let handler: Handler = Arc::new(handler);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a test port");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (requests, stop) = (requests.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let (handler, requests) = (handler.clone(), requests.clone());
                            std::thread::spawn(move || serve(stream, &handler, &requests));
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                    }
                }
            });
        }
        Self { base, requests, stop }
    }

    pub fn requests(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve(stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Req>>) {
    stream.set_nonblocking(false).ok();
    let Some(req) = read_request(&stream) else { return };
    requests.lock().unwrap().push(req.clone());
    let answer = handler(&req);
    let mut out = stream;
    let mut head = format!("HTTP/1.1 {} X\r\n", answer.status);
    for (k, v) in &answer.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", answer.body.len()));
    if out.write_all(head.as_bytes()).is_err() || req.method == "HEAD" {
        return;
    }
    let body = &answer.body;
    if let Some(n) = answer.cut_after {
        let _ = out.write_all(&body[..n.min(body.len())]);
        let _ = out.flush();
        let _ = out.shutdown(std::net::Shutdown::Both);
        return;
    }
    if let Some((n, wait)) = answer.stall_after {
        let _ = out.write_all(&body[..n.min(body.len())]);
        let _ = out.flush();
        std::thread::sleep(wait);
        return;
    }
    let _ = out.write_all(body);
    let _ = out.flush();
}

fn read_request(stream: &TcpStream) -> Option<Req> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(Req { method, target, headers, body: String::from_utf8_lossy(&body).into_owned() })
}
