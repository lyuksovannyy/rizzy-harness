//! Media over HTTP: uploads and their limits, serving stored files under
//! the T17 rules, thumbnails, privacy (T10), live topics, and deletion that
//! leaves neither the file nor its key behind.

mod common;
use common::SameOrigin as _;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const CSP: &str = "default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; sandbox";

struct T {
    base: String,
    addr: std::net::SocketAddr,
    master: Master,
    dir: tempfile::TempDir,
}

impl T {
    fn db(&self) -> PathBuf {
        self.dir.path().join("master.db")
    }

    fn blobs(&self) -> PathBuf {
        self.dir.path().join("blobs")
    }
}

/// A server with a file-backed store (so the WAL can be inspected), Media
/// switched on, and `edit` applied to its config.
async fn start_with(edit: impl FnOnce(&mut Config)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut cfg = Config::default();
    cfg.media.enabled = true;
    edit(&mut cfg);
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    master.set_provider_runtime(Arc::new(rt));
    harness_master::storage::open(&master, dir.path()).unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master,
        dir,
    }
}

async fn start() -> T {
    start_with(|_| {}).await
}

const TOKEN: &str = "Bearer tok";

fn auth(rb: reqwest::RequestBuilder, c: &str) -> reqwest::RequestBuilder {
    if c.starts_with("Bearer ") {
        rb.header("authorization", c)
    } else {
        rb.header("cookie", c).same_origin()
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = auth(
        reqwest::Client::new().request(method, format!("{}{path}", t.base)),
        c,
    );
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn get_raw(t: &T, path: &str, c: &str, headers: &[(&str, &str)]) -> reqwest::Response {
    let mut rb = auth(reqwest::Client::new().get(format!("{}{path}", t.base)), c);
    for (k, v) in headers {
        rb = rb.header(*k, *v);
    }
    rb.send().await.unwrap()
}

fn enc(name: &str) -> String {
    name.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn upload_to(
    t: &T,
    c: &str,
    query: &str,
    name: &str,
    bytes: &[u8],
    ctype: &str,
) -> (StatusCode, Value) {
    let rb = reqwest::Client::new()
        .post(format!("{}/api/media{query}", t.base))
        .header("x-file-name", enc(name))
        .header("content-type", ctype)
        .body(bytes.to_vec());
    let r = auth(rb, c).send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn upload(t: &T, c: &str, name: &str, bytes: &[u8]) -> (StatusCode, Value) {
    upload_to(t, c, "", name, bytes, "application/octet-stream").await
}

fn cookie(r: &reqwest::Response) -> String {
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.header("cookie", c).same_origin();
    }
    rb.send().await.unwrap()
}

/// The server admin "alex" and a member "bob", with their user ids.
async fn users(t: &T) -> [(String, String); 2] {
    let alex = cookie(
        &post_anon(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    );
    let body = json!({"username": "bob", "password": "temporary pw"});
    req(t, Method::POST, "/api/users", Some(body), &alex).await;
    let first = post_anon(
        t,
        "/api/login",
        json!({"username": "bob", "password": "temporary pw"}),
        None,
    )
    .await;
    let bob = cookie(
        &post_anon(
            t,
            "/api/me/password",
            json!({"current": "temporary pw", "new": "bobs own password"}),
            Some(&cookie(&first)),
        )
        .await,
    );
    let mut out = Vec::new();
    for c in [alex, bob] {
        let (_, me) = req(t, Method::GET, "/api/me", None, &c).await;
        out.push((c, me["user"]["user_id"].as_str().unwrap().to_string()));
    }
    out.try_into().unwrap()
}

/// A PNG header of `w`×`h`, then `tail` (never decoded, only sniffed).
fn png(w: u32, h: u32, tail: &[u8]) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(tail);
    b
}

fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]], tail: &[u8]) -> Vec<u8> {
    let mut b = (16 + 4 * compatible.len() as u32).to_be_bytes().to_vec();
    b.extend(b"ftyp");
    b.extend(major);
    b.extend([0, 0, 0, 0]);
    for c in compatible {
        b.extend(*c);
    }
    b.extend(tail);
    b
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn header<'a>(r: &'a reqwest::Response, name: &str) -> &'a str {
    r.headers()
        .get(name)
        .map(|v| v.to_str().unwrap())
        .unwrap_or("")
}

/// Every header a stored file is served with, whatever its type.
fn assert_file_headers(r: &reqwest::Response) {
    assert_eq!(
        header(r, "content-security-policy"),
        CSP,
        "the guard kept the file's CSP"
    );
    assert_eq!(header(r, "x-content-type-options"), "nosniff");
    assert_eq!(header(r, "cross-origin-resource-policy"), "same-origin");
    assert_eq!(header(r, "cache-control"), "no-store");
}

