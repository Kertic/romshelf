//! Self-update of the AppImage in place.
//!
//! A release carries the AppImage, `romshelf-update.json` (version, SHA-256, size) and
//! `romshelf-update.json.sig` (Ed25519 over the manifest, base64). The public key is built
//! into Romshelf. The file is replaced with a rename, so every Steam shortcut keeps working,
//! and the previous build is kept beside it for `romshelf update --rollback`.

use crate::clock::Clock;
use crate::http::{HttpRequest, HttpTransport};
use crate::{Error, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

pub const FEED_URL: &str = "https://api.github.com/repos/Kertic/romshelf/releases/latest";
pub const APPIMAGE_ASSET: &str = "romshelf.AppImage";
pub const MANIFEST_ASSET: &str = "romshelf-update.json";
pub const SIGNATURE_ASSET: &str = "romshelf-update.json.sig";
/// Seconds between automatic checks.
pub const CHECK_INTERVAL: i64 = 24 * 60 * 60;
/// Token store key of the optional GitHub token (needed while the repository is private).
pub const GITHUB_TOKEN_KEY: &str = "github_token";

/// The release signing key's public half, base64. Empty in builds without update signing.
const PUBLIC_KEY_B64: &str = include_str!("../update-key.pub");

/// The built-in public key, or all zeros when this build has none.
pub fn builtin_public_key() -> [u8; 32] {
    STANDARD
        .decode(PUBLIC_KEY_B64.trim())
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        .unwrap_or([0; 32])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// "1.2.3" or "v1.2.3".
    pub fn parse(s: &str) -> Result<Version> {
        let bad = || Error::Invalid(format!("not a version: {s:?}"));
        let parts: Vec<u64> = s
            .trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().map_err(|_| bad()))
            .collect::<Result<_>>()?;
        match parts.as_slice() {
            [a, b, c] => Ok(Version(*a, *b, *c)),
            _ => Err(bad()),
        }
    }

    /// This build's version.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Outcome {
    /// This build has no update key, or does not run from an AppImage.
    NotConfigured,
    /// Checked less than a day ago.
    NotDue,
    /// A game is starting or running; try again next time.
    LaunchInProgress,
    UpToDate,
    /// The new build is in place and runs from the next start.
    Updated(Version),
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    /// When the feed was last read successfully.
    last_check: i64,
    /// A version rolled back from; never installed again automatically.
    skipped: Option<Version>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    sha256: String,
    size: u64,
}

#[derive(Clone)]
pub struct Updater {
    pub http: Arc<dyn HttpTransport>,
    pub clock: Arc<dyn Clock>,
    pub feed_url: String,
    pub public_key: [u8; 32],
    /// GitHub token for a private repository.
    pub token: Option<String>,
    pub current: Version,
    /// The running AppImage, `~/Applications/romshelf.AppImage`.
    pub install: PathBuf,
    pub state_file: PathBuf,
    /// Present while `romshelf launch` is running; holds its process id.
    pub launch_lock: PathBuf,
}

