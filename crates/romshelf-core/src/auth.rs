//! Google sign-in: desktop loopback redirect with PKCE, token exchange and refresh.
//!
//! The client ID and secret are the user's own, stored with `romshelf auth set-client`.

use crate::clock::Clock;
use crate::http::{HttpRequest, HttpTransport};
use crate::tokens::TokenStore;
use crate::{Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";
/// Full Drive access: `drive.file` cannot see ROMs uploaded through the Drive website.
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

/// Token store keys.
pub const CLIENT_KEY: &str = "google_client";
pub const TOKEN_KEY: &str = "google_token";

/// Refresh this many seconds before Google's stated expiry.
const EXPIRY_MARGIN: i64 = 60;

const NO_CLIENT: &str = "Romshelf needs your Google Cloud client first. On the Deck, run \
`romshelf auth set-client --file <downloaded client JSON>` (see docs/google-setup.md).";
const NOT_SIGNED_IN: &str = "Not signed in to Google. Sign in to connect Google Drive.";
const EXPIRED: &str = "Your Google sign-in has expired or was removed. Sign in again.";
const NO_DRIVE_SCOPE: &str = "Romshelf wasn't given access to Google Drive. Sign in again and \
allow access to your Drive files.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    pub client_id: String,
    /// Google issues a secret to desktop clients and still expects it, although it is not secret.
    pub client_secret: Option<String>,
}

impl ClientConfig {
    /// Parses the client JSON downloaded from Google Cloud ("Desktop app" type).
    pub fn from_google_json(bytes: &[u8]) -> Result<ClientConfig> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|_| Error::Invalid("That file isn't a Google client JSON file.".into()))?;
        if value.get("web").is_some() {
            return Err(Error::Invalid(
                "That is a web application client. Create a client of type \"Desktop app\" instead."
                    .into(),
            ));
        }
        let installed = value.get("installed").ok_or_else(|| {
            Error::Invalid("That file isn't a Google desktop client JSON file.".into())
        })?;
        let field = |name: &str| {
            installed
                .get(name)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };
        let client_id = field("client_id")
            .ok_or_else(|| Error::Invalid("The client JSON has no client_id.".into()))?;
        Ok(ClientConfig {
            client_id,
            client_secret: field("client_secret"),
        })
    }

    pub fn load(store: &dyn TokenStore) -> Result<Option<ClientConfig>> {
        match store.get(CLIENT_KEY)? {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    pub fn save(&self, store: &dyn TokenStore) -> Result<()> {
        store.set(CLIENT_KEY, &serde_json::to_string(self)?)
    }
}

/// PKCE verifier and its S256 challenge (RFC 7636).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn from_verifier(verifier: &str) -> Pkce {
        let digest = Sha256::digest(verifier.as_bytes());
        Pkce {
            verifier: verifier.into(),
            challenge: URL_SAFE_NO_PAD.encode(digest),
        }
    }

    pub fn generate() -> Result<Pkce> {
        Ok(Pkce::from_verifier(&random_token(32)?))
    }
}

/// `bytes` random bytes, base64url without padding.
pub fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| Error::External(format!("no randomness: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn authorization_url(
    client: &ClientConfig,
    redirect_uri: &str,
    pkce: &Pkce,
    state: &str,
) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", DRIVE_SCOPE)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        // A refresh token, and a fresh one on every sign-in, so signing in again always works.
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .finish();
    format!("{AUTH_ENDPOINT}?{query}")
}

/// One-shot HTTP listener on 127.0.0.1 that receives Google's redirect.
pub struct Loopback {
    listener: TcpListener,
}