// ---- raw HTTP, for bodies that stall or stop early -------------------------

async fn raw_head(t: &T, c: &str, extra: &str) -> tokio::net::TcpStream {
    let mut s = tokio::net::TcpStream::connect(t.addr).await.unwrap();
    let auth = if c.starts_with("Bearer ") {
        format!("Authorization: {c}")
    } else {
        // A browser's upload: the cookie and its page's Origin.
        format!("Cookie: {c}\r\nOrigin: http://{}", t.addr)
    };
    let head = format!(
        "POST /api/media HTTP/1.1\r\nHost: {}\r\n{auth}\r\nX-File-Name: a.png\r\n{extra}\r\n",
        t.addr
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s
}

/// The status of the response on `s`, waiting at most `secs`.
async fn raw_status(s: &mut tokio::net::TcpStream, secs: u64) -> u16 {
    let mut buf = Vec::new();
    let read = async {
        let mut chunk = [0u8; 4096];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = s.read(&mut chunk).await.unwrap();
            assert!(n > 0, "closed without an answer");
            buf.extend_from_slice(&chunk[..n]);
        }
    };
    tokio::time::timeout(Duration::from_secs(secs), read)
        .await
        .expect("no answer in time");
    let text = String::from_utf8_lossy(&buf);
    text.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn tmp_files(t: &T) -> usize {
    std::fs::read_dir(t.blobs().join("tmp")).map_or(0, |d| d.count())
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Whether `bytes` appear in master.db or its WAL.
fn in_database(t: &T, bytes: &[u8]) -> bool {
    let mut wal = t.db().into_os_string();
    wal.push("-wal");
    [t.db(), PathBuf::from(wal)]
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .any(|b| contains(&b, bytes))
}

/// `media_id`'s blob id and key, read from the store.
fn blob_of(t: &T, media_id: &str) -> (String, Vec<u8>) {
    let s = t.master.inner_mirror();
    let row = s.media_get(media_id).unwrap().unwrap();
    let key = s.blob_key(&row.blob.blob_id).unwrap().unwrap();
    (row.blob.blob_id, key)
}

fn blob_file(t: &T, blob_id: &str) -> PathBuf {
    t.blobs()
        .join(&blob_id[30..])
        .join(format!("{blob_id}.rzb"))
}

// ---- tests -----------------------------------------------------------------

#[tokio::test]
async fn upload_list_and_serve_with_exact_headers() {
    let t = start().await;
    let [(alex, _), _] = users(&t).await;
    let bytes = png(640, 480, &[42; 5000]);
    let (st, v) = upload(&t, &alex, "Cat photo.png", &bytes).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["media_id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 32);
    for (k, want) in [
        ("name", json!("Cat photo.png")),
        ("kind", json!("image")),
        ("format", json!("png")),
        ("mime", json!("image/png")),
        ("display", json!("inline")),
        ("size", json!(bytes.len())),
        ("width", json!(640)),
        ("height", json!(480)),
        ("duration_ms", Value::Null),
        ("has_thumb", json!(false)),
        ("no_thumb", json!(false)),
        ("origin", json!("upload")),
        ("source", Value::Null),
        ("artifact", Value::Null),
        ("chats", json!(0)),
    ] {
        assert_eq!(v[k], want, "{k}: {v}");
    }
    assert!(v.get("duplicate").is_none());
    let (_, me) = req(&t, Method::GET, "/api/me", None, &alex).await;
    assert_eq!(v["workspace_id"], me["workspace"]["workspace_id"]);

    // The same bytes again: the same item, nothing new stored.
    let (st, again) = upload(&t, &alex, "copy.png", &bytes).await;
    assert_eq!(st, StatusCode::OK, "{again}");
    assert_eq!(
        (again["media_id"].as_str(), again["duplicate"].as_bool()),
        (Some(id.as_str()), Some(true))
    );
    assert_eq!(files_under(&t.blobs()).len(), 1);
    assert_eq!(tmp_files(&t), 0);

    let (st, list) = req(&t, Method::GET, "/api/media", None, &alex).await;
    assert_eq!(st, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["next"], Value::Null);
    assert_eq!(list["usage"]["used"], json!(bytes.len()));
    assert_eq!(list["usage"]["quota"], json!(4096u64 << 20));
    assert_eq!(list["limits"]["max_file"], json!(200u64 << 20));
    let (_, one) = req(&t, Method::GET, &format!("/api/media/{id}"), None, &alex).await;
    assert_eq!(one["media_id"], json!(id));

    let path = format!("/api/media/{id}/content");
    let r = get_raw(&t, &path, &alex, &[]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_file_headers(&r);
    assert_eq!(header(&r, "content-type"), "image/png");
    let etag = format!("\"{}\"", sha(&bytes));
    assert_eq!(header(&r, "etag"), etag);
    assert!(header(&r, "content-disposition").starts_with("inline; filename=\"Cat photo.png\""));
    assert_eq!(header(&r, "accept-ranges"), "bytes");
    assert_eq!(header(&r, "vary"), "");
    assert_eq!(r.bytes().await.unwrap().as_ref(), bytes.as_slice());

    let r = get_raw(&t, &path, &alex, &[("if-none-match", &etag)]).await;
    assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
    assert_file_headers(&r);

    let r = get_raw(&t, &path, &alex, &[("range", "bytes=2-5")]).await;
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        header(&r, "content-range"),
        format!("bytes 2-5/{}", bytes.len())
    );
    assert_eq!(r.bytes().await.unwrap().as_ref(), &bytes[2..=5]);
    // Across the 64 KiB segment boundary of a larger file.
    let big = png(
        1,
        1,
        &(0..200_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
    );
    let (_, bv) = upload(&t, &alex, "big.png", &big).await;
    let bpath = format!("/api/media/{}/content", bv["media_id"].as_str().unwrap());
    let r = get_raw(&t, &bpath, &alex, &[("range", "bytes=65530-65545")]).await;
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.bytes().await.unwrap().as_ref(), &big[65530..=65545]);
    let r = get_raw(&t, &bpath, &alex, &[]).await;
    assert_eq!(r.bytes().await.unwrap().as_ref(), big.as_slice());

    let past = format!("bytes={}-", bytes.len());
    let r = get_raw(&t, &path, &alex, &[("range", &past)]).await;
    assert_eq!(r.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        header(&r, "content-range"),
        format!("bytes */{}", bytes.len())
    );

    let r = get_raw(&t, &format!("{path}?download=1"), &alex, &[]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_file_headers(&r);
    assert_eq!(header(&r, "content-type"), "application/octet-stream");
    assert!(
        header(&r, "content-disposition").starts_with("attachment; filename=\"Cat photo.png\"")
    );
    assert_eq!(header(&r, "x-content-sha256"), sha(&bytes));

    // Features say Media is on, and search offers it.
    let (_, f) = req(&t, Method::GET, "/api/features", None, &alex).await;
    assert_eq!(
        f["media"],
        json!({"enabled": true, "show": true, "max_file": 200u64 << 20, "vision_images": false})
    );
    assert!(
        f["search"]["kinds"]
            .as_array()
            .unwrap()
            .contains(&json!("media"))
    );
}

#[tokio::test]
async fn svg_and_heic_are_shown_only_to_an_img() {
    let t = start().await;
    let [(alex, _), _] = users(&t).await;
    let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"><script>window.__pwned=1</script></svg>"#;
    let (st, v) = upload_to(&t, &alex, "", "logo.svg", svg, "image/svg+xml").await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(
        (v["kind"].as_str(), v["display"].as_str()),
        (Some("image"), Some("img"))
    );
    let path = format!("/api/media/{}/content", v["media_id"].as_str().unwrap());
    let plain = format!("\"{}\"", sha(svg));
    let img = format!("\"{}-img\"", sha(svg));

    // Opened as a page (no Sec-Fetch-Dest, or a document): a download.
    for dest in [None, Some("document")] {
        let h: Vec<(&str, &str)> = dest.iter().map(|d| ("sec-fetch-dest", *d)).collect();
        let r = get_raw(&t, &path, &alex, &h).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_file_headers(&r);
        assert_eq!(header(&r, "content-type"), "application/octet-stream");
        assert!(header(&r, "content-disposition").starts_with("attachment;"));
        assert_eq!(header(&r, "vary"), "Sec-Fetch-Dest");
        assert_eq!(header(&r, "etag"), plain);
    }
    // To an <img>: its own type, under a variant ETag.
    let r = get_raw(&t, &path, &alex, &[("sec-fetch-dest", "image")]).await;
    assert_eq!(header(&r, "content-type"), "image/svg+xml");
    assert_eq!(header(&r, "vary"), "Sec-Fetch-Dest");
    assert_eq!(header(&r, "etag"), img);
    assert_file_headers(&r);
    // A cached <img> response never revalidates into a document.
    let r = get_raw(&t, &path, &alex, &[("if-none-match", &img)]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(header(&r, "content-type"), "application/octet-stream");
    let r = get_raw(
        &t,
        &path,
        &alex,
        &[("sec-fetch-dest", "image"), ("if-none-match", &img)],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_MODIFIED);

    let mut ispe = 20u32.to_be_bytes().to_vec();
    ispe.extend(b"ispe\0\0\0\0");
    ispe.extend(4032u32.to_be_bytes());
    ispe.extend(3024u32.to_be_bytes());
    let heic = ftyp(b"heic", &[b"mif1", b"heic"], &ispe);
    let (st, h) = upload(&t, &alex, "IMG_0001.HEIC", &heic).await;
    assert_eq!(st, StatusCode::CREATED, "{h}");
    assert_eq!(
        (
            h["format"].as_str(),
            h["display"].as_str(),
            h["width"].as_u64(),
            h["height"].as_u64()
        ),
        (Some("heic"), Some("img"), Some(4032), Some(3024))
    );
    let hpath = format!("/api/media/{}/content", h["media_id"].as_str().unwrap());
    let r = get_raw(&t, &hpath, &alex, &[("sec-fetch-dest", "image")]).await;
    assert_eq!(header(&r, "content-type"), "image/heic");
    let r = get_raw(&t, &hpath, &alex, &[]).await;
    assert_eq!(header(&r, "content-type"), "application/octet-stream");

    // QuickTime plays inline as video/quicktime.
    let mov = ftyp(b"qt  ", &[b"qt  "], &[0; 64]);
    let (st, q) = upload(&t, &alex, "clip.MOV", &mov).await;
    assert_eq!(st, StatusCode::CREATED, "{q}");
    assert_eq!(
        (
            q["kind"].as_str(),
            q["mime"].as_str(),
            q["display"].as_str()
        ),
        (Some("video"), Some("video/quicktime"), Some("inline"))
    );
    let qpath = format!("/api/media/{}/content", q["media_id"].as_str().unwrap());
    let r = get_raw(&t, &qpath, &alex, &[]).await;
    assert_eq!(header(&r, "content-type"), "video/quicktime");
}

#[tokio::test]
async fn refusals_415_413_and_507_before_the_body() {
    let t = start_with(|c| {
        c.storage.max_file_mb = 1;
        c.storage.account_quota_mb = 2;
    })
    .await;
    let [(alex, _), _] = users(&t).await;
    let html = b"<!DOCTYPE html><html><script>window.__pwned=1</script></html>";
    let (st, v) = upload_to(&t, &alex, "", "cat.png", html, "image/png").await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{v}");
    assert_eq!(
        v["error"],
        "not a media file; build outputs go to Artifacts"
    );
    let (st, _) = upload(&t, &alex, "report.pdf", b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n").await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Too large, said by Content-Length: refused before any byte is read.
    let mut s = raw_head(&t, &alex, &format!("Content-Length: {}\r\n", 3 << 20)).await;
    assert_eq!(raw_status(&mut s, 10).await, 413);
    // Too large while streaming (no Content-Length): refused once counted.
    let mut s = raw_head(&t, &alex, "Transfer-Encoding: chunked\r\n").await;
    let chunk = png(1, 1, &[0; 65_536 - 33]);
    for _ in 0..16 {
        s.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
            .await
            .unwrap();
        s.write_all(&chunk).await.unwrap();
        s.write_all(b"\r\n").await.unwrap();
    }
    s.write_all(b"1\r\nX\r\n").await.unwrap();
    assert_eq!(raw_status(&mut s, 10).await, 413);

    // Fill the account (2 MB) to just under its quota, then a Content-Length
    // that does not fit: 507 at once, without sending the body.
    let filler = png(1, 1, &vec![7; (1 << 20) - 100]);
    assert_eq!(
        upload(&t, &alex, "a.png", &filler).await.0,
        StatusCode::CREATED
    );
    let filler2 = png(2, 2, &vec![8; (1 << 20) - 100]);
    assert_eq!(
        upload(&t, &alex, "b.png", &filler2).await.0,
        StatusCode::CREATED
    );
    let mut s = raw_head(&t, &alex, "Content-Length: 1000\r\n").await;
    assert_eq!(raw_status(&mut s, 10).await, 507);
    let (st, v) = upload(&t, &alex, "c.png", &png(3, 3, &[1; 1000])).await;
    assert_eq!(st, StatusCode::INSUFFICIENT_STORAGE, "{v}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(tmp_files(&t), 0, "refused uploads leave no temp files");
    assert_eq!(files_under(&t.blobs()).len(), 2);
}

#[tokio::test]
async fn stalled_uploads_time_out_and_slots_are_bounded() {
    let t = start().await;
    let [(alex, _), (bob, _)] = users(&t).await;
    harness_master::storage::set_upload_timeouts(
        &t.master,
        Duration::from_secs(1),
        Duration::from_secs(60),
    );
    let mut s = raw_head(&t, &alex, "Content-Length: 100\r\n").await;
    s.write_all(&png(1, 1, &[])[..20]).await.unwrap();
    assert_eq!(raw_status(&mut s, 10).await, 408);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(tmp_files(&t), 0, "the stalled upload's temp file is gone");

    harness_master::storage::set_upload_timeouts(
        &t.master,
        Duration::from_secs(20),
        Duration::from_secs(60),
    );
    let mut held = Vec::new();
    for c in [&bob, &bob, &alex, &alex] {
        let mut s = raw_head(&t, c, "Content-Length: 1000\r\n").await;
        s.write_all(b"\x89PNG").await.unwrap();
        held.push(s);
    }
    // Let the server take all four slots.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/media", t.base))
        .header("cookie", &bob)
        .same_origin()
        .body(png(1, 1, b"third"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS, "two per account");
    assert_eq!(header(&r, "retry-after"), "5");
    let (st, _) = upload(&t, TOKEN, "t.png", &png(1, 1, b"token")).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "four on the server");
    // Closing the stalled uploads gives the slots back.
    drop(held);
    let mut ok = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if upload(&t, &bob, "later.png", &png(1, 1, b"later")).await.0 == StatusCode::CREATED {
            ok = true;
            break;
        }
    }
    assert!(ok, "slots came back");
}

#[tokio::test]
async fn thumbnails_and_hints_belong_to_the_owner() {
    let t = start().await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let (_, v) = upload(&t, &alex, "a.png", &png(800, 600, b"x")).await;
    let id = v["media_id"].as_str().unwrap().to_string();
    let thumb_path = format!("/api/media/{id}/thumb");
    assert_eq!(
        get_raw(&t, &thumb_path, &alex, &[]).await.status(),
        StatusCode::NOT_FOUND
    );

    let put = |c: &str, body: Vec<u8>| {
        let rb = reqwest::Client::new()
            .put(format!("{}{thumb_path}", t.base))
            .header("cookie", c)
            .same_origin()
            .body(body);
        async move { rb.send().await.unwrap().status() }
    };
    let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
    webp.extend([1; 100]);
    assert_eq!(
        put(&bob, webp.clone()).await,
        StatusCode::NOT_FOUND,
        "not bob's"
    );
    assert_eq!(
        put(&alex, b"GIF89a....".to_vec()).await,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        put(&alex, b"<svg/>".to_vec()).await,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let mut big = webp.clone();
    big.resize(70 * 1024, 0);
    assert_eq!(put(&alex, big).await, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(put(&alex, webp.clone()).await, StatusCode::OK);

    let r = get_raw(&t, &thumb_path, &alex, &[]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_file_headers(&r);
    assert_eq!(header(&r, "content-type"), "image/webp");
    let etag = header(&r, "etag").to_string();
    assert!(etag.starts_with("\"t-"), "{etag}");
    assert_eq!(r.bytes().await.unwrap().as_ref(), webp.as_slice());
    let r = get_raw(&t, &thumb_path, &alex, &[("if-none-match", &etag)]).await;
    assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
    let (_, v) = req(&t, Method::GET, &format!("/api/media/{id}"), None, &alex).await;
    assert_eq!(v["has_thumb"], true);

    // An image's size is its header's; a video takes clamped hints.
    let path = format!("/api/media/{id}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"width": 5, "height": 5, "no_thumb": true})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["width"].as_u64(),
            v["height"].as_u64(),
            v["no_thumb"].as_bool()
        ),
        (Some(800), Some(600), Some(true))
    );
    let mp4 = ftyp(b"isom", &[b"isom", b"mp41"], &[0; 32]);
    let (_, m) = upload(&t, &alex, "clip.mp4", &mp4).await;
    let mpath = format!("/api/media/{}", m["media_id"].as_str().unwrap());
    let hints = json!({"width": 9_999_999, "height": 0, "duration_ms": 999_999_999_999i64});
    let (_, m) = req(&t, Method::PATCH, &mpath, Some(hints), &alex).await;
    assert_eq!(
        (
            m["width"].as_u64(),
            m["height"].as_u64(),
            m["duration_ms"].as_u64()
        ),
        (Some(100_000), Some(1), Some(48 * 3600 * 1000))
    );
    let (st, _) = req(
        &t,
        Method::PATCH,
        &mpath,
        Some(json!({"no_thumb": true})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(
        &t,
        Method::PATCH,
        &mpath,
        Some(json!({"rotate": 90})),
        &alex,
    )
    .await;
    assert!(st.is_client_error());

    // Renaming feeds the name index.
    let (st, v) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"name": "  Harbour at dawn.png "})),
        &alex,
    )
    .await;
    assert_eq!(
        (st, v["name"].as_str()),
        (StatusCode::OK, Some("Harbour at dawn.png"))
    );
    let (st, _) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"name": " \u{200b} "})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, list) = req(&t, Method::GET, "/api/media?q=harbour", None, &alex).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
    let (_, list) = req(&t, Method::GET, "/api/media?q=clip&kind=video", None, &alex).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
    let (_, list) = req(&t, Method::GET, "/api/media?kind=image", None, &alex).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
    let (st, _) = req(&t, Method::GET, "/api/media?kind=images", None, &alex).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn media_is_private_admins_included_and_guests_upload() {
    let t = start().await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let (_, a) = upload(&t, &alex, "alex.png", &png(1, 1, b"alex")).await;
    let (_, b) = upload(&t, &bob, "bob.png", &png(1, 1, b"bob")).await;
    let (aid, bid) = (
        a["media_id"].as_str().unwrap(),
        b["media_id"].as_str().unwrap(),
    );
    // Neither sees the other's, the server admin (alex) included.
    for (c, other) in [(&bob, aid), (&alex, bid)] {
        for path in [
            format!("/api/media/{other}"),
            format!("/api/media/{other}/content"),
            format!("/api/media/{other}/thumb"),
        ] {
            assert_eq!(
                get_raw(&t, &path, c, &[]).await.status(),
                StatusCode::NOT_FOUND,
                "{path}"
            );
        }
        let (st, _) = req(&t, Method::DELETE, &format!("/api/media/{other}"), None, c).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, _) = req(
            &t,
            Method::POST,
            "/api/media/delete",
            Some(json!({"ids": [other]})),
            c,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "skipped, unconfirmed");
        let (_, list) = req(&t, Method::GET, "/api/media", None, c).await;
        assert_eq!(list["items"].as_array().unwrap().len(), 1);
    }
    // Both are still there; the API token sees every one.
    let (_, all) = req(&t, Method::GET, "/api/media", None, TOKEN).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    let r = get_raw(&t, &format!("/api/media/{aid}/content"), TOKEN, &[]).await;
    assert_eq!(r.status(), StatusCode::OK);

    // A guest of a team workspace keeps their own Media there.
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let ws = w["workspace_id"].as_str().unwrap().to_string();
    let add = json!({"username": "bob", "role": "guest"});
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(add),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let q = format!("?workspace={ws}");
    let (st, g) = upload_to(&t, &bob, &q, "guest.png", &png(1, 1, b"guest"), "image/png").await;
    assert_eq!(st, StatusCode::CREATED, "{g}");
    assert_eq!(g["workspace_id"].as_str(), Some(ws.as_str()));
    let (_, list) = req(&t, Method::GET, &format!("/api/media{q}"), None, &bob).await;
    let names: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["guest.png"], "the workspace filters");
    let (_, list) = req(&t, Method::GET, &format!("/api/media{q}"), None, &alex).await;
    assert!(
        list["items"].as_array().unwrap().is_empty(),
        "the workspace never shares"
    );
    let (_, f) = req(&t, Method::GET, &format!("/api/features{q}"), None, &bob).await;
    assert_eq!(f["media"]["show"], true);
    // A workspace the caller is not in: 404.
    let (st, _) = req(
        &t,
        Method::GET,
        "/api/media?workspace=0193f1c2a4b57d6e8f9012345678abcd",
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn media_off_answers_404() {
    let t = start_with(|c| c.media.enabled = false).await;
    let [(alex, _), _] = users(&t).await;
    let (st, _) = req(&t, Method::GET, "/api/media", None, &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(
        upload(&t, &alex, "a.png", &png(1, 1, b"")).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, f) = req(&t, Method::GET, "/api/features", None, &alex).await;
    assert_eq!(f["media"]["show"], false);
    assert!(
        !f["search"]["kinds"]
            .as_array()
            .unwrap()
            .contains(&json!("media"))
    );
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn socket(t: &T, c: &str) -> Socket {
    let mut r = format!("ws://{}/api/ws", t.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", c.parse().unwrap());
    let origin = format!("http://{}", r.uri().authority().unwrap());
    r.headers_mut().insert("origin", origin.parse().unwrap());
    let (mut s, _) = tokio_tungstenite::connect_async(r).await.unwrap();
    s.send(Message::Text("hello".into())).await.unwrap();
    assert_eq!(next_type(&mut s).await, "error");
    s
}

async fn next_type(s: &mut Socket) -> String {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), s.next())
            .await
            .expect("socket went quiet");
        if let Some(Ok(Message::Text(text))) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            return v["type"].as_str().unwrap().to_string();
        }
    }
}

async fn topics_until(s: &mut Socket, last: &str) -> Vec<String> {
    let mut got = Vec::new();
    loop {
        let ty = next_type(s).await;
        if ty.ends_with("_changed") && ty != "sessions_changed" && ty != "nodes_changed" {
            got.push(ty.clone());
        }
        if ty == last {
            return got;
        }
    }
}

#[tokio::test]
async fn media_changed_reaches_only_the_owner() {
    let t = start().await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let (st, _) = upload(&t, &alex, "a.png", &png(1, 1, b"live")).await;
    assert_eq!(st, StatusCode::CREATED);
    // A shared topic marks where to stop reading.
    t.master.changed("artifacts", None);
    assert_eq!(
        topics_until(&mut a, "artifacts_changed").await,
        ["media_changed", "artifacts_changed"]
    );
    assert_eq!(
        topics_until(&mut b, "artifacts_changed").await,
        ["artifacts_changed"]
    );
}

#[tokio::test]
async fn deleting_shreds_the_file_and_its_key() {
    let t = start().await;
    let [(alex, _), _] = users(&t).await;
    let marker = b"PLAINTEXT-MARKER-7c1f0e";
    let bytes = png(10, 10, &marker.repeat(3000));
    let (st, v) = upload(&t, &alex, "secret plans.png", &bytes).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["media_id"].as_str().unwrap();
    let (blob_id, key) = blob_of(&t, id);
    assert_eq!(key.len(), 32);
    let file = blob_file(&t, &blob_id);
    assert!(file.exists());
    for f in files_under(&t.blobs()) {
        assert!(
            !contains(&std::fs::read(&f).unwrap(), marker),
            "plaintext in {f:?}"
        );
    }
    assert!(
        in_database(&t, &key),
        "the key is in master.db before the delete"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&t.blobs()), 0o700);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
    }

    let (st, _) = req(&t, Method::DELETE, &format!("/api/media/{id}"), None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    assert!(!file.exists());
    assert!(!in_database(&t, &key), "the key left master.db and its WAL");
    let s = t.master.inner_mirror();
    assert!(
        s.media_search(None, None, Some("\"secret\"*"), 10, 0)
            .unwrap()
            .is_empty()
    );
    let (st, _) = req(&t, Method::GET, &format!("/api/media/{id}"), None, &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // The bulk delete shreds the same way.
    let (_, v) = upload(&t, &alex, "more.png", &png(2, 2, b"more")).await;
    let id = v["media_id"].as_str().unwrap();
    let (blob_id, key) = blob_of(&t, id);
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/media/delete",
        Some(json!({"ids": [id]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(!blob_file(&t, &blob_id).exists());
    assert!(!in_database(&t, &key));
    let ids: Vec<String> = (0..101).map(|i| format!("{i:032x}")).collect();
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/media/delete",
        Some(json!({"ids": ids})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn account_deletion_removes_files_keys_and_names() {
    let t = start().await;
    let [(alex, _), (bob, bob_id)] = users(&t).await;
    let mut gone = Vec::new();
    for (name, tail) in [("bob one.png", b"one"), ("bob two.png", b"two")] {
        let (_, v) = upload(&t, &bob, name, &png(1, 1, tail)).await;
        gone.push(blob_of(&t, v["media_id"].as_str().unwrap()));
    }
    let (_, keep) = upload(&t, &alex, "alex.png", &png(1, 1, b"one")).await;
    let (keep_blob, _) = blob_of(&t, keep["media_id"].as_str().unwrap());
    let (st, v) = req(
        &t,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for (blob_id, key) in &gone {
        assert!(!blob_file(&t, blob_id).exists(), "file of {blob_id}");
        assert!(!in_database(&t, key), "key of {blob_id}");
    }
    assert!(
        blob_file(&t, &keep_blob).exists(),
        "other accounts keep theirs"
    );
    let s = t.master.inner_mirror();
    assert!(
        s.media_search(None, None, Some("\"bob\"*"), 10, 0)
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.media_search(None, None, None, 10, 0).unwrap().len(), 1);
}

#[tokio::test]
async fn sweeps_remove_orphans_and_temp_files() {
    let t = start().await;
    let [(alex, _), _] = users(&t).await;
    let (_, v) = upload(&t, &alex, "kept.png", &png(1, 1, b"kept")).await;
    let (kept, _) = blob_of(&t, v["media_id"].as_str().unwrap());
    let shard = t.blobs().join("ee");
    std::fs::create_dir_all(&shard).unwrap();
    let old_orphan = shard.join("0193f1c2a4b57d6e8f9012345678abee.rzb");
    let new_orphan = shard.join("0193f1c2a4b57d6e8f9012345678acee.rzb");
    let old_tmp = t
        .blobs()
        .join("tmp")
        .join("0193f1c2a4b57d6e8f9012345678adee.part");
    for p in [&old_orphan, &new_orphan, &old_tmp] {
        std::fs::write(p, b"leftover").unwrap();
    }
    let two_hours_ago = std::time::SystemTime::now() - Duration::from_secs(7200);
    for p in [&old_orphan, &old_tmp] {
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
    }
    harness_master::storage::hourly(&t.master).await;
    assert!(!old_orphan.exists() && !old_tmp.exists());
    assert!(new_orphan.exists(), "a young file may still be committing");
    assert!(blob_file(&t, &kept).exists());

    // At start, every file without a row goes, young or old.
    let again = Master::new(
        Store::open(&t.db(), Durability::Normal).unwrap(),
        Auth::new(Secret::new("tok".into()), vec![], false),
    );
    harness_master::storage::open(&again, t.dir.path()).unwrap();
    assert!(!new_orphan.exists());
    assert!(blob_file(&t, &kept).exists());
}

#[tokio::test]
async fn the_search_provider_shows_only_the_callers_media() {
    use harness_master::search::{SearchCtx, fts_query, tokens};
    let t = start().await;
    let [(alex, alex_id), (bob, bob_id)] = users(&t).await;
    let (_, sunset) = upload(&t, &alex, "Sunset over Riga.png", &png(1, 1, b"s")).await;
    upload(
        &t,
        &alex,
        "sunset clip.mp4",
        &ftyp(b"isom", &[b"isom"], &[0; 16]),
    )
    .await;
    upload(&t, &alex, "raw sunset.mkv", &{
        let mut b = vec![0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x88];
        b.extend(b"matroska");
        b.extend([0; 300]);
        b
    })
    .await;
    upload(&t, &bob, "bobs sunset.png", &png(1, 1, b"b")).await;
    let sid = sunset["media_id"].as_str().unwrap().to_string();
    let rb = reqwest::Client::new()
        .put(format!("{}/api/media/{sid}/thumb", t.base))
        .header("cookie", &alex)
        .same_origin()
        .body(b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0".to_vec());
    assert_eq!(rb.send().await.unwrap().status(), StatusCode::OK);
    let (_, me) = req(&t, Method::GET, "/api/me", None, &alex).await;
    let ws = me["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();

    let run = |q: &str, owner: Option<String>, ws: Option<String>| {
        let toks = tokens(q);
        let fts = fts_query(&toks, false);
        let s = t.master.inner_mirror();
        let ctx = SearchCtx {
            tokens: &toks,
            fts,
            owner: owner.as_deref(),
            workspace: ws.as_deref(),
            limit: 10,
            offset: 0,
        };
        harness_master::media::search(&s, &ctx).unwrap()
    };
    let rows = run("riga sunset", Some(alex_id.clone()), Some(ws.clone()));
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(
        (r.kind, r.type_, r.id.as_str()),
        ("media", "image", sid.as_str())
    );
    assert_eq!(r.title, "\u{2}Sunset\u{3} over \u{2}Riga\u{3}.png");
    assert_eq!(r.preview, "Uploaded");
    assert_eq!(
        r.thumb.as_deref(),
        Some(format!("/api/media/{sid}/thumb").as_str())
    );
    assert_eq!(
        (r.tier, r.snippet.as_deref(), r.project_id.as_deref()),
        (0, None, None)
    );

    let rows = run("sunset", Some(alex_id.clone()), Some(ws.clone()));
    let mut types: Vec<&str> = rows.iter().map(|r| r.type_).collect();
    types.sort();
    assert_eq!(
        types,
        ["image", "media_other", "video"],
        "bob's is not alex's"
    );
    assert!(rows.iter().all(|r| r.thumb.is_none() || r.id == sid));
    // FTS syntax in the query is inert.
    assert!(
        run(
            "sunset OR NEAR(\"x\" *",
            Some(alex_id.clone()),
            Some(ws.clone())
        )
        .len()
            <= 3
    );
    // No terms: the newest visible items.
    assert_eq!(run("", Some(alex_id.clone()), Some(ws.clone())).len(), 3);
    assert_eq!(run("", Some(bob_id.clone()), None).len(), 1);
    // Another workspace of alex's has none of them.
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let other = w["workspace_id"].as_str().map(str::to_string);
    assert!(run("sunset", Some(alex_id), other).is_empty());
    // The API token searches everyone's.
    assert_eq!(run("sunset", None, None).len(), 4);
}
