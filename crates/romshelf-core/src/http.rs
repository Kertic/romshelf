//! HTTP seam under the Google client, so its retry and token logic can be tested with a fake.

use crate::{Error, Result};
use std::io::{Read, Write};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new(method: &'static str, url: impl Into<String>) -> HttpRequest {
        HttpRequest {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> HttpRequest {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, content_type: &str, body: Vec<u8>) -> HttpRequest {
        self.body = body;
        self.header("Content-Type", content_type)
    }

    pub fn json(self, value: &serde_json::Value) -> HttpRequest {
        let body = value.to_string().into_bytes();
        self.body("application/json; charset=UTF-8", body)
    }

    pub fn form(self, fields: &[(&str, &str)]) -> HttpRequest {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish()
            .into_bytes();
        self.body("application/x-www-form-urlencoded", body)
    }

    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub trait HttpTransport: Send + Sync {
    /// Sends one request. A non-2xx status is a response, not an error; only failing to
    /// reach the server is an error (`Error::Network`).
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse>;

    /// Like `send`, but a 2xx body is written to `sink` as it arrives and not returned.
    /// An error body is returned as usual. A sink error made by `cancelled_write` means cancelled.
    fn send_to(&self, req: &HttpRequest, sink: &mut dyn Write) -> Result<HttpResponse> {
        let resp = self.send(req)?;
        if !resp.ok() {
            return Ok(resp);
        }
        sink.write_all(&resp.body).map_err(sink_error)?;
        Ok(HttpResponse {
            status: resp.status,
            body: Vec::new(),
        })
    }
}

pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<ReqwestTransport> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // Large downloads arrive in stage 3 and stream; until then whole bodies are small.
            .timeout(Duration::from_secs(300))
            .user_agent(concat!("romshelf/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(ReqwestTransport { client })
    }
}

/// What a download sink returns when the user cancels. (Not `ErrorKind::Interrupted`:
/// `write_all` retries those forever.)
#[derive(Debug)]
pub struct CancelledWrite;

impl std::fmt::Display for CancelledWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for CancelledWrite {}

pub fn cancelled_write() -> std::io::Error {
    std::io::Error::other(CancelledWrite)
}

/// A failed write to a download's sink: cancelled, or a disk error.
pub fn sink_error(e: std::io::Error) -> Error {
    if e.get_ref()
        .is_some_and(|inner| inner.is::<CancelledWrite>())
    {
        Error::Cancelled
    } else {
        Error::Io(e)
    }
}

impl ReqwestTransport {
    fn start(&self, req: &HttpRequest) -> Result<reqwest::blocking::Response> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let mut builder = self.client.request(method, &req.url);
        for (name, value) in &req.headers {
            builder = builder.header(name, value);
        }
        if !req.body.is_empty() || matches!(req.method, "POST" | "PATCH" | "PUT") {
            builder = builder.body(req.body.clone());
        }
        builder.send().map_err(|e| Error::Network(e.to_string()))
    }
}

impl HttpTransport for ReqwestTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let resp = self.start(req)?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .map_err(|e| Error::Network(e.to_string()))?
            .to_vec();
        Ok(HttpResponse { status, body })
    }

    fn send_to(&self, req: &HttpRequest, sink: &mut dyn Write) -> Result<HttpResponse> {
        let mut resp = self.start(req)?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp
                .bytes()
                .map_err(|e| Error::Network(e.to_string()))?
                .to_vec();
            return Ok(HttpResponse { status, body });
        }
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| Error::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            sink.write_all(&buf[..n]).map_err(sink_error)?;
        }
        Ok(HttpResponse {
            status,
            body: Vec::new(),
        })
    }
}
