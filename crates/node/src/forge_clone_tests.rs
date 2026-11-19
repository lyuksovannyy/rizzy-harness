//! `project.clone` against a fake git server on 127.0.0.1 (PLAN.md T86):
//! `git http-backend` behind a few lines of HTTP that record every request
//! and can ask for a credential, redirect or stall. A made-up token, the
//! canary, is searched for wherever it must not be. No real forge, no real
//! token.

use super::*;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use crate::policy::ProjectsPolicy;

const CANARY: &str = "glpat-CANARYclone00000000000000001";
const USER: &str = "ada";

fn basic() -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{CANARY}"))
    )
}

// ---------------------------------------------------------------- server

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    auth: Option<String>,
    /// Every header line, as sent.
    head: String,
}

#[derive(Clone, Default)]
struct Mode {
    /// 401 until the right Basic credential comes.
    ask: bool,
    /// Answer with this status and `Location: <to><path>`, on the first
    /// GET (`on_post` false) or on the POST.
    redirect: Option<(u16, String, bool)>,
    /// Read the request, then never answer.
    stall: bool,
    /// Send answers slowly: 64 KiB every 50 ms.
    slow: bool,
}

struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    /// Answers written to the end.
    finished: Arc<Mutex<usize>>,
}

impl Server {
    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
    /// No request to this server carried the token in any form.
    fn never_saw_the_token(&self) {
        for s in self.seen() {
            assert!(
                !s.head.contains(CANARY) && !s.head.contains(&basic()[6..]),
                "{} {} carried the token: {}",
                s.method,
                s.path,
                s.head
            );
        }
    }
}

/// A server for the bare repositories under `root`.
fn server(root: &Path, mode: Mode) -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let finished: Arc<Mutex<usize>> = Arc::default();
    let (root, keep, done) = (root.to_path_buf(), seen.clone(), finished.clone());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (root, keep, mode, done) = (root.clone(), keep.clone(), mode.clone(), done.clone());
            std::thread::spawn(move || {
                if answer(s, &root, &keep, &mode).is_ok() {
                    *done.lock().unwrap() += 1;
                }
            });
        }
    });
    Server {
        port,
        seen,
        finished,
    }
}

fn answer(s: TcpStream, root: &Path, seen: &Mutex<Vec<Seen>>, mode: &Mode) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut head = String::new();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h)?;
        if h == "\r\n" || h.is_empty() {
            break;
        }
        head.push_str(&h);
        if let Some((k, v)) = h.trim_end().split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let mut body = Vec::new();
    if let Some(n) = get("content-length").and_then(|n| n.parse::<usize>().ok()) {
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    } else if get("transfer-encoding").is_some_and(|t| t.contains("chunked")) {
        loop {
            let mut sz = String::new();
            r.read_line(&mut sz)?;
            let n = usize::from_str_radix(sz.trim(), 16).unwrap_or(0);
            let mut chunk = vec![0; n + 2];
            r.read_exact(&mut chunk)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    }
    seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: target.clone(),
        auth: get("authorization"),
        head: head.clone(),
    });
    let mut w = s;
    if mode.stall {
        std::thread::sleep(Duration::from_secs(120));
        return Ok(());
    }
    if let Some((code, to, on_post)) = &mode.redirect
        && (method == "POST") == *on_post
    {
        return write!(
            w,
            "HTTP/1.1 {code} Moved\r\nLocation: {to}{target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }
    if mode.ask && get("authorization") != Some(basic()) {
        return write!(
            w,
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"fake\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut c = std::process::Command::new("git");
    c.arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", &method)
        .env("CONTENT_TYPE", get("content-type").unwrap_or_default())
        .env("CONTENT_LENGTH", body.len().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(p) = get("git-protocol") {
        c.env("GIT_PROTOCOL", p);
    }
    if let Some(e) = get("content-encoding") {
        c.env("HTTP_CONTENT_ENCODING", e);
    }
    let mut child = c.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let feed = std::thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let out = child.wait_with_output()?.stdout;
    let _ = feed.join();
    let split = out
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4))
        .or_else(|| out.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)))
        .unwrap_or((out.len(), 0));
    let (cgi, rest) = (
        String::from_utf8_lossy(&out[..split.0]).to_string(),
        &out[split.0 + split.1..],
    );
    let mut status = "200 OK".to_string();
    let mut resp = String::new();
    for l in cgi.lines() {
        if let Some((k, v)) = l.split_once(':') {
            if k.eq_ignore_ascii_case("status") {
                status = v.trim().to_string();
            } else {
                resp.push_str(&format!("{}: {}\r\n", k.trim(), v.trim()));
            }
        }
    }
    write!(
        w,
        "HTTP/1.1 {status}\r\n{resp}Content-Length: {}\r\nConnection: close\r\n\r\n",
        rest.len()
    )?;
    if mode.slow {
        for c in rest.chunks(64 * 1024) {
            w.write_all(c)?;
            w.flush()?;
            std::thread::sleep(Duration::from_millis(50));
        }
        return Ok(());
    }
    w.write_all(rest)
}

// ---------------------------------------------------------------- repos

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A bare repository `root/<full>.git` on `main`, its one commit made by
/// `fill` in a work folder.
fn bare(root: &Path, full: &str, fill: impl FnOnce(&Path)) {
    bare_with(root, full, fill, &[]);
}

/// [`bare`], with gitlinks (path, commit) added to the commit.
fn bare_with(root: &Path, full: &str, fill: impl FnOnce(&Path), links: &[(&str, &str)]) {
    let work = root.join(format!("work-{}", full.replace('/', "-")));
    std::fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("README.md"), "hello\n").unwrap();
    fill(&work);
    git_in(&work, &["add", "-A"]);
    for (path, sha) in links {
        git_in(
            &work,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sha},{path}"),
            ],
        );
    }
    git_in(&work, &["commit", "-q", "-m", "first"]);
    let dest = root.join(format!("{full}.git"));
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    git_in(
        root,
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
    );
}

// ---------------------------------------------------------------- rig

struct Rig {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    data: PathBuf,
    projects: PathBuf,
    policy: NodePolicy,
    made: Made,
}

