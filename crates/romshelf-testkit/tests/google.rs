//! Stage 1: the Google Drive client and sign-in, against a scripted HTTP transport.

use romshelf_core::auth::{
    ClientConfig, GoogleAuth, Pkce, StoredToken, DRIVE_SCOPE, REVOKE_ENDPOINT, TOKEN_ENDPOINT,
    TOKEN_KEY,
};
use romshelf_core::drive::google::GoogleDrive;
use romshelf_core::drive::DriveClient;
use romshelf_core::http::HttpRequest;
use romshelf_core::tokens::TokenStore;
use romshelf_core::Error;
use romshelf_testkit::{FakeClock, FakeHttp, MemoryTokenStore};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

const NOW: i64 = 1_800_000_000;

struct Rig {
    http: Arc<FakeHttp>,
    tokens: Arc<MemoryTokenStore>,
    auth: Arc<GoogleAuth>,
    drive: GoogleDrive,
}

fn rig(expires_at: Option<i64>) -> Rig {
    let http = Arc::new(FakeHttp::default());
    let tokens = Arc::new(MemoryTokenStore::default());
    let clock = Arc::new(FakeClock::at(NOW));
    ClientConfig {
        client_id: "cid.apps.googleusercontent.com".into(),
        client_secret: Some("csecret".into()),
    }
    .save(tokens.as_ref())
    .unwrap();
    if let Some(expires_at) = expires_at {
        let t = StoredToken {
            access_token: "old".into(),
            expires_at,
            refresh_token: "refresh-1".into(),
            scope: DRIVE_SCOPE.into(),
        };
        tokens
            .set(TOKEN_KEY, &serde_json::to_string(&t).unwrap())
            .unwrap();
    }
    let auth = Arc::new(GoogleAuth::new(http.clone(), tokens.clone(), clock));
    let drive = GoogleDrive::new(auth.clone(), http.clone()).with_retry_delay(Duration::ZERO);
    Rig {
        http,
        tokens,
        auth,
        drive,
    }
}

fn signed_in() -> Rig {
    rig(Some(NOW + 3600))
}

fn file_json(id: &str, name: &str) -> String {
    format!(
        r#"{{"id":"{id}","name":"{name}","mimeType":"application/octet-stream","parents":["p"],"size":"1234","md5Checksum":"abc","trashed":false}}"#
    )
}

fn bearer(req: &HttpRequest) -> &str {
    req.header_value("Authorization").unwrap_or_default()
}

fn token_calls(http: &FakeHttp) -> usize {
    http.requests()
        .iter()
        .filter(|r| r.url == TOKEN_ENDPOINT)
        .count()
}

fn form(req: &HttpRequest) -> HashMap<String, String> {
    url::form_urlencoded::parse(&req.body)
        .into_owned()
        .collect()
}