impl Updater {
    fn sibling(&self, suffix: &str) -> PathBuf {
        let name = self
            .install
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| APPIMAGE_ASSET.into());
        self.install.with_file_name(format!("{name}.{suffix}"))
    }

    pub fn previous_path(&self) -> PathBuf {
        self.sibling("previous")
    }

    fn load_state(&self) -> State {
        std::fs::read(&self.state_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &State) -> Result<()> {
        if let Some(dir) = self.state_file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.state_file, serde_json::to_vec(state)?)?;
        Ok(())
    }

    fn launch_in_progress(&self) -> bool {
        let Ok(text) = std::fs::read_to_string(&self.launch_lock) else {
            return false;
        };
        // A lock left behind by a crashed launch names a process that no longer exists.
        #[cfg(target_os = "linux")]
        if let Ok(pid) = text.trim().parse::<u32>() {
            return std::path::Path::new(&format!("/proc/{pid}")).exists();
        }
        let _ = text;
        true
    }

    fn get(&self, url: &str, accept: &str) -> Result<Vec<u8>> {
        let mut req = HttpRequest::new("GET", url)
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = self.http.send(&req)?;
        match resp.status {
            200..=299 => Ok(resp.body),
            404 if self.token.is_none() => Err(Error::External(
                "The update feed wasn't found. While the Romshelf repository is private, give \
                 Romshelf a read-only GitHub token with `romshelf update --set-token`."
                    .into(),
            )),
            401 | 403 | 404 => Err(Error::External(format!(
                "GitHub refused the update request ({}). Check the token with \
                 `romshelf update --set-token`.",
                resp.status
            ))),
            status => Err(Error::External(format!(
                "GitHub answered {status} to the update request. Try again later."
            ))),
        }
    }

    /// Checks the feed and installs a newer signed build. `force` ignores the daily limit.
    pub fn run(&self, force: bool) -> Result<Outcome> {
        if self.public_key == [0; 32] {
            return Ok(Outcome::NotConfigured);
        }
        let mut state = self.load_state();
        if !force && self.clock.now() - state.last_check < CHECK_INTERVAL {
            return Ok(Outcome::NotDue);
        }
        if self.launch_in_progress() {
            return Ok(Outcome::LaunchInProgress);
        }

        let release: Release =
            serde_json::from_slice(&self.get(&self.feed_url, "application/vnd.github+json")?)?;
        let version = Version::parse(&release.tag_name)?;
        if version <= self.current || state.skipped == Some(version) {
            state.last_check = self.clock.now();
            self.save_state(&state)?;
            return Ok(Outcome::UpToDate);
        }

        let asset = |name: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.url.as_str())
                .ok_or_else(|| {
                    Error::External(format!("Release {version} has no {name}. Try again later."))
                })
        };
        let (file_url, manifest_url, sig_url) = (
            asset(APPIMAGE_ASSET)?,
            asset(MANIFEST_ASSET)?,
            asset(SIGNATURE_ASSET)?,
        );
        let octet = "application/octet-stream";
        let manifest_bytes = self.get(manifest_url, octet)?;
        let sig_text = self.get(sig_url, octet)?;
        self.verify_signature(&manifest_bytes, &sig_text)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
        if Version::parse(&manifest.version)? != version {
            return Err(Error::External(format!(
                "Release {version}'s update details are for version {}. Not installing it.",
                manifest.version
            )));
        }

        let file = self.get(file_url, octet)?;
        if file.len() as u64 != manifest.size || hex(&Sha256::digest(&file)) != manifest.sha256 {
            return Err(Error::External(format!(
                "The download of Romshelf {version} failed its checksum. Nothing was changed."
            )));
        }
        if !file.starts_with(b"\x7fELF") {
            return Err(Error::External(format!(
                "The download of Romshelf {version} isn't an AppImage. Nothing was changed."
            )));
        }
        self.install_file(&file)?;
        // Only a finished check counts; a failure is retried at the next start.
        state.last_check = self.clock.now();
        self.save_state(&state)?;
        Ok(Outcome::Updated(version))
    }

    fn verify_signature(&self, manifest: &[u8], sig_text: &[u8]) -> Result<()> {
        let invalid = || {
            Error::External(
                "The update's signature doesn't match Romshelf's key. Nothing was changed.".into(),
            )
        };
        let key = VerifyingKey::from_bytes(&self.public_key).map_err(|_| invalid())?;
        let raw = STANDARD
            .decode(String::from_utf8_lossy(sig_text).trim())
            .map_err(|_| invalid())?;
        let sig = Signature::from_slice(&raw).map_err(|_| invalid())?;
        key.verify_strict(manifest, &sig).map_err(|_| invalid())
    }

    /// Writes beside the current file, then swaps with renames. The old build becomes
    /// `.previous`; if the second rename fails, it is put back.
    fn install_file(&self, bytes: &[u8]) -> Result<()> {
        let download = self.sibling("download");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&download)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&download, std::fs::Permissions::from_mode(0o755))?;
        }
        let previous = self.previous_path();
        if self.install.exists() {
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(&self.install, &previous)?;
        }
        if let Err(e) = std::fs::rename(&download, &self.install) {
            if previous.exists() {
                let _ = std::fs::rename(&previous, &self.install);
            }
            return Err(e.into());
        }
        Ok(())
    }

    /// Puts the previous build back and stops this version from being installed again.
    pub fn rollback(&self) -> Result<()> {
        let previous = self.previous_path();
        if !previous.exists() {
            return Err(Error::NotFound(
                "No previous version of Romshelf is kept, so there is nothing to roll back to."
                    .into(),
            ));
        }
        let rejected = self.sibling("rolled-back");
        if self.install.exists() {
            std::fs::rename(&self.install, &rejected)?;
        }
        if let Err(e) = std::fs::rename(&previous, &self.install) {
            let _ = std::fs::rename(&rejected, &self.install);
            return Err(e.into());
        }
        let _ = std::fs::remove_file(&rejected);
        let mut state = self.load_state();
        state.skipped = Some(self.current);
        self.save_state(&state)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The updater for the running AppImage, or `None` when Romshelf isn't running from one
/// (a development build). The AppImage runtime sets `APPIMAGE` to the file's path.
pub fn for_running_appimage(
    paths: &crate::paths::Paths,
    tokens: &dyn crate::tokens::TokenStore,
) -> Result<Option<Updater>> {
    let Some(install) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return Ok(None);
    };
    Ok(Some(Updater {
        http: Arc::new(crate::http::ReqwestTransport::new()?),
        clock: Arc::new(crate::clock::SystemClock),
        feed_url: FEED_URL.into(),
        public_key: builtin_public_key(),
        token: tokens.get(GITHUB_TOKEN_KEY)?,
        current: Version::current(),
        install,
        state_file: paths.update_state_file(),
        launch_lock: paths.launch_lock(),
    }))
}