fn rig(hosts: &[String]) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let (ws, data) = (base.join("ws"), base.join("data"));
    for d in [&ws, &data] {
        std::fs::create_dir_all(d).unwrap();
    }
    let projects = ws.join("projects");
    let mut policy = NodePolicy {
        workspace_roots: vec![ws.clone()],
        ..Default::default()
    };
    policy.skills.roots.clear();
    policy.projects.dir = Some(projects.clone());
    policy.limits.projects_per_hour = 100;
    policy.forge.hosts = hosts.to_vec();
    policy.protect(&data);
    policy.locate_projects(&data).unwrap();
    Rig {
        _tmp: tmp,
        base,
        data,
        projects,
        policy,
        made: Made::default(),
    }
}

fn req(host: &str, full: &str, private: bool) -> CloneRequest {
    CloneRequest {
        name: "Cloned".into(),
        host: host.into(),
        full_name: full.into(),
        username: private.then(|| USER.into()),
        token: private.then(|| Secret::new(CANARY.into())),
    }
}

fn bounds() -> Bounds {
    Bounds {
        timeout: Duration::from_secs(60),
        max_bytes: 64 * 1024 * 1024,
        every: Duration::from_millis(100),
    }
}

impl Rig {
    async fn clone(&self, r: CloneRequest) -> Result<PathBuf, String> {
        clone(&self.policy, &self.made, r, bounds(), &projects::suffix).await
    }

    /// Nothing made: the staging folder empty, the projects folder empty
    /// (or not made).
    fn nothing_kept(&self) {
        let staging = self.data.join(ProjectsPolicy::STAGING);
        assert_eq!(
            std::fs::read_dir(&staging).unwrap().count(),
            0,
            "the staging folder keeps something"
        );
        if self.projects.exists() {
            assert_eq!(std::fs::read_dir(&self.projects).unwrap().count(), 0);
        }
    }
}

/// Every file under `dir` (not following links), with the token in any
/// form.
fn files_with_token(dir: &Path) -> Vec<PathBuf> {
    let b = basic();
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            std::fs::read(e.path()).is_ok_and(|d| {
                let has = |n: &[u8]| d.windows(n.len()).any(|w| w == n);
                has(CANARY.as_bytes()) || has(&b.as_bytes()[6..])
            })
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn git_supports_proactive() -> bool {
    let out = std::process::Command::new("git")
        .arg("version")
        .output()
        .unwrap();
    version_at_least(&String::from_utf8_lossy(&out.stdout), PROACTIVE_SINCE)
}

// ---------------------------------------------------------------- tests

/// A public repository: cloned with no credential at all (no request
/// carries one, no helper is configured), `origin` the clean URL, the
/// folder in the projects folder, nothing left in staging.
#[tokio::test]
async fn a_public_clone_sends_no_credential_and_keeps_a_clean_origin() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    let p = r.clone(req(&a.host(), "o/pub", false)).await.unwrap();
    assert_eq!(p.parent(), Some(r.projects.as_path()));
    assert!(
        p.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("cloned-")
    );
    assert_eq!(
        std::fs::read_to_string(p.join("README.md")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        git_in(&p, &["config", "--get", "remote.origin.url"]),
        format!("{}/o/pub.git", a.base())
    );
    assert!(a.seen().iter().all(|s| s.auth.is_none()), "{:?}", a.seen());
    assert!(!a.seen().is_empty());
    let cfg = std::fs::read_to_string(p.join(".git/config")).unwrap();
    assert!(!cfg.contains("credential") && !cfg.contains('@'), "{cfg}");
    let hooks = p.join(".git/hooks");
    assert!(!hooks.exists() || std::fs::read_dir(&hooks).unwrap().next().is_none());
    r.nothing_kept_but(&p);
    assert_eq!(r.made_count(), 1, "a clone counts against the hour");
}

impl Rig {
    fn nothing_kept_but(&self, keep: &Path) {
        let staging = self.data.join(ProjectsPolicy::STAGING);
        assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);
        let names: Vec<_> = std::fs::read_dir(&self.projects)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(names, vec![keep.to_path_buf()]);
    }
    fn made_count(&self) -> usize {
        // `room(k)` fails while k or more were made in the hour.
        (1..).find(|k| self.made.room(*k).is_ok()).unwrap() - 1
    }
}

/// A private repository, both ways git may ask: proactively (git 2.46 or
/// later sends the credential with its first request) and with the 401
/// round trip of older git. The server gets the token; nothing in the new
/// folder, `.git/config` included, nor in the node's data directory, holds
/// it; `origin` is the clean URL.
#[tokio::test]
async fn a_private_clone_gets_the_token_through_the_helper_only() {
    for old in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        bare(tmp.path(), "o/secret", |_| {});
        let a = server(
            tmp.path(),
            Mode {
                ask: true,
                ..Default::default()
            },
        );
        let r = rig(&[a.host()]);
        pretend_old_git(old);
        let res = r.clone(req(&a.host(), "o/secret", true)).await;
        pretend_old_git(false);
        let p = res.unwrap_or_else(|e| panic!("old={old}: {e}"));
        let seen = a.seen();
        if !old && git_supports_proactive() {
            assert_eq!(seen[0].auth, Some(basic()), "proactive: {seen:?}");
            assert!(
                seen.iter().all(|s| s.auth.is_some()),
                "no 401 round trip: {seen:?}"
            );
        } else {
            assert_eq!(seen[0].auth, None, "the 401 round trip: {seen:?}");
            assert!(seen.iter().any(|s| s.auth == Some(basic())));
        }
        assert_eq!(
            git_in(&p, &["config", "--get", "remote.origin.url"]),
            format!("{}/o/secret.git", a.base())
        );
        assert_eq!(files_with_token(&p), Vec::<PathBuf>::new());
        assert_eq!(files_with_token(&r.data), Vec::<PathBuf>::new());
        r.nothing_kept_but(&p);
    }
}