fn query(req: &HttpRequest) -> HashMap<String, String> {
    url::Url::parse(&req.url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

#[test]
fn expired_access_token_refreshes_once_then_makes_the_request() {
    let r = rig(Some(NOW - 10));
    r.http.push(
        200,
        r#"{"access_token":"new","expires_in":3599,"token_type":"Bearer"}"#,
    );
    r.http.push(200, &file_json("f1", "a.gba"));

    let f = r.drive.get("f1").unwrap();
    assert_eq!(f.size, 1234);
    assert_eq!(f.md5.as_deref(), Some("abc"));

    let reqs = r.http.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(token_calls(&r.http), 1);
    let refresh = form(&reqs[0]);
    assert_eq!(refresh["grant_type"], "refresh_token");
    assert_eq!(refresh["refresh_token"], "refresh-1");
    assert_eq!(refresh["client_secret"], "csecret");
    assert_eq!(bearer(&reqs[1]), "Bearer new");

    let stored = r.auth.stored().unwrap().unwrap();
    assert_eq!(stored.access_token, "new");
    assert_eq!(stored.expires_at, NOW + 3599);
    assert_eq!(stored.refresh_token, "refresh-1", "refresh token kept");
}

#[test]
fn rejected_access_token_refreshes_once_and_retries_the_request() {
    let r = signed_in();
    r.http.push(
        401,
        r#"{"error":{"code":401,"message":"Invalid Credentials"}}"#,
    );
    r.http
        .push(200, r#"{"access_token":"new","expires_in":3600}"#);
    r.http.push(200, &file_json("f1", "a.gba"));

    r.drive.get("f1").unwrap();
    let reqs = r.http.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(token_calls(&r.http), 1);
    assert_eq!(bearer(&reqs[0]), "Bearer old");
    assert_eq!(bearer(&reqs[2]), "Bearer new");
}

#[test]
fn a_second_rejection_is_not_refreshed_again() {
    let r = signed_in();
    r.http.push(401, "{}");
    r.http
        .push(200, r#"{"access_token":"new","expires_in":3600}"#);
    r.http
        .push(401, r#"{"error":{"code":401,"message":"Still no"}}"#);
    assert!(r.drive.get("f1").is_err());
    assert_eq!(token_calls(&r.http), 1);
    assert_eq!(r.http.remaining(), 0);
}

#[test]
fn a_revoked_refresh_token_signs_out_with_a_clear_message() {
    let r = rig(Some(NOW - 10));
    r.http.push(
        400,
        r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
    );
    let err = r.drive.get("f1").unwrap_err();
    assert!(matches!(err, Error::Auth(_)), "{err:?}");
    assert!(err.to_string().contains("Sign in again"));
    assert!(!r.auth.signed_in().unwrap());
}

#[test]
fn not_signed_in_is_an_auth_error_without_any_request() {
    let r = rig(None);
    assert!(matches!(r.drive.get("f1").unwrap_err(), Error::Auth(_)));
    assert!(r.http.requests().is_empty());
}

#[test]
fn rate_limit_then_success() {
    let r = signed_in();
    r.http.push(
        429,
        r#"{"error":{"code":429,"message":"Rate Limit Exceeded"}}"#,
    );
    r.http.push(200, &file_json("f1", "a.gba"));
    assert_eq!(r.drive.get("f1").unwrap().id, "f1");
    assert_eq!(r.http.requests().len(), 2);
}

#[test]
fn five_server_errors_fail_with_the_last_error() {
    let r = signed_in();
    for n in 1..=5 {
        r.http.push(
            500,
            &format!(r#"{{"error":{{"code":500,"message":"Backend Error {n}"}}}}"#),
        );
    }
    r.http.push(200, &file_json("f1", "never reached"));
    let err = r.drive.get("f1").unwrap_err();
    assert!(err.to_string().contains("Backend Error 5"), "{err}");
    assert_eq!(r.http.requests().len(), 5);
    assert_eq!(r.http.remaining(), 1);
}

#[test]
fn network_errors_are_retried() {
    let r = signed_in();
    r.http.push_network_error("connection reset");
    r.http.push(503, "{}");
    r.http.push(200, &file_json("f1", "a.gba"));
    assert!(r.drive.get("f1").is_ok());
}

#[test]
fn quota_403_is_retried_but_permission_403_is_not() {
    let r = signed_in();
    r.http.push(
        403,
        r#"{"error":{"code":403,"message":"User rate limit exceeded","errors":[{"reason":"userRateLimitExceeded"}]}}"#,
    );
    r.http.push(200, &file_json("f1", "a.gba"));
    assert!(r.drive.get("f1").is_ok());

    let r = signed_in();
    r.http.push(
        403,
        r#"{"error":{"code":403,"message":"The user does not have sufficient permissions","errors":[{"reason":"insufficientFilePermissions"}]}}"#,
    );
    let err = r.drive.get("f1").unwrap_err();
    assert!(err.to_string().contains("sufficient permissions"));
    assert_eq!(r.http.requests().len(), 1);
}

#[test]
fn missing_file_is_not_found() {
    let r = signed_in();
    r.http.push(
        404,
        r#"{"error":{"code":404,"message":"File not found: f9."}}"#,
    );
    assert!(matches!(r.drive.get("f9").unwrap_err(), Error::NotFound(_)));
}

#[test]
fn list_follows_pages_and_skips_trash() {
    let r = signed_in();
    r.http.push(
        200,
        &format!(
            r#"{{"nextPageToken":"p2","files":[{}]}}"#,
            file_json("a", "one.gba")
        ),
    );
    r.http.push(
        200,
        r#"{"files":[{"id":"b","name":"sub","mimeType":"application/vnd.google-apps.folder","parents":["p"]}]}"#,
    );
    let files = r.drive.list("p").unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[1].is_folder());
    assert_eq!(files[1].size, 0);

    let reqs = r.http.requests();
    let q1 = query(&reqs[0]);
    assert_eq!(q1["q"], "'p' in parents and trashed = false");
    assert!(!q1.contains_key("pageToken"));
    assert_eq!(query(&reqs[1])["pageToken"], "p2");
}

#[test]
fn list_query_escapes_the_parent_id() {
    let r = signed_in();
    r.http.push(200, r#"{"files":[]}"#);
    r.drive.list("it's").unwrap();
    assert_eq!(
        query(&r.http.requests()[0])["q"],
        r"'it\'s' in parents and trashed = false"
    );
}

#[test]
fn create_folder_sends_name_type_and_parent() {
    let r = signed_in();
    r.http.push(
        200,
        r#"{"id":"n","name":"roms","mimeType":"application/vnd.google-apps.folder","parents":["p"]}"#,
    );
    let f = r.drive.create_folder("roms", "p").unwrap();
    assert!(f.is_folder());
    let req = &r.http.requests()[0];
    assert_eq!(req.method, "POST");
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["name"], "roms");
    assert_eq!(body["mimeType"], "application/vnd.google-apps.folder");
    assert_eq!(body["parents"][0], "p");
}

#[test]
fn create_file_uploads_metadata_and_bytes_together() {
    let r = signed_in();
    r.http.push(200, &file_json("n", "settings.json"));
    r.drive
        .create_file("settings.json", "p", b"{\"a\":1}")
        .unwrap();
    let req = &r.http.requests()[0];
    assert_eq!(query(req)["uploadType"], "multipart");
    let ct = req.header_value("Content-Type").unwrap();
    assert!(ct.starts_with("multipart/related; boundary="));
    let body = String::from_utf8(req.body.clone()).unwrap();
    assert!(body.contains(r#""name":"settings.json""#));
    assert!(body.contains(r#""parents":["p"]"#));
    assert!(body.contains("{\"a\":1}"));
}

#[test]
fn update_content_replaces_the_media() {
    let r = signed_in();
    r.http.push(200, &file_json("s", "settings.json"));
    r.drive.update_content("s", b"new").unwrap();
    let req = &r.http.requests()[0];
    assert_eq!(req.method, "PATCH");
    assert!(req.url.contains("/upload/drive/v3/files/s"));
    assert_eq!(query(req)["uploadType"], "media");
    assert_eq!(req.body, b"new");
}

#[test]
fn download_resumes_with_a_range_and_handles_the_end() {
    let r = signed_in();
    r.http.push(206, "tail");
    assert_eq!(r.drive.download("f", 6).unwrap(), b"tail");
    let req = &r.http.requests()[0];
    assert_eq!(query(req)["alt"], "media");
    assert_eq!(req.header_value("Range"), Some("bytes=6-"));

    r.http.push(416, "");
    assert!(r.drive.download("f", 99).unwrap().is_empty());

    r.http.push(200, "whole");
    assert_eq!(r.drive.download("f", 0).unwrap(), b"whole");
    assert_eq!(r.http.requests()[2].header_value("Range"), None);
}

#[test]
fn rename_move_and_trash_patch_the_file() {
    let r = signed_in();
    r.http.push(200, &file_json("f", "b.gba"));
    r.drive.rename("f", "b.gba").unwrap();
    // move_to reads the current parents first.
    r.http.push(200, &file_json("f", "b.gba"));
    r.http.push(200, &file_json("f", "b.gba"));
    r.drive.move_to("f", "q").unwrap();
    r.http.push(200, &file_json("f", "b.gba"));
    r.drive.trash("f").unwrap();

    let reqs = r.http.requests();
    let body = |i: usize| serde_json::from_slice::<serde_json::Value>(&reqs[i].body).unwrap();
    assert_eq!(body(0)["name"], "b.gba");
    let mv = query(&reqs[2]);
    assert_eq!(mv["addParents"], "q");
    assert_eq!(mv["removeParents"], "p");
    assert_eq!(body(3)["trashed"], true);
    assert!(reqs.iter().all(|r| r.method != "DELETE"), "never deletes");
}

#[test]
fn changes_follow_pages_until_a_new_start_token() {
    let r = signed_in();
    r.http.push(200, r#"{"startPageToken":"10"}"#);
    assert_eq!(r.drive.start_page_token().unwrap(), "10");
    r.http.push(
        200,
        &format!(
            r#"{{"nextPageToken":"11","changes":[{{"fileId":"a","removed":false,"file":{}}}]}}"#,
            file_json("a", "x.gba")
        ),
    );
    r.http.push(
        200,
        r#"{"newStartPageToken":"12","changes":[{"fileId":"b","removed":true}]}"#,
    );
    let page = r.drive.changes("10").unwrap();
    assert_eq!(page.new_token, "12");
    assert_eq!(page.changes.len(), 2);
    assert_eq!(page.changes[0].file.as_ref().unwrap().name, "x.gba");
    assert_eq!(page.changes[1].file, None);
    assert_eq!(query(&r.http.requests()[2])["pageToken"], "11");
}

#[test]
fn about_reports_the_account() {
    let r = signed_in();
    r.http.push(
        200,
        r#"{"user":{"emailAddress":"me@example.com","displayName":"Me"}}"#,
    );
    let a = r.drive.about().unwrap();
    assert_eq!(a.email, "me@example.com");
    assert_eq!(a.name, "Me");
}

/// Plays the browser: reads the consent URL and sends Google's redirect to the loopback port.
fn browser_redirect(consent_url: &str, extra: &str) -> romshelf_core::Result<()> {
    let url = url::Url::parse(consent_url).unwrap();
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let redirect = url::Url::parse(&q["redirect_uri"]).unwrap();
    let port = redirect.port().unwrap();
    let state = q["state"].clone();
    let extra = extra.to_string();
    std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            s,
            "GET /?state={state}&code=the-code{extra} HTTP/1.1\r\nHost: x\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
    });
    Ok(())
}

#[test]
fn login_exchanges_the_code_with_the_verifier() {
    let r = rig(None);
    r.http.push(
        200,
        &format!(
            r#"{{"access_token":"a1","expires_in":3600,"refresh_token":"r1","scope":"{DRIVE_SCOPE}","token_type":"Bearer"}}"#
        ),
    );
    let challenge = std::sync::Mutex::new(String::new());
    let open = |url: &str| {
        let q: HashMap<String, String> = url::Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        *challenge.lock().unwrap() = q["code_challenge"].clone();
        browser_redirect(url, "")
    };
    r.auth
        .login(&open, Duration::from_secs(5), &AtomicBool::new(false))
        .unwrap();

    let reqs = r.http.requests();
    assert_eq!(reqs.len(), 1);
    let f = form(&reqs[0]);
    assert_eq!(f["grant_type"], "authorization_code");
    assert_eq!(f["code"], "the-code");
    assert_eq!(f["client_id"], "cid.apps.googleusercontent.com");
    assert!(f["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert_eq!(
        Pkce::from_verifier(&f["code_verifier"]).challenge,
        *challenge.lock().unwrap()
    );
    let stored = r.auth.stored().unwrap().unwrap();
    assert_eq!(stored.refresh_token, "r1");
    assert_eq!(stored.expires_at, NOW + 3600);
}

#[test]
fn login_without_drive_permission_is_refused() {
    let r = rig(None);
    r.http.push(
        200,
        r#"{"access_token":"a1","expires_in":3600,"refresh_token":"r1","scope":"openid"}"#,
    );
    let err = r
        .auth
        .login(
            &|u: &str| browser_redirect(u, ""),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("access to Google Drive"), "{err}");
    assert!(!r.auth.signed_in().unwrap());
}

#[test]
fn login_needs_a_client_first() {
    let http = Arc::new(FakeHttp::default());
    let auth = GoogleAuth::new(
        http.clone(),
        Arc::new(MemoryTokenStore::default()),
        Arc::new(FakeClock::at(NOW)),
    );
    let err = auth
        .login(
            &|_: &str| Ok(()),
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("set-client"), "{err}");
    assert!(http.requests().is_empty());
}

#[test]
fn login_reports_when_no_browser_opens() {
    let r = rig(None);
    let err = r
        .auth
        .login(
            &|_: &str| Err(Error::NoBrowser),
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(matches!(err, Error::NoBrowser));
}

#[test]
fn logout_revokes_and_forgets_tokens_but_keeps_the_client() {
    let r = signed_in();
    r.http.push(200, "");
    r.auth.logout().unwrap();
    let reqs = r.http.requests();
    assert_eq!(reqs[0].url, REVOKE_ENDPOINT);
    assert_eq!(form(&reqs[0])["token"], "refresh-1");
    assert!(!r.auth.signed_in().unwrap());
    assert!(r.auth.has_client().unwrap());
    assert!(r.tokens.get(TOKEN_KEY).unwrap().is_none());
}

#[test]
fn logout_works_offline() {
    let r = signed_in();
    r.http.push_network_error("offline");
    r.auth.logout().unwrap();
    assert!(!r.auth.signed_in().unwrap());
}

#[test]
fn an_expired_change_token_is_reported_as_invalid() {
    let r = signed_in();
    r.http.push(
        400,
        r#"{"error":{"code":400,"message":"Invalid Value","errors":[{"reason":"invalid"}]}}"#,
    );
    assert!(matches!(
        r.drive.changes("old").unwrap_err(),
        Error::Invalid(_)
    ));
    assert_eq!(r.http.requests().len(), 1, "not retried");
}
