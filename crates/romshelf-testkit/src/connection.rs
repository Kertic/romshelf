//! A `Connection` on the fake Drive, with sign-in played through a real loopback listener.

use crate::{FakeClock, FakeDrive, FakeHttp, MemoryTokenStore};
use romshelf_core::auth::{ClientConfig, GoogleAuth, DRIVE_SCOPE};
use romshelf_core::connection::Connection;
use romshelf_core::Result;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

pub struct FakeConnection {
    pub connection: Connection,
    pub http: Arc<FakeHttp>,
    pub drive: Arc<FakeDrive>,
}

impl FakeConnection {
    /// A fake client is already set; nobody is signed in.
    pub fn new(settings_path: PathBuf) -> FakeConnection {
        let http = Arc::new(FakeHttp::default());
        let tokens = Arc::new(MemoryTokenStore::default());
        let auth = Arc::new(GoogleAuth::new(
            http.clone(),
            tokens,
            Arc::new(FakeClock::at(1_800_000_000)),
        ));
        auth.set_client(&ClientConfig {
            client_id: "fake.apps.googleusercontent.com".into(),
            client_secret: None,
        })
        .expect("memory store");
        let drive = Arc::new(FakeDrive::default());
        FakeConnection {
            connection: Connection::new(auth, drive.clone(), settings_path),
            http,
            drive,
        }
    }

    /// Queues Google's answer to the next code exchange.
    pub fn script_login(&self) {
        script_login(&self.http);
    }
}

/// Queues Google's answer to the next code exchange on `http`.
pub fn script_login(http: &FakeHttp) {
    http.push(
        200,
        &format!(
            r#"{{"access_token":"fake-access","expires_in":3600,"refresh_token":"fake-refresh","scope":"{DRIVE_SCOPE}","token_type":"Bearer"}}"#
        ),
    );
}

/// Plays the browser for a consent URL: sends Google's redirect, with a code, to the loopback port.
pub fn simulate_browser(consent_url: &str) -> Result<()> {
    let q: HashMap<String, String> = url::Url::parse(consent_url)
        .map_err(|e| romshelf_core::Error::Invalid(e.to_string()))?
        .query_pairs()
        .into_owned()
        .collect();
    let redirect = url::Url::parse(&q["redirect_uri"])
        .map_err(|e| romshelf_core::Error::Invalid(e.to_string()))?;
    let port = redirect.port().unwrap_or(80);
    let state = q["state"].clone();
    std::thread::spawn(move || {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = write!(
                s,
                "GET /?state={state}&code=fake-code HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
            );
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
        }
    });
    Ok(())
}