/// A redirect from the listed host to another one (301, 302, 307; on the
/// first GET and on the POST; with the proactive credential and with the
/// 401 round trip): refused, the other host is never asked (and asks for a
/// credential itself, which the helper would refuse), nothing kept.
#[tokio::test]
async fn a_redirect_to_another_host_never_carries_the_token() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/secret", |_| {});
    let b = server(
        tmp.path(),
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    for code in [301u16, 302, 307] {
        for on_post in [false, true] {
            for old in [false, true] {
                let a = server(
                    tmp.path(),
                    Mode {
                        ask: true,
                        redirect: Some((code, b.base(), on_post)),
                        ..Default::default()
                    },
                );
                let r = rig(&[a.host()]);
                pretend_old_git(old);
                let res = r.clone(req(&a.host(), "o/secret", true)).await;
                pretend_old_git(false);
                let at = format!("{code} on_post={on_post} old={old}");
                let e = res.expect_err(&at);
                assert!(e.contains("git clone failed"), "{at}: {e}");
                assert!(!e.contains(CANARY), "{at}: {e}");
                assert!(!a.seen().is_empty(), "{at}");
                r.nothing_kept();
            }
        }
    }
    b.never_saw_the_token();
    assert_eq!(
        b.seen().len(),
        0,
        "the other host was asked: {:?}",
        b.seen()
    );
}

/// The processes of a clone that stalls: neither git nor any process of
/// its group has the token in its arguments or environment (the helper has
/// run: the server got the first request, with the credential where git
/// sends it first); at the time limit git's whole group is ended, nothing
/// is kept and the error names the bound.
#[tokio::test]
async fn a_stalled_clone_has_no_token_in_proc_and_ends_at_the_time_limit() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/secret", |_| {});
    let a = server(
        tmp.path(),
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let r = rig(&[a.host()]);
    let needle = a.host();
    let mut b = bounds();
    b.timeout = Duration::from_secs(4);
    let clone = clone(
        &r.policy,
        &r.made,
        req(&a.host(), "o/secret", true),
        b,
        &projects::suffix,
    );
    let watch = async {
        // The server saw the request (the helper ran before it, where git
        // asks first), and git and its remote helper are alive.
        let mut pids = Vec::new();
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(30)).await;
            pids = pids_with(&needle);
            if !a.seen().is_empty() && pids.len() >= 2 {
                break;
            }
        }
        assert!(!a.seen().is_empty(), "the server was never asked");
        assert!(!pids.is_empty(), "no git process found");
        let group = pgrp(pids[0]);
        let members: Vec<i32> = all_pids()
            .into_iter()
            .filter(|p| pgrp(*p) == group)
            .collect();
        assert!(members.len() >= 2, "{members:?}");
        for p in &members {
            for what in ["cmdline", "environ"] {
                let d = std::fs::read(format!("/proc/{p}/{what}")).unwrap_or_default();
                assert!(
                    !d.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()),
                    "the token is in /proc/{p}/{what}"
                );
            }
        }
        members
    };
    let (res, members) = tokio::join!(clone, watch);
    let e = res.unwrap_err();
    assert!(e.contains("clone_timeout_s"), "{e}");
    assert!(!e.contains(CANARY));
    for _ in 0..50 {
        if pids_with(&needle).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        pids_with(&needle),
        Vec::<i32>::new(),
        "git is still running"
    );
    for p in members {
        let alive = std::fs::read_to_string(format!("/proc/{p}/stat"))
            .is_ok_and(|s| !s.split(") ").nth(1).unwrap_or("").starts_with('Z'));
        assert!(!alive, "{p} of the clone's group is still running");
    }
    r.nothing_kept();
    assert_eq!(r.made_count(), 0, "a failed clone does not count");
}

fn all_pids() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect()
}

/// Processes whose arguments contain `needle`.
fn pids_with(needle: &str) -> Vec<i32> {
    all_pids()
        .into_iter()
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .is_ok_and(|d| d.windows(needle.len()).any(|w| w == needle.as_bytes()))
        })
        .collect()
}

fn pgrp(pid: i32) -> i32 {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            let rest = s.rsplit_once(") ")?.1.to_string();
            rest.split_whitespace().nth(2)?.parse().ok()
        })
        .unwrap_or(-1)
}

/// A hostile master: each request is refused before git starts (a
/// stand-in git leaves a marker if it ever runs), and nothing reaches the
/// server or the disk.
#[tokio::test]
async fn hostile_requests_are_refused_before_git_starts() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/r", |_| {});
    let a = server(tmp.path(), Mode::default());
    let mut r = rig(&[a.host(), "github.com".into()]);
    let marker = r.base.join("git-ran");
    let fake = r.base.join("git");
    crate::test_script::script(
        &fake,
        &format!("#!/bin/sh\ntouch {}\nexec git \"$@\"\n", marker.display()),
    );
    crate::git::use_test_git(Some(fake));
    let h = a.host();
    let long = format!("o/{}", "a".repeat(600));
    let mut cases: Vec<(String, CloneRequest)> = vec![
        ("unlisted host".into(), req("gitlab.com", "o/r", false)),
        (
            "unlisted host with a token".into(),
            req("evil.example", "o/r", true),
        ),
        ("unlisted port".into(), req("127.0.0.1:1", "o/r", true)),
        (
            "listed host's other port".into(),
            req("github.com:443", "o/r", false),
        ),
        ("upper case".into(), req("GitHub.com", "o/r", false)),
        ("trailing dot".into(), req("github.com.", "o/r", false)),
        ("user info".into(), req(&format!("x@{h}"), "o/r", false)),
        (
            "host with a path".into(),
            req(&format!("{h}/o"), "o/r", false),
        ),
        (
            "scheme in host".into(),
            req(&format!("https://{h}"), "o/r", false),
        ),
    ];
    for bad in [
        "o/../r",
        "../r",
        "o/..",
        "-o/r",
        "o/-r",
        "o@x/r",
        "o:x/r",
        "https://e/o/r",
        "file:///etc/x",
        "o/r x",
        "o/r\0",
        "o/r\n",
        "o",
        "o//r",
        "/o/r",
        "o/r/",
        "o/r.git",
        "o/r.atom",
        "o/.",
        long.as_str(),
        "ö/r",
    ] {
        cases.push((format!("full_name {bad:?}"), req(&h, bad, false)));
    }
    let mut no_user = req(&h, "o/r", true);
    no_user.username = None;
    cases.push(("token without a login".into(), no_user));
    let mut bad_user = req(&h, "o/r", true);
    bad_user.username = Some("a'b".into());
    cases.push(("quote in the login".into(), bad_user));
    let mut nl = req(&h, "o/r", true);
    nl.token = Some(Secret::new(format!("{CANARY}\nusername=evil")));
    cases.push(("line break in the token".into(), nl));
    let mut empty = req(&h, "o/r", true);
    empty.token = Some(Secret::new(String::new()));
    cases.push(("empty token".into(), empty));
    for (what, c) in cases {
        let e = r.clone(c).await.expect_err(&what);
        assert!(!e.contains(CANARY), "{what}: {e}");
        assert!(!marker.exists(), "{what}: git ran");
    }
    // `[forge] clone = false`, and a node without a projects folder.
    r.policy.forge.clone = false;
    assert_eq!(r.clone(req(&h, "o/r", true)).await.unwrap_err(), OFF);
    r.policy.forge.clone = true;
    r.policy.forge.hosts.clear();
    assert_eq!(r.clone(req(&h, "o/r", true)).await.unwrap_err(), OFF);
    r.policy.forge.hosts = vec![h.clone()];
    r.policy.projects.located = None;
    assert_eq!(r.clone(req(&h, "o/r", true)).await.unwrap_err(), OFF);
    crate::git::use_test_git(None);
    assert!(!marker.exists());
    assert!(a.seen().is_empty(), "{:?}", a.seen());
    assert_eq!(files_with_token(&r.base), Vec::<PathBuf>::new());
}

