//! The connection a download uses.
//!
//! It is not the metadata lookup's connection, for three reasons:
//!
//! * **No redirect is followed on its own.** Both sites answer a download
//!   with a redirect to a signed storage address. The engine reads the first
//!   answer itself, so it sees the site's refusal headers, and it asks the
//!   storage address with no `Authorization` header at all. A token therefore
//!   only ever goes to the site it belongs to, by construction rather than by
//!   a library default.
//! * **No limit on the whole transfer**, which for a 16 GB file on a slow
//!   line is hours, but **a limit on silence**: a read that gets nothing for
//!   [`IDLE`] ends the transfer as a dropped connection, so a stalled line
//!   never freezes the queue.
//! * A token only ever travels in a header, never in an address, so it cannot
//!   end up in an error message that quotes one.

use std::io::Read;
use std::time::Duration;

use crate::error::{ErrorCode, Result, VaultError};

/// How long a read may get nothing before the connection counts as dropped.
pub const IDLE: Duration = Duration::from_secs(30);

/// The most text read from an answer that is not the file itself.
const TEXT_LIMIT: u64 = 8 * 1024 * 1024;

/// One request.
#[derive(Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// Shows which headers are set, never the value of `Authorization`.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(k, v)| if k.eq_ignore_ascii_case("authorization") { format!("{k}: <set>") } else { format!("{k}: {v}") })
            .collect();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &without_query(&self.url))
            .field("headers", &headers)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    /// A form post, used for Hugging Face's `paths-info`.
    PostForm,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self { method: Method::Get, url: url.into(), headers: Vec::new() }
    }

    pub fn head(url: impl Into<String>) -> Self {
        Self { method: Method::Head, url: url.into(), headers: Vec::new() }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// Adds `Authorization: Bearer`, when there is a token.
    pub fn bearer(self, token: Option<&str>) -> Self {
        match token {
            Some(t) => self.header("authorization", format!("Bearer {t}")),
            None => self,
        }
    }
}

/// What came back. The body is read only if the caller wants it.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply").field("status", &self.status).finish()
    }
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body as text, up to a limit that no API answer comes near.
    pub fn text(self) -> Result<String> {
        let mut out = String::new();
        self.body
            .take(TEXT_LIMIT)
            .read_to_string(&mut out)
            .map_err(|e| unreachable_site(&e.to_string()))?;
        Ok(out)
    }

    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }
}

/// Somewhere to send a request. Tests point it at a server on this computer.
pub trait Web: Send + Sync {
    fn send(&self, req: &Request, form: Option<&str>) -> Result<Reply>;
}

/// The refusal for a site that did not answer at all.
pub fn unreachable_site(detail: &str) -> VaultError {
    VaultError::new(
        ErrorCode::NetworkUnavailable,
        "The site did not answer. Check the internet connection and try again.",
    )
    .with_detail(detail.to_string())
}

/// An address with its query and fragment removed, for a message.
///
/// A storage address carries its signature in the query, and a signature is
/// a credential for as long as it lasts.
pub fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// The real connection.
pub struct UreqWeb {
    agent: ureq::Agent,
}

impl Default for UreqWeb {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqWeb {
    pub fn new() -> Self {
        Self::with_idle(IDLE)
    }

    /// The same connection with another limit on silence, for tests.
    pub fn with_idle(idle: Duration) -> Self {
        use ureq::unversioned::resolver::DefaultResolver;
        use ureq::unversioned::transport::{Connector, DefaultConnector};

        let config = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .user_agent(concat!("ComfyVault/", env!("CARGO_PKG_VERSION")))
            .build();
        let connector = DefaultConnector::default().chain(idle::IdleLimit(idle));
        Self { agent: ureq::Agent::with_parts(config, connector, DefaultResolver::default()) }
    }
}

impl Web for UreqWeb {
    fn send(&self, req: &Request, form: Option<&str>) -> Result<Reply> {
        let result = match req.method {
            Method::Get => {
                let mut r = self.agent.get(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.call()
            }
            Method::Head => {
                let mut r = self.agent.head(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.call()
            }
            Method::PostForm => {
                let mut r = self
                    .agent
                    .post(&req.url)
                    .header("content-type", "application/x-www-form-urlencoded");
                for (k, v) in &req.headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.send(form.unwrap_or_default())
            }
        };
        let resp = result.map_err(|e| unreachable_site(&describe(&e)))?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let body = resp.into_body().into_reader();
        Ok(Reply { status, headers, body: Box::new(body) })
    }
}

/// The failure, without anything a request carried.
fn describe(e: &ureq::Error) -> String {
    match e {
        // These can quote the address, which may carry a signature.
        ureq::Error::BadUri(_) => "the address could not be used".to_string(),
        other => other.to_string(),
    }
}

/// A connector that puts a limit on every wait for data.
///
/// ureq has limits for the whole call and for the whole body, and neither
/// fits a transfer that can rightly take hours. This caps each single wait
/// instead. It uses ureq's `unversioned` transport interface, which the lock
/// file pins to the version it was written against.
mod idle {
    use std::time::Duration;

    use ureq::unversioned::transport::{
        time::Duration as UDuration, Buffers, ConnectionDetails, Connector, NextTimeout, Transport,
    };

    #[derive(Debug)]
    pub struct IdleLimit(pub Duration);

    impl Connector<Box<dyn Transport>> for IdleLimit {
        type Out = Capped;

        fn connect(
            &self,
            _details: &ConnectionDetails,
            chained: Option<Box<dyn Transport>>,
        ) -> Result<Option<Self::Out>, ureq::Error> {
            Ok(chained.map(|inner| Capped { inner, limit: self.0 }))
        }
    }

    #[derive(Debug)]
    pub struct Capped {
        inner: Box<dyn Transport>,
        limit: Duration,
    }

    impl Capped {
        fn cap(&self, t: NextTimeout) -> NextTimeout {
            let after = match t.after {
                UDuration::Exact(d) if d <= self.limit => t.after,
                _ => UDuration::Exact(self.limit),
            };
            NextTimeout { after, reason: t.reason }
        }
    }

    impl Transport for Capped {
        fn buffers(&mut self) -> &mut dyn Buffers {
            self.inner.buffers()
        }

        fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
            let t = self.cap(timeout);
            self.inner.transmit_output(amount, t)
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            let t = self.cap(timeout);
            self.inner.await_input(t)
        }

        fn is_open(&mut self) -> bool {
            self.inner.is_open()
        }

        fn is_tls(&self) -> bool {
            self.inner.is_tls()
        }
    }
}