impl Loopback {
    pub fn bind() -> Result<Loopback> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        Ok(Loopback { listener })
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }

    /// Waits for the redirect and returns the authorization code. Requests for other paths
    /// (a browser asking for `/favicon.ico`) are answered and ignored.
    pub fn wait_for_code(
        &self,
        expected_state: &str,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(
                    "Sign-in wasn't finished in time. Try again.".into(),
                ));
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = handle_redirect(stream, expected_state) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// `None` when the request was not the redirect and waiting should continue.
fn handle_redirect(mut stream: TcpStream, expected_state: &str) -> Option<Result<String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let target = head
        .lines()
        .next()
        .and_then(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("GET"))
                .then(|| parts.next())
                .flatten()
        })
        .unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/" {
        respond(&mut stream, 404, "Not found.");
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        respond(
            &mut stream,
            400,
            "This sign-in page doesn't match Romshelf's request. Start sign-in again in Romshelf.",
        );
        return Some(Err(Error::Auth(
            "The sign-in reply didn't match Romshelf's request. Try again.".into(),
        )));
    }
    if let Some(error) = error {
        respond(
            &mut stream,
            200,
            "Sign-in was cancelled. You can close this tab and return to Romshelf.",
        );
        return Some(Err(Error::Auth(format!(
            "Google sign-in didn't finish ({error}). Try again."
        ))));
    }
    match code {
        Some(code) => {
            respond(
                &mut stream,
                200,
                "Signed in to Romshelf. You can close this tab and return to Romshelf.",
            );
            Some(Ok(code))
        }
        None => {
            respond(&mut stream, 400, "No sign-in code was received.");
            Some(Err(Error::Auth(
                "Google didn't send a sign-in code. Try again.".into(),
            )))
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Romshelf</title></head>\
         <body style=\"background:#10161B;color:#E8EEF2;font-family:sans-serif;padding:40px\">\
         <p style=\"font-size:20px\">{message}</p></body></html>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

/// What is kept in the token store under `TOKEN_KEY`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    /// Seconds since the epoch.
    pub expires_at: i64,
    pub refresh_token: String,
    pub scope: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
    #[serde(default)]
    scope: String,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
}

/// Holds the user's tokens and keeps the access token fresh.
pub struct GoogleAuth {
    http: Arc<dyn HttpTransport>,
    tokens: Arc<dyn TokenStore>,
    clock: Arc<dyn Clock>,
    refresh_lock: Mutex<()>,
}

impl GoogleAuth {
    pub fn new(
        http: Arc<dyn HttpTransport>,
        tokens: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
    ) -> GoogleAuth {
        GoogleAuth {
            http,
            tokens,
            clock,
            refresh_lock: Mutex::new(()),
        }
    }

    pub fn client(&self) -> Result<ClientConfig> {
        ClientConfig::load(self.tokens.as_ref())?.ok_or_else(|| Error::Auth(NO_CLIENT.into()))
    }

    /// Stores the client. A different client id makes existing tokens useless, so they go.
    pub fn set_client(&self, client: &ClientConfig) -> Result<()> {
        let previous = ClientConfig::load(self.tokens.as_ref())?;
        if previous.is_some_and(|p| p.client_id != client.client_id) {
            self.tokens.delete(TOKEN_KEY)?;
        }
        client.save(self.tokens.as_ref())
    }

    pub fn has_client(&self) -> Result<bool> {
        Ok(ClientConfig::load(self.tokens.as_ref())?.is_some())
    }

    pub fn stored(&self) -> Result<Option<StoredToken>> {
        match self.tokens.get(TOKEN_KEY)? {
            Some(s) => Ok(serde_json::from_str(&s).ok()),
            None => Ok(None),
        }
    }

    pub fn signed_in(&self) -> Result<bool> {
        Ok(self.stored()?.is_some())
    }

    /// Runs the whole browser sign-in. `open` receives the consent URL; it may open a browser
    /// or show the address. Blocks until the redirect arrives, `timeout` passes or `cancel` is set.
    pub fn login(
        &self,
        open: &dyn Fn(&str) -> Result<()>,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<()> {
        let client = self.client()?;
        let pkce = Pkce::generate()?;
        let state = random_token(16)?;
        let loopback = Loopback::bind()?;
        let redirect_uri = loopback.redirect_uri();
        open(&authorization_url(&client, &redirect_uri, &pkce, &state))?;
        let code = loopback.wait_for_code(&state, timeout, cancel)?;
        self.exchange_code(&client, &code, &pkce.verifier, &redirect_uri)
    }

    pub fn exchange_code(
        &self,
        client: &ClientConfig,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<()> {
        let mut fields = vec![
            ("client_id", client.client_id.as_str()),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];
        if let Some(secret) = &client.client_secret {
            fields.push(("client_secret", secret));
        }
        let resp = self
            .http
            .send(&HttpRequest::new("POST", TOKEN_ENDPOINT).form(&fields))?;
        if !resp.ok() {
            return Err(Error::Auth(format!(
                "Google refused the sign-in ({}). Check the client with `romshelf auth set-client` and try again.",
                token_error(&resp.body)
            )));
        }
        let t: TokenResponse = serde_json::from_slice(&resp.body)?;
        if !t.scope.split_whitespace().any(|s| s == DRIVE_SCOPE) {
            return Err(Error::Auth(NO_DRIVE_SCOPE.into()));
        }
        let refresh_token = t.refresh_token.ok_or_else(|| {
            Error::Auth("Google didn't grant offline access. Sign in again.".into())
        })?;
        self.save(&StoredToken {
            access_token: t.access_token,
            expires_at: self.clock.now() + t.expires_in,
            refresh_token,
            scope: t.scope,
        })
    }

    /// A valid access token, refreshing first when it is about to expire.
    pub fn access_token(&self) -> Result<String> {
        let _guard = self.refresh_lock.lock().unwrap_or_else(|e| e.into_inner());
        let token = self
            .stored()?
            .ok_or_else(|| Error::Auth(NOT_SIGNED_IN.into()))?;
        if token.expires_at - EXPIRY_MARGIN > self.clock.now() {
            return Ok(token.access_token);
        }
        self.refresh_locked(token)
    }

    /// Called when Google rejected `rejected` with 401. Refreshes unless another caller already did.
    pub fn refresh_after_rejection(&self, rejected: &str) -> Result<String> {
        let _guard = self.refresh_lock.lock().unwrap_or_else(|e| e.into_inner());
        let token = self
            .stored()?
            .ok_or_else(|| Error::Auth(NOT_SIGNED_IN.into()))?;
        if token.access_token != rejected {
            return Ok(token.access_token);
        }
        self.refresh_locked(token)
    }

    fn refresh_locked(&self, token: StoredToken) -> Result<String> {
        let client = self.client()?;
        let mut fields = vec![
            ("client_id", client.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", token.refresh_token.as_str()),
        ];
        if let Some(secret) = &client.client_secret {
            fields.push(("client_secret", secret));
        }
        let resp = self
            .http
            .send(&HttpRequest::new("POST", TOKEN_ENDPOINT).form(&fields))?;
        if !resp.ok() {
            let error = token_error(&resp.body);
            if error == "invalid_grant" || resp.status == 401 {
                // Revoked, expired (seven days in "Testing"), or the password changed.
                self.tokens.delete(TOKEN_KEY)?;
                return Err(Error::Auth(EXPIRED.into()));
            }
            return Err(Error::External(format!(
                "Google couldn't refresh the sign-in ({error}). Try again."
            )));
        }
        let t: TokenResponse = serde_json::from_slice(&resp.body)?;
        let fresh = StoredToken {
            access_token: t.access_token,
            expires_at: self.clock.now() + t.expires_in,
            refresh_token: t.refresh_token.unwrap_or(token.refresh_token),
            scope: if t.scope.is_empty() {
                token.scope
            } else {
                t.scope
            },
        };
        self.save(&fresh)?;
        Ok(fresh.access_token)
    }

    /// Revokes the grant at Google (best effort) and forgets the tokens. The client stays.
    pub fn logout(&self) -> Result<()> {
        if let Some(token) = self.stored()? {
            let _ = self.http.send(
                &HttpRequest::new("POST", REVOKE_ENDPOINT)
                    .form(&[("token", token.refresh_token.as_str())]),
            );
        }
        self.tokens.delete(TOKEN_KEY)
    }

    fn save(&self, token: &StoredToken) -> Result<()> {
        self.tokens.set(TOKEN_KEY, &serde_json::to_string(token)?)
    }
}

fn token_error(body: &[u8]) -> String {
    serde_json::from_slice::<TokenError>(body)
        .map(|e| e.error)
        .unwrap_or_else(|_| "unknown error".into())
}

/// Opens `url` in the system browser. `Error::NoBrowser` when nothing could be opened.
pub fn open_browser(url: &str) -> Result<()> {
    let mut cmd = browser_command(url);
    let mut child = cmd.spawn().map_err(|_| Error::NoBrowser)?;
    // xdg-open returns quickly; a non-zero exit means no handler was found.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(Error::NoBrowser),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(Error::NoBrowser),
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn browser_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url);
    // The AppImage's bundled libraries must not leak into the browser.
    for var in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GIO_MODULE_DIR",
        "GDK_PIXBUF_MODULE_FILE",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GSETTINGS_SCHEMA_DIR",
        "PYTHONHOME",
        "PYTHONPATH",
    ] {
        cmd.env_remove(var);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("rundll32");
    cmd.args(["url.dll,FileProtocolHandler", url]);
    cmd
}

#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    cmd.arg(url);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn pkce_matches_rfc_7636_test_vector() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_verifier_is_valid() {
        let a = Pkce::generate().unwrap();
        let b = Pkce::generate().unwrap();
        assert_ne!(a.verifier, b.verifier);
        // RFC 7636: 43 to 128 characters from the unreserved set.
        assert!((43..=128).contains(&a.verifier.len()));
        assert!(a
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
        assert_eq!(a.challenge, Pkce::from_verifier(&a.verifier).challenge);
    }

    #[test]
    fn authorization_url_asks_for_drive_with_s256() {
        let client = ClientConfig {
            client_id: "id.apps.googleusercontent.com".into(),
            client_secret: None,
        };
        let pkce = Pkce::from_verifier("v".repeat(43).as_str());
        let url = authorization_url(&client, "http://127.0.0.1:5000", &pkce, "st");
        let parsed = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], DRIVE_SCOPE);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], pkce.challenge);
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5000");
        assert_eq!(q["state"], "st");
        assert_eq!(q["access_type"], "offline");
        assert!(url.starts_with(AUTH_ENDPOINT));
    }

    #[test]
    fn client_json_parsing() {
        let ok = br#"{"installed":{"client_id":"abc.apps.googleusercontent.com","client_secret":"s","redirect_uris":["http://localhost"]}}"#;
        assert_eq!(
            ClientConfig::from_google_json(ok).unwrap(),
            ClientConfig {
                client_id: "abc.apps.googleusercontent.com".into(),
                client_secret: Some("s".into())
            }
        );
        let web = br#"{"web":{"client_id":"x"}}"#;
        assert!(ClientConfig::from_google_json(web)
            .unwrap_err()
            .to_string()
            .contains("Desktop app"));
        assert!(ClientConfig::from_google_json(b"not json").is_err());
        assert!(ClientConfig::from_google_json(br#"{"installed":{}}"#).is_err());
    }

    /// Sends a browser-style GET to the loopback listener and returns the response's status line.
    fn redirect(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out.lines().next().unwrap_or_default().to_string()
    }

    #[test]
    fn loopback_returns_the_code_for_the_right_state() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port();
        let browser = thread::spawn(move || {
            assert!(redirect(port, "/favicon.ico").contains("404"));
            redirect(port, "/?state=abc&code=4%2F0Ab&scope=x")
        });
        let code = lb
            .wait_for_code("abc", Duration::from_secs(5), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(code, "4/0Ab");
        assert!(browser.join().unwrap().contains("200"));
    }

    #[test]
    fn loopback_rejects_the_wrong_state() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port();
        let browser = thread::spawn(move || redirect(port, "/?state=evil&code=c"));
        let err = lb
            .wait_for_code("abc", Duration::from_secs(5), &AtomicBool::new(false))
            .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "{err:?}");
        assert!(browser.join().unwrap().contains("400"));
    }

    #[test]
    fn loopback_reports_a_refused_consent() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port();
        let browser = thread::spawn(move || redirect(port, "/?state=abc&error=access_denied"));
        let err = lb
            .wait_for_code("abc", Duration::from_secs(5), &AtomicBool::new(false))
            .unwrap_err();
        assert!(err.to_string().contains("access_denied"));
        browser.join().unwrap();
    }

    #[test]
    fn loopback_times_out_with_a_clear_error() {
        let lb = Loopback::bind().unwrap();
        let err = lb
            .wait_for_code("abc", Duration::from_millis(100), &AtomicBool::new(false))
            .unwrap_err();
        assert!(matches!(err, Error::Timeout(_)));
        assert_eq!(
            err.to_string(),
            "Sign-in wasn't finished in time. Try again."
        );
    }

    #[test]
    fn loopback_can_be_cancelled() {
        let lb = Loopback::bind().unwrap();
        let err = lb
            .wait_for_code("abc", Duration::from_secs(5), &AtomicBool::new(true))
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }
}