/// A hostile repository and a hostile `HOME` (a `.gitconfig` with a smudge
/// filter, a hooks folder, `insteadOf` to another server, a proxy, a
/// credential helper and recursive submodules): nothing runs (no marker),
/// no submodule is fetched (from a local path or another host), the other
/// server is never asked, a symlink to a folder outside stays a link and
/// nothing is written through it.
#[tokio::test]
async fn a_hostile_repository_runs_and_fetches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let outside = base.join("outside");
    let marker = base.join("marker");
    std::fs::create_dir_all(&outside).unwrap();
    bare(&base, "o/local-sub", |_| {});
    let b = server(&base, Mode::default());
    let sha = git_in(&base.join("o/local-sub.git"), &["rev-parse", "HEAD"]);
    let local = base.join("o/local-sub.git");
    let (m, out, bb, l, s) = (
        marker.display().to_string(),
        outside.display().to_string(),
        b.base(),
        local.display().to_string(),
        sha.clone(),
    );
    bare_with(
        &base,
        "o/hostile",
        |w| {
            std::fs::write(
                w.join(".gitmodules"),
                format!(
                    "[submodule \"near\"]\n\tpath = near\n\turl = {l}\n\
                 [submodule \"far\"]\n\tpath = far\n\turl = {bb}/o/local-sub.git\n"
                ),
            )
            .unwrap();
            std::fs::write(w.join(".gitattributes"), "* filter=evil\n").unwrap();
            std::fs::create_dir_all(w.join(".githooks")).unwrap();
            std::fs::write(
                w.join(".githooks/post-checkout"),
                format!("#!/bin/sh\ntouch {m}\n"),
            )
            .unwrap();
            std::os::unix::fs::symlink(&out, w.join("out")).unwrap();
        },
        &[("near", &s), ("far", &s)],
    );
    let a = server(&base, Mode::default());
    let home = base.join("home");
    std::fs::create_dir_all(home.join("hooks")).unwrap();
    crate::test_script::script(
        &home.join("hooks/post-checkout"),
        &format!("#!/bin/sh\ntouch {m}\n"),
    );
    std::fs::write(
        home.join(".gitconfig"),
        format!(
            "[filter \"evil\"]\n\tsmudge = sh -c 'touch {m}; cat'\n\trequired = true\n\
             [core]\n\thooksPath = {h}/hooks\n\
             [url \"{bb}/\"]\n\tinsteadOf = {ab}/\n\
             [http]\n\tproxy = {bb}\n\
             [credential]\n\thelper = !touch {m}\n\
             [submodule]\n\trecurse = true\n",
            h = home.display(),
            ab = a.base(),
            m = marker.display(),
        ),
    )
    .unwrap();
    let r = rig(&[a.host()]);
    use_test_home(Some(home));
    let res = r.clone(req(&a.host(), "o/hostile", false)).await;
    use_test_home(None);
    let p = res.unwrap();
    assert!(!marker.exists(), "a filter, hook or helper ran");
    assert!(
        b.seen().is_empty(),
        "the other server was asked: {:?}",
        b.seen()
    );
    assert!(!a.seen().is_empty());
    for sub in ["near", "far"] {
        let d = p.join(sub);
        assert!(
            !d.exists() || std::fs::read_dir(&d).unwrap().next().is_none(),
            "{sub} was fetched"
        );
    }
    assert!(!p.join(".git/modules").exists());
    // The submodules are there as gitlinks (the test would prove nothing
    // otherwise), just not fetched.
    let links = git_in(&p, &["ls-tree", "HEAD", "near", "far"]);
    assert_eq!(links.matches("160000 commit").count(), 2, "{links}");
    let link = std::fs::symlink_metadata(p.join("out")).unwrap();
    assert!(link.file_type().is_symlink());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    assert!(
        p.join(".githooks/post-checkout").exists(),
        "the tree is as it is"
    );
}

/// A repository bigger than `[limits] clone_max_mb`: ended, nothing kept,
/// the error names the bound, the hour's count untouched.
#[tokio::test]
async fn a_clone_past_the_size_cap_is_ended_and_removed() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/big", |w| {
        let mut b = vec![0u8; 3 * 1024 * 1024];
        getrandom::fill(&mut b).unwrap();
        std::fs::write(w.join("blob.bin"), b).unwrap();
    });
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    let mut b = bounds();
    b.max_bytes = 1024 * 1024;
    let e = clone(
        &r.policy,
        &r.made,
        req(&a.host(), "o/big", false),
        b,
        &projects::suffix,
    )
    .await
    .unwrap_err();
    assert!(e.contains("clone_max_mb"), "{e}");
    r.nothing_kept();
    assert_eq!(r.made_count(), 0);
}

