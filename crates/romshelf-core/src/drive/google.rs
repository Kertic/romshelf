//! `DriveClient` over the Google Drive v3 REST API.

use super::{Account, Change, ChangePage, DriveClient, DriveFile, FOLDER_MIME};
use crate::auth::GoogleAuth;
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
const FILE_FIELDS: &str = "id,name,mimeType,parents,size,md5Checksum,trashed";
/// Attempts per request, counting the first. Rate limits, 5xx and network failures are retried.
const MAX_ATTEMPTS: u32 = 5;
const MAX_DELAY: Duration = Duration::from_secs(32);

pub struct GoogleDrive {
    auth: Arc<GoogleAuth>,
    http: Arc<dyn HttpTransport>,
    retry_delay: Duration,
}

impl GoogleDrive {
    pub fn new(auth: Arc<GoogleAuth>, http: Arc<dyn HttpTransport>) -> GoogleDrive {
        GoogleDrive {
            auth,
            http,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// First retry waits this long; each later one twice as long, up to 32 s.
    pub fn with_retry_delay(mut self, delay: Duration) -> GoogleDrive {
        self.retry_delay = delay;
        self
    }

    /// Sends an authorized request with one token refresh on 401 and retries with backoff.
    fn call(&self, build: impl Fn() -> HttpRequest) -> Result<HttpResponse> {
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            let token = self.auth.access_token()?;
            let req = build().header("Authorization", format!("Bearer {token}"));
            let failure = match self.http.send(&req) {
                Ok(resp) if resp.ok() || resp.status == 206 => return Ok(resp),
                Ok(resp) if resp.status == 401 && !refreshed => {
                    refreshed = true;
                    self.auth.refresh_after_rejection(&token)?;
                    continue;
                }
                Ok(resp) if retryable(&resp) => api_error(&resp),
                Ok(resp) => return Err(api_error(&resp)),
                Err(e @ Error::Network(_)) => e,
                Err(e) => return Err(e),
            };
            attempt += 1;
            if attempt >= MAX_ATTEMPTS {
                return Err(failure);
            }
            let delay = self
                .retry_delay
                .saturating_mul(1 << (attempt - 1))
                .min(MAX_DELAY);
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
        }
    }

    fn call_json<T: for<'de> Deserialize<'de>>(
        &self,
        build: impl Fn() -> HttpRequest,
    ) -> Result<T> {
        let resp = self.call(build)?;
        Ok(serde_json::from_slice(&resp.body)?)
    }

    fn file(&self, build: impl Fn() -> HttpRequest) -> Result<DriveFile> {
        self.call_json::<RawFile>(build).map(RawFile::into_file)
    }
}

fn url(base: &str, path: &str, query: &[(&str, &str)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in query {
        s.append_pair(k, v);
    }
    let q = s.finish();
    if q.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{q}")
    }
}

/// A path segment for a file id. Drive ids are URL-safe, but never trust that.
fn seg(id: &str) -> String {
    url::form_urlencoded::byte_serialize(id.as_bytes()).collect()
}

fn retryable(resp: &HttpResponse) -> bool {
    match resp.status {
        429 | 500..=599 => true,
        403 => {
            let reasons = ErrorBody::parse(&resp.body)
                .map(|e| e.reasons)
                .unwrap_or_default();
            reasons
                .iter()
                .any(|r| r == "rateLimitExceeded" || r == "userRateLimitExceeded")
        }
        _ => false,
    }
}

struct ErrorBody {
    message: String,
    reasons: Vec<String>,
}

impl ErrorBody {
    fn parse(body: &[u8]) -> Option<ErrorBody> {
        #[derive(Deserialize)]
        struct Outer {
            error: Inner,
        }
        #[derive(Deserialize)]
        struct Inner {
            #[serde(default)]
            message: String,
            #[serde(default)]
            errors: Vec<Reason>,
        }
        #[derive(Deserialize)]
        struct Reason {
            #[serde(default)]
            reason: String,
        }
        let o: Outer = serde_json::from_slice(body).ok()?;
        Some(ErrorBody {
            message: o.error.message,
            reasons: o.error.errors.into_iter().map(|r| r.reason).collect(),
        })
    }
}

fn api_error(resp: &HttpResponse) -> Error {
    let message = ErrorBody::parse(&resp.body)
        .map(|e| e.message)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "no details".into());
    match resp.status {
        404 => Error::NotFound(message),
        401 => Error::Auth(format!(
            "Google Drive didn't accept the sign-in ({message}). Sign in again."
        )),
        status => Error::External(format!("Google Drive error {status}: {message}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    parents: Vec<String>,
    /// Drive sends sizes as strings; folders and Google Docs have none.
    size: Option<String>,
    md5_checksum: Option<String>,
    #[serde(default)]
    trashed: bool,
}

impl RawFile {
    fn into_file(self) -> DriveFile {
        DriveFile {
            id: self.id,
            name: self.name,
            mime_type: self.mime_type,
            parents: self.parents,
            size: self.size.and_then(|s| s.parse().ok()).unwrap_or(0),
            md5: self.md5_checksum,
            trashed: self.trashed,
        }
    }
}

/// Drive query string literal: backslash and quote escaped.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', r"\\").replace('\'', r"\'"))
}

impl DriveClient for GoogleDrive {
    fn about(&self) -> Result<Account> {
        #[derive(Deserialize)]
        struct About {
            user: User,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct User {
            #[serde(default)]
            email_address: String,
            #[serde(default)]
            display_name: String,
        }
        let about: About = self.call_json(|| {
            HttpRequest::new(
                "GET",
                url(
                    API,
                    "/about",
                    &[("fields", "user(emailAddress,displayName)")],
                ),
            )
        })?;
        Ok(Account {
            email: about.user.email_address,
            name: about.user.display_name,
        })
    }

    fn create_folder(&self, name: &str, parent: &str) -> Result<DriveFile> {
        let body = json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] });
        self.file(|| {
            HttpRequest::new("POST", url(API, "/files", &[("fields", FILE_FIELDS)])).json(&body)
        })
    }