/// The cap is checked while git runs, not only after: a repository that
/// arrives slowly is ended mid-transfer (the server never finishes writing
/// its pack), not once it is all on disk.
#[tokio::test]
async fn the_size_cap_ends_git_while_it_is_still_fetching() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/big", |w| {
        let mut b = vec![0u8; 8 * 1024 * 1024];
        getrandom::fill(&mut b).unwrap();
        std::fs::write(w.join("blob.bin"), b).unwrap();
    });
    let a = server(
        tmp.path(),
        Mode {
            slow: true,
            ..Default::default()
        },
    );
    let r = rig(&[a.host()]);
    let mut b = bounds();
    b.max_bytes = 1024 * 1024;
    let e = clone(
        &r.policy,
        &r.made,
        req(&a.host(), "o/big", false),
        b,
        &projects::suffix,
    )
    .await
    .unwrap_err();
    assert!(e.contains("clone_max_mb"), "{e}");
    // Let the server notice the closed connection.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let posts = a.seen().iter().filter(|s| s.method == "POST").count();
    assert!(posts >= 1);
    assert!(
        *a.finished.lock().unwrap() < a.seen().len(),
        "every answer was sent to the end: git ran until the pack was all there"
    );
    r.nothing_kept();
}

/// A taken folder name gets the next suffix; nothing is overwritten.
#[tokio::test]
async fn a_taken_name_gets_new_characters() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    std::fs::create_dir_all(r.projects.join("cloned-aaaa")).unwrap();
    std::fs::write(r.projects.join("cloned-aaaa/mine"), "keep").unwrap();
    let left = Mutex::new(VecDeque::from(["aaaa".to_string(), "bbbb".to_string()]));
    let next = move || {
        left.lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "zzzz".into())
    };
    let p = clone(
        &r.policy,
        &r.made,
        req(&a.host(), "o/pub", false),
        bounds(),
        &next,
    )
    .await
    .unwrap();
    assert_eq!(p, r.projects.join("cloned-bbbb"));
    assert_eq!(
        std::fs::read_to_string(r.projects.join("cloned-aaaa/mine")).unwrap(),
        "keep"
    );
}

/// A node that stopped mid-clone: its staging folder is emptied at the
/// next start, so no half clone survives.
#[test]
fn a_restart_sweeps_a_half_clone() {
    let mut r = rig(&["github.com".into()]);
    let half = r
        .data
        .join(ProjectsPolicy::STAGING)
        .join("build-0011223344556677");
    std::fs::create_dir_all(half.join(".git/objects/pack")).unwrap();
    std::fs::write(half.join(".git/objects/pack/tmp_pack_x"), vec![0u8; 4096]).unwrap();
    let data = r.data.clone();
    r.policy.locate_projects(&data).unwrap();
    assert_eq!(
        std::fs::read_dir(data.join(ProjectsPolicy::STAGING))
            .unwrap()
            .count(),
        0
    );
}

/// The command a private clone runs: the token in no argument and in no
/// environment variable; no `HOME`, no proxy variable; the hardened
/// configuration; the URL without a credential; the production scheme is
/// https and allows only https.
#[tokio::test]
async fn the_clone_command_carries_no_token_and_the_hardened_configuration() {
    let (fd, ino) = token_pipe(&Secret::new(CANARY.into())).unwrap();
    let h = helper(PROD_SCHEME, "github.com", "o/r.git", USER, ino).unwrap();
    assert!(!h.contains(CANARY));
    let cfg = clone_config(PROD_SCHEME, Some(&h), true);
    for want in [
        "protocol.https.allow=always",
        "protocol.file.allow=never",
        "http.followRedirects=false",
        "http.proxy=",
        "credential.useHttpPath=true",
        "core.hooksPath=/dev/null",
        "core.fsmonitor=false",
        "submodule.recurse=false",
        "transfer.fsckObjects=true",
        // Review of PR 163, L3: nothing that is off only by git's default
        // and could send git to another URL.
        "transfer.bundleURI=false",
        "fetch.bundleURI=",
        "fetch.uriProtocols=",
        "http.proactiveAuth=basic",
    ] {
        assert!(cfg.iter().any(|c| c == want), "{want} missing: {cfg:?}");
    }
    assert!(
        !cfg.iter().any(|c| c.starts_with("protocol.http.")),
        "{cfg:?}"
    );
    assert_eq!(
        cfg.iter()
            .filter(|c| c.starts_with("credential.helper="))
            .count(),
        1
    );
    assert_eq!(
        url(PROD_SCHEME, "github.com", "o/r"),
        "https://github.com/o/r.git"
    );
    // The token is in the pipe, readable once.
    let mut f = std::fs::File::from(fd);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, format!("{CANARY}\n"));
    // The base: the hardened environment, the reset helper list.
    let c = crate::git::hardened(Path::new("git"));
    let envs: Vec<_> = c.as_std().get_envs().collect();
    let get = |k: &str| {
        envs.iter()
            .find(|(n, _)| *n == k)
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    };
    assert_eq!(get("GIT_CONFIG_GLOBAL").as_deref(), Some("/dev/null"));
    assert_eq!(get("GIT_CONFIG_NOSYSTEM").as_deref(), Some("1"));
    assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    assert_eq!(get("GIT_ASKPASS").as_deref(), Some(""));
    for k in [
        "http_proxy",
        "https_proxy",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "all_proxy",
    ] {
        assert!(get(k).is_none(), "{k}");
    }
    let args: Vec<_> = c
        .as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.iter().any(|a| a == "credential.helper="));
    assert!(args.iter().any(|a| a == "protocol.allow=never"));
}

/// The helper alone, run under a parent that holds the token's pipe at
/// descriptor 0, as git does: it answers `get` for exactly the scheme,
/// host and path it was made for, and for nothing else (another host, a
/// port, another path, another scheme, `store`, `erase`), leaving the
/// token unread.
#[test]
fn the_helper_answers_only_for_its_own_host_and_path() {
    let run = |op: &str, input: &str| -> (String, String) {
        let (fd, ino) = token_pipe(&Secret::new(CANARY.into())).unwrap();
        let h = helper("https", "github.com", "o/r.git", USER, ino).unwrap();
        let snippet = h.strip_prefix('!').unwrap().to_string();
        // The outer shell plays git: the token's pipe at its descriptor 0,
        // the helper its child with the request on its own stdin. After
        // the helper, the outer shell reads what is left of the pipe.
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(
                "printf '%s' \"$REQ\" | sh -c \"$HELPER \\\"\\$@\\\"\" helper \"$OP\"; \
                 printf '|LEFT='; cat",
            )
            .env("REQ", input)
            .env("HELPER", snippet)
            .env("OP", op)
            .stdin(std::process::Stdio::from(fd))
            .output()
            .unwrap();
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        let (said, left) = s.split_once("|LEFT=").unwrap();
        (said.to_string(), left.to_string())
    };
    let ok = "protocol=https\nhost=github.com\npath=o/r.git\n\n";
    let (said, left) = run("get", ok);
    assert_eq!(said, format!("username={USER}\npassword={CANARY}\n"));
    assert_eq!(left, "");
    for (op, input) in [
        ("get", "protocol=https\nhost=evil.example\npath=o/r.git\n\n"),
        (
            "get",
            "protocol=https\nhost=github.com:8443\npath=o/r.git\n\n",
        ),
        (
            "get",
            "protocol=https\nhost=github.com.evil.example\npath=o/r.git\n\n",
        ),
        (
            "get",
            "protocol=https\nhost=github.com\npath=o/other.git\n\n",
        ),
        ("get", "protocol=https\nhost=github.com\n\n"),
        ("get", "protocol=http\nhost=github.com\npath=o/r.git\n\n"),
        ("get", "host=github.com\npath=o/r.git\n\n"),
        ("store", ok),
        ("erase", ok),
    ] {
        let (said, left) = run(op, input);
        assert_eq!(said, "", "{op} {input:?}");
        assert_eq!(
            left,
            format!("{CANARY}\n"),
            "{op} {input:?}: the token was read"
        );
    }
    // Nothing unchecked is ever written into it.
    assert!(helper("https", "github.com", "o/r'.git", USER, 1).is_err());
    assert!(helper("https", "github.com", "o/r.git", "a b", 1).is_err());
}

#[test]
fn git_errors_lose_the_token_and_stay_short() {
    let r = req("github.com", "o/r", true);
    let s = format!(
        "fatal: x {CANARY}\nremote: {}\n{}",
        &basic()[6..],
        "y".repeat(5000)
    );
    let e = clean_error(&s, &r);
    assert!(!e.contains(CANARY) && !e.contains(&basic()[6..]), "{e}");
    assert!(e.chars().count() <= ERROR_MAX);
    assert_eq!(clean_error("", &r), "git said nothing");
}

#[test]
fn versions_and_names() {
    assert!(version_at_least("git version 2.56.1\n", (2, 46)));
    assert!(version_at_least(
        "git version 2.46.0 (Apple Git-1)",
        (2, 46)
    ));
    assert!(version_at_least("git version 3.0.0", (2, 46)));
    assert!(!version_at_least("git version 2.45.9", (2, 46)));
    assert!(!version_at_least("nonsense", (2, 46)));
    for ok in ["o/r", "Ada-L/My.Repo_1", "acme/.github", "g/s/s2/p", "a/b-"] {
        assert!(valid_full_name(ok), "{ok}");
    }
    let deep = vec!["a"; 22].join("/");
    assert!(!valid_full_name(&deep));
    assert!(valid_full_name(&vec!["a"; 21].join("/")));
    for ok in [
        "github.com",
        "git.example.com",
        "127.0.0.1:8080",
        "gitlab.com:443",
        "a-b.c",
    ] {
        assert!(crate::policy::valid_forge_host(ok), "{ok}");
    }
    for bad in [
        "",
        "GitHub.com",
        "github.com.",
        "x@github.com",
        "github.com/o",
        "https://github.com",
        "github.com:",
        "github.com:0",
        "github.com:65536",
        "github.com:080",
        "[::1]",
        "-a.com",
        "a..com",
        "1.2.3",
        "01.2.3.4",
        "github.com:1:2",
        "github .com",
    ] {
        assert!(!crate::policy::valid_forge_host(bad), "{bad}");
    }
}