    fn create_file(&self, name: &str, parent: &str, bytes: &[u8]) -> Result<DriveFile> {
        let boundary = format!("romshelf-{}", crate::auth::random_token(12)?);
        let meta = json!({ "name": name, "parents": [parent] }).to_string();
        let mut body = Vec::with_capacity(bytes.len() + meta.len() + 200);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n\
                 --{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let content_type = format!("multipart/related; boundary={boundary}");
        self.file(|| {
            HttpRequest::new(
                "POST",
                url(
                    UPLOAD_API,
                    "/files",
                    &[("uploadType", "multipart"), ("fields", FILE_FIELDS)],
                ),
            )
            .body(&content_type, body.clone())
        })
    }

    fn update_content(&self, id: &str, bytes: &[u8]) -> Result<DriveFile> {
        self.file(|| {
            HttpRequest::new(
                "PATCH",
                url(
                    UPLOAD_API,
                    &format!("/files/{}", seg(id)),
                    &[("uploadType", "media"), ("fields", FILE_FIELDS)],
                ),
            )
            .body("application/octet-stream", bytes.to_vec())
        })
    }

    fn list(&self, parent: &str) -> Result<Vec<DriveFile>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            files: Vec<RawFile>,
            next_page_token: Option<String>,
        }
        let q = format!("{} in parents and trashed = false", quote(parent));
        let fields = format!("nextPageToken,files({FILE_FIELDS})");
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let page: Page = self.call_json(|| {
                let mut query = vec![
                    ("q", q.as_str()),
                    ("fields", fields.as_str()),
                    ("pageSize", "1000"),
                ];
                if let Some(t) = &page_token {
                    query.push(("pageToken", t));
                }
                HttpRequest::new("GET", url(API, "/files", &query))
            })?;
            out.extend(page.files.into_iter().map(RawFile::into_file));
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => return Ok(out),
            }
        }
    }

    fn get(&self, id: &str) -> Result<DriveFile> {
        self.file(|| {
            HttpRequest::new(
                "GET",
                url(
                    API,
                    &format!("/files/{}", seg(id)),
                    &[("fields", FILE_FIELDS)],
                ),
            )
        })
    }

    fn rename(&self, id: &str, new_name: &str) -> Result<DriveFile> {
        let body = json!({ "name": new_name });
        self.file(|| {
            HttpRequest::new(
                "PATCH",
                url(
                    API,
                    &format!("/files/{}", seg(id)),
                    &[("fields", FILE_FIELDS)],
                ),
            )
            .json(&body)
        })
    }

    fn move_to(&self, id: &str, new_parent: &str) -> Result<DriveFile> {
        let current = self.get(id)?;
        let remove = current
            .parents
            .iter()
            .filter(|p| p.as_str() != new_parent)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        self.file(|| {
            let mut query = vec![("addParents", new_parent), ("fields", FILE_FIELDS)];
            if !remove.is_empty() {
                query.push(("removeParents", &remove));
            }
            HttpRequest::new("PATCH", url(API, &format!("/files/{}", seg(id)), &query))
                .json(&json!({}))
        })
    }

    fn trash(&self, id: &str) -> Result<()> {
        let body = json!({ "trashed": true });
        self.file(|| {
            HttpRequest::new(
                "PATCH",
                url(
                    API,
                    &format!("/files/{}", seg(id)),
                    &[("fields", FILE_FIELDS)],
                ),
            )
            .json(&body)
        })?;
        Ok(())
    }

    fn download(&self, id: &str, offset: u64) -> Result<Vec<u8>> {
        let result = self.call(|| {
            let req = HttpRequest::new(
                "GET",
                url(API, &format!("/files/{}", seg(id)), &[("alt", "media")]),
            );
            if offset > 0 {
                req.header("Range", format!("bytes={offset}-"))
            } else {
                req
            }
        });
        match result {
            Ok(resp) => Ok(resp.body),
            // Range not satisfiable: the offset is already at the end.
            Err(Error::External(m)) if offset > 0 && m.contains("error 416") => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn start_page_token(&self) -> Result<String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Start {
            start_page_token: String,
        }
        let s: Start =
            self.call_json(|| HttpRequest::new("GET", url(API, "/changes/startPageToken", &[])))?;
        Ok(s.start_page_token)
    }

    fn changes(&self, token: &str) -> Result<ChangePage> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawChange {
            file_id: String,
            #[serde(default)]
            removed: bool,
            file: Option<RawFile>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            changes: Vec<RawChange>,
            next_page_token: Option<String>,
            new_start_page_token: Option<String>,
        }
        let fields =
            format!("nextPageToken,newStartPageToken,changes(fileId,removed,file({FILE_FIELDS}))");
        let mut changes = Vec::new();
        let mut page_token = token.to_string();
        loop {
            let page: Page = self.call_json(|| {
                HttpRequest::new(
                    "GET",
                    url(
                        API,
                        "/changes",
                        &[
                            ("pageToken", page_token.as_str()),
                            ("fields", fields.as_str()),
                            ("includeRemoved", "true"),
                            ("pageSize", "1000"),
                            ("spaces", "drive"),
                        ],
                    ),
                )
            })?;
            changes.extend(page.changes.into_iter().map(|c| Change {
                file: if c.removed {
                    None
                } else {
                    c.file.map(RawFile::into_file)
                },
                file_id: c.file_id,
            }));
            if let Some(new_token) = page.new_start_page_token {
                return Ok(ChangePage { changes, new_token });
            }
            page_token = page.next_page_token.ok_or_else(|| {
                Error::External("Google Drive's change list ended without a token".into())
            })?;
        }
    }
}