/// `[forge]` in node.toml: the defaults, and entries that do not parse as
/// a host stop the node.
#[test]
fn forge_policy_is_checked() {
    let p = NodePolicy::default();
    assert!(p.forge.clone);
    assert_eq!(p.forge.hosts, vec!["github.com", "gitlab.com"]);
    assert_eq!(p.limits.clone_timeout_s, 600);
    assert_eq!(p.limits.clone_max_mb, 2048);
    let parse = |s: &str| -> Result<NodePolicy, String> {
        let p: NodePolicy = toml::from_str(s).map_err(|e| e.to_string())?;
        p.validate().map(|()| p)
    };
    let p = parse("[forge]\nclone = false\nhosts = [\"git.example.com:8443\"]\n").unwrap();
    assert!(!p.forge.clones());
    assert!(!p.forge.allows("git.example.com:8443"));
    for bad in [
        "[forge]\nhosts = [\"https://x.com\"]\n",
        "[forge]\nhosts = [\"X.com\"]\n",
        "[forge]\nhosts = [\"a.com\", \"a.com\"]\n",
        "[forge]\nurl = \"x\"\n",
        "[limits]\nclone_timeout_s = 0\n",
        "[limits]\nclone_max_mb = 0\n",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

// ------------------------------------------------- through the runtime

/// Logs of this test thread at TRACE.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Through the node's command loop: it announces `forge-clone:<limit>`
/// (never a host); a private clone is answered by one `project.created`;
/// a second clone while one runs is refused; a refusal names no token;
/// afterwards the token is in no file of the node's data directory (its
/// database: events, the audit log), in no log line at TRACE and in no
/// answer; the audit line names the host and repository.
#[tokio::test]
async fn through_the_runtime_the_token_is_nowhere_but_the_server() {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    use harness_protocol::{MasterToNode, NodeToMaster};
    let logs = Logs::default();
    let sink = logs.clone();
    let sub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/secret", |_| {});
    let a = server(
        tmp.path(),
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let stall = server(
        tmp.path(),
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let gone = server(
        tmp.path(),
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let mut r = rig(&[a.host(), stall.host(), gone.host()]);
    r.policy.limits.clone_timeout_s = 3;
    let cfg = crate::NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        policy: r.policy.clone(),
        store: harness_store::Store::open(&r.data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let store = cfg.store.clone();
    let node = crate::NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let caps = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("{other:?}"),
    };
    assert!(caps.contains(&"forge-clone:3".to_string()), "{caps:?}");
    assert!(!caps.iter().any(|c| c.contains("127.0.0.1")), "{caps:?}");
    let ask = |id: &str, host: &str, full: &str| MasterToNode::ProjectClone {
        req_id: id.into(),
        name: "Cloned".into(),
        host: host.into(),
        full_name: full.into(),
        username: Some(USER.into()),
        token: Some(Secret::new(CANARY.into())),
    };
    let mut answers = Vec::new();
    let mut wait_for = async |link: &mut crate::NodeLink, id: &str| loop {
        let m = tokio::time::timeout(Duration::from_secs(60), link.from_node.recv())
            .await
            .expect("no answer")
            .expect("link closed");
        match m {
            NodeToMaster::ProjectCreated {
                req_id,
                path,
                error,
            } if req_id == id => {
                answers.push(format!("{path:?} {error:?}"));
                return (path, error);
            }
            NodeToMaster::Ack { req_id, .. } => panic!("acked {req_id}"),
            _ => {}
        }
    };
    link.to_node
        .try_send(ask("ok", &a.host(), "o/secret"))
        .unwrap();
    let (path, error) = wait_for(&mut link, "ok").await;
    let p = PathBuf::from(path.unwrap_or_else(|| panic!("{error:?}")));
    assert!(p.join("README.md").exists());
    // One at a time: the stalled one holds the slot, the next is refused.
    link.to_node
        .try_send(ask("slow", &stall.host(), "o/secret"))
        .unwrap();
    for _ in 0..200 {
        if !stall.seen().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    link.to_node
        .try_send(ask("busy", &a.host(), "o/secret"))
        .unwrap();
    let (path, error) = wait_for(&mut link, "busy").await;
    assert!(path.is_none() && error.unwrap().contains("cloning another"));
    let (_, error) = wait_for(&mut link, "slow").await;
    assert!(error.unwrap().contains("clone_timeout_s"));
    // A refusal: a host the node does not list.
    link.to_node
        .try_send(ask("no", "evil.example", "o/secret"))
        .unwrap();
    let (_, error) = wait_for(&mut link, "no").await;
    assert!(error.unwrap().contains("[forge] hosts"));
    // The master goes away mid-clone: the clone ends at its limit all the
    // same, and leaves no half folder (staging empty, only the first
    // clone's folder in the projects folder) and no git process.
    link.to_node
        .try_send(ask("gone", &gone.host(), "o/secret"))
        .unwrap();
    for _ in 0..200 {
        if !gone.seen().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(link);
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if pids_with(&gone.host()).is_empty()
            && std::fs::read_dir(r.data.join(ProjectsPolicy::STAGING))
                .unwrap()
                .next()
                .is_none()
        {
            break;
        }
    }
    assert_eq!(pids_with(&gone.host()), Vec::<i32>::new());
    r.nothing_kept_but(&p);
    // The audit lines are written by their own task: wait for them.
    for _ in 0..100 {
        if node.inner.audit_log.pending().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for ans in &answers {
        assert!(!ans.contains(CANARY), "{ans}");
    }
    let logged = String::from_utf8_lossy(&logs.0.lock().unwrap()).to_string();
    assert!(!logged.contains(CANARY) && !logged.contains(&basic()[6..]));
    assert_eq!(files_with_token(&r.base), Vec::<PathBuf>::new());
    let audit: Vec<String> = store
        .audit_lines("project.clone")
        .unwrap()
        .into_iter()
        .map(|(_, d)| d)
        .collect();
    assert!(
        audit
            .iter()
            .any(|d| d.contains("\"full_name\":\"o/secret\"") && d.contains("\"ok\":true")),
        "{audit:?}"
    );
    assert!(audit.iter().all(|d| !d.contains(CANARY)));
}

/// Clones count against `[limits] projects_per_hour` with
/// `project.create`'s folders: at the cap, refused before git starts.
#[tokio::test]
async fn clones_count_against_the_hour() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(tmp.path(), Mode::default());
    let mut r = rig(&[a.host()]);
    r.policy.limits.projects_per_hour = 1;
    r.clone(req(&a.host(), "o/pub", false)).await.unwrap();
    let asked = a.seen().len();
    let e = r.clone(req(&a.host(), "o/pub", false)).await.unwrap_err();
    assert!(e.contains("projects_per_hour"), "{e}");
    assert_eq!(a.seen().len(), asked, "git ran past the cap");
}

// ------------------------------------------------- review of PR 163

/// The node owner's own `.netrc` (M1): libcurl reads `<home>/.netrc`,
/// finding the home through the password database when `HOME` is unset. A
/// fake folder with a `.netrc` is mounted over the node user's real home
/// for the one git command (a private mount namespace through bwrap; the
/// real home is never touched), and a public clone, no token, against a
/// server that asks: the owner's credential must never reach it.
#[tokio::test]
async fn the_node_owners_netrc_is_never_used() {
    let Some(bwrap) = ["/usr/bin/bwrap", "/bin/bwrap"]
        .into_iter()
        .find(|p| Path::new(p).exists())
    else {
        assert!(
            std::env::var_os("HARNESS_REQUIRE_BWRAP").is_none(),
            "bwrap is required"
        );
        return;
    };
    let me = nix::unistd::User::from_uid(nix::unistd::getuid())
        .unwrap()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(
        tmp.path(),
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let r = rig(&[a.host()]);
    let fake_home = r.base.join("fakehome");
    std::fs::create_dir_all(&fake_home).unwrap();
    std::fs::write(
        fake_home.join(".netrc"),
        "default login nodeowner password NETRC-SECRET-of-the-node-owner\n",
    )
    .unwrap();
    let wrap = r.base.join("git-in-ns");
    crate::test_script::script(
        &wrap,
        &format!(
            "#!/bin/sh\nexec {bwrap} --dev-bind / / --bind {} {} -- git \"$@\"\n",
            fake_home.display(),
            me.dir.display()
        ),
    );
    crate::git::use_test_git(Some(wrap));
    let res = r.clone(req(&a.host(), "o/pub", false)).await;
    crate::git::use_test_git(None);
    use base64::Engine as _;
    let netrc = base64::engine::general_purpose::STANDARD
        .encode("nodeowner:NETRC-SECRET-of-the-node-owner");
    for s in a.seen() {
        assert!(
            !s.head.contains(&netrc),
            "the node owner's .netrc went to the forge: {}",
            s.head
        );
    }
    assert!(res.is_err(), "the server asks; nobody may answer");
    r.nothing_kept();
}

/// The same `.netrc` in the `HOME` git is given would be used: what the
/// test above guards against is real on this machine's git and curl.
#[tokio::test]
async fn a_netrc_in_gits_home_would_be_used() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(
        tmp.path(),
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let r = rig(&[a.host()]);
    let home = r.base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".netrc"),
        "default login nodeowner password NETRC-SECRET-of-the-node-owner\n",
    )
    .unwrap();
    use_test_home(Some(home));
    let _ = r.clone(req(&a.host(), "o/pub", false)).await;
    use_test_home(None);
    use base64::Engine as _;
    let netrc = base64::engine::general_purpose::STANDARD
        .encode("nodeowner:NETRC-SECRET-of-the-node-owner");
    assert!(
        a.seen().iter().any(|s| s.head.contains(&netrc)),
        "curl did not read the .netrc: the guard above proves nothing here"
    );
}

/// What the clone's git gets in its environment (M1): exactly an
/// allow-list; `HOME`, `XDG_CONFIG_HOME` and `CURL_HOME` an empty folder
/// the node made in its own data directory (0700), removed afterwards.
#[tokio::test]
async fn the_clone_gets_an_allow_listed_environment_and_an_empty_home() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/pub", |_| {});
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    let dump = r.base.join("env.txt");
    let wrap = r.base.join("git-env");
    crate::test_script::script(
        &wrap,
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = clone ] && {{ env > {d}; \
             ls -A \"$HOME\" > {d}.home; stat -c %a \"$HOME\" > {d}.mode; }}; done\n\
             exec git \"$@\"\n",
            d = dump.display()
        ),
    );
    crate::git::use_test_git(Some(wrap));
    let res = r.clone(req(&a.host(), "o/pub", false)).await;
    crate::git::use_test_git(None);
    res.unwrap();
    let env = std::fs::read_to_string(&dump).unwrap();
    let vars: std::collections::BTreeMap<String, String> = env
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let allowed = [
        "PATH",
        "LANG",
        "TMPDIR",
        "HOME",
        "XDG_CONFIG_HOME",
        "CURL_HOME",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_TERMINAL_PROMPT",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_PAGER",
        "GIT_NO_LAZY_FETCH",
        "GIT_OPTIONAL_LOCKS",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        // Set by the shell that reads the list, not by the node.
        "PWD",
        "SHLVL",
        "OLDPWD",
        "_",
    ];
    for k in vars.keys() {
        assert!(allowed.contains(&k.as_str()), "{k} reached git");
    }
    let home = PathBuf::from(vars.get("HOME").expect("HOME is set"));
    assert!(
        home.starts_with(r.data.join(ProjectsPolicy::STAGING)),
        "{}",
        home.display()
    );
    assert_eq!(vars.get("XDG_CONFIG_HOME"), vars.get("HOME"));
    assert_eq!(vars.get("CURL_HOME"), vars.get("HOME"));
    assert_eq!(
        std::fs::read_to_string(format!("{}.home", dump.display())).unwrap(),
        "",
        "the home was not empty"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{}.mode", dump.display()))
            .unwrap()
            .trim(),
        "700"
    );
    assert!(!home.exists(), "the home is removed after the clone");
}

/// An empty repository on the forge (L1): the folder gets the same first
/// empty commit `project.create` makes, on the branch the remote names as
/// its default, so the harness can work in it and a push can start it.
#[tokio::test]
async fn an_empty_repository_gets_a_first_commit_on_its_default_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("o/empty.git");
    std::fs::create_dir_all(&dest).unwrap();
    git_in(&dest, &["init", "-q", "--bare", "-b", "trunk"]);
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    let p = r.clone(req(&a.host(), "o/empty", false)).await.unwrap();
    assert_eq!(git_in(&p, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git_in(&p, &["branch", "--show-current"]), "trunk");
    assert_eq!(
        git_in(&p, &["log", "-1", "--format=%an <%ae>|%s"]),
        format!("harness <harness@localhost>|{}", crate::git::FIRST_COMMIT)
    );
    assert_eq!(
        git_in(&p, &["config", "--get", "remote.origin.url"]),
        format!("{}/o/empty.git", a.base())
    );
    r.nothing_kept_but(&p);
}

/// A repository with refs whose `HEAD` names no commit (L1): refused, with
/// an error that says so, nothing kept.
#[tokio::test]
async fn a_repository_whose_head_names_no_commit_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/headless", |_| {});
    let b = tmp.path().join("o/headless.git");
    git_in(&b, &["branch", "-m", "main", "dev"]);
    git_in(&b, &["symbolic-ref", "HEAD", "refs/heads/gone"]);
    let a = server(tmp.path(), Mode::default());
    let r = rig(&[a.host()]);
    let e = r
        .clone(req(&a.host(), "o/headless", false))
        .await
        .unwrap_err();
    assert!(e.contains("names no commit"), "{e}");
    r.nothing_kept();
    assert_eq!(r.made_count(), 0);
}

/// The token did not reach git (another process took it from the pipe:
/// here a stand-in git that drains its standard input first) (L4): the
/// error says what that means and what to do, never the token.
#[tokio::test]
async fn a_credential_that_never_arrived_is_said_plainly() {
    let tmp = tempfile::tempdir().unwrap();
    bare(tmp.path(), "o/secret", |_| {});
    let a = server(
        tmp.path(),
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let r = rig(&[a.host()]);
    let wrap = r.base.join("git-thief");
    crate::test_script::script(
        &wrap,
        "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = clone ] && cat > /dev/null; done\nexec git \"$@\"\n",
    );
    crate::git::use_test_git(Some(wrap));
    let res = r.clone(req(&a.host(), "o/secret", true)).await;
    crate::git::use_test_git(None);
    let e = res.unwrap_err();
    assert!(e.contains("did not receive the credential"), "{e}");
    assert!(e.contains("Plugins → Connectors"), "{e}");
    assert!(!e.contains(CANARY));
    r.nothing_kept();
}
