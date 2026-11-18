//! The master's MCP client for custom connectors (ADOPTION_PLAN X.35,
//! PLAN.md T84) against a fake remote server (`fake_mcp`): every byte is
//! bounded, nothing remote reaches a log or an error, the requests carry
//! our headers only and go to the connector's origin only, the master
//! offers no capability, and nothing is left running after a check.

mod fake_mcp;

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fake_mcp::{CANARY, Fake, GOOD};
use harness_core::Secret;
use harness_master::connectors::client::Failure;
use harness_master::connectors::{self, CHECK_BYTES, Checked, MAX_TOOLS, Net};

/// Everything this process logs, at every level, unfiltered.
fn logs() -> Arc<Mutex<Vec<u8>>> {
    static BUF: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    BUF.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
        buf
    })
    .clone()
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().lock().unwrap()).to_string()
}

struct T {
    fake: Fake,
    net: Net,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let fake = fake_mcp::start(dir.path()).await;
    let net = Net {
        lookup: harness_master::remote::Lookup::table(fake.table()),
        allow_private: true,
        roots: vec![fake.cert_der.clone()],
        timeout: Duration::from_secs(3),
    };
    T {
        fake,
        net,
        _dir: dir,
    }
}

fn token(t: &str) -> Option<connectors::client::TokenHeader> {
    Some(connectors::token_header(None, &Secret::new(t.into())).unwrap())
}

async fn run(t: &T, path: &str, tok: Option<&str>) -> Checked {
    connectors::check(
        "conn",
        &format!("https://mcp.test/{path}"),
        tok.and_then(token),
        &t.net,
    )
    .await
}

/// After a check: no connection open at the fake, no task left on the
/// check's runtime, the budget respected.
async fn assert_clean(t: &T, c: &Checked) {
    assert_eq!(t.fake.settle().await, 0, "a connection was left open");
    assert_eq!(c.tasks_left, 0, "tasks were left: {}", c.tasks_left);
    assert!(c.bytes_read <= CHECK_BYTES, "{}", c.bytes_read);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_check_reads_what_the_server_offers_with_our_headers_only() {
    let t = start().await;
    let c = run(&t, "auth/mcp", Some(GOOD)).await;
    let o = c.result.as_ref().expect("the check works");
    assert_eq!(o.server_name.as_deref(), Some("fake"));
    assert_eq!(o.server_version.as_deref(), Some("1.2.3"));
    assert_eq!(
        o.instructions.as_deref(),
        Some("Use the tools<script>x</script> wisely"),
        "hidden characters go, the rest is text"
    );
    let names: Vec<&str> = o.tools.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["tool_0", "tool_1", "tool_2"]);
    assert_eq!(o.tools[0].agent_name, "mcp__conn__tool_0");
    assert!(!o.more);
    assert_clean(&t, &c).await;

    let seen = t.fake.seen();
    let rpcs: Vec<(String, Option<&str>)> =
        seen.iter().map(|s| (s.method.clone(), s.rpc())).collect();
    assert_eq!(
        rpcs,
        [
            ("POST".to_string(), Some("initialize")),
            ("POST".to_string(), Some("notifications/initialized")),
            ("POST".to_string(), Some("tools/list")),
            ("DELETE".to_string(), None),
        ],
        "the session is freed at the end"
    );
    assert!(
        seen.iter().all(|s| s.method != "GET"),
        "no standalone stream"
    );
    // The exact headers of each request (`accept-encoding` is reqwest's:
    // an answer is counted against the budget after decoding).
    let first = [
        "accept",
        "accept-encoding",
        "authorization",
        "content-length",
        "content-type",
        "host",
        "user-agent",
    ];
    assert_eq!(seen[0].names(), first, "initialize");
    let mut later: Vec<&str> = first.to_vec();
    later.extend(["mcp-protocol-version", "mcp-session-id"]);
    later.sort();
    assert_eq!(seen[1].names(), later, "initialized");
    assert_eq!(seen[1].header("mcp-protocol-version"), Some("2025-11-25"));
    assert_eq!(seen[2].names(), later, "tools/list");
    // reqwest's own `accept: */*` on a request that sets none.
    let mut del = vec![
        "accept",
        "accept-encoding",
        "authorization",
        "host",
        "mcp-protocol-version",
        "mcp-session-id",
        "user-agent",
    ];
    del.sort();
    assert_eq!(seen[3].names(), del, "DELETE");
    for s in &seen {
        assert_eq!(s.host, "mcp.test");
        assert_eq!(s.path, "/auth/mcp");
        assert_eq!(s.query, None);
        assert_eq!(
            s.header("authorization"),
            Some(format!("Bearer {GOOD}").as_str())
        );
        assert!(
            s.header("user-agent")
                .unwrap()
                .starts_with("rizzy-harness/")
        );
    }
    assert_eq!(
        t.fake.accepted.load(std::sync::atomic::Ordering::SeqCst),
        seen.len(),
        "no connection is kept for another request"
    );
    assert_eq!(
        seen[0].header("accept"),
        Some("application/json, text/event-stream")
    );
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert_eq!(seen[2].header("mcp-session-id"), Some("sess-1"));
    assert!(!log_text().contains(&GOOD[8..]), "the token is in the log");
}

/// The master advertises no capability: no sampling, no roots, no
/// elicitation, nothing else.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn initialize_advertises_no_capability() {
    let t = start().await;
    let c = run(&t, "ok", None).await;
    assert!(c.result.is_ok(), "{:?}", c.result);
    let init = t
        .fake
        .seen()
        .into_iter()
        .find(|s| s.rpc() == Some("initialize"))
        .unwrap();
    assert_eq!(init.body["params"]["capabilities"], serde_json::json!({}));
    assert_eq!(
        init.body["params"]["clientInfo"]["name"],
        harness_core::APP_NAME
    );
    assert_eq!(init.body["params"]["protocolVersion"], "2025-11-25");
    assert!(
        init.header("authorization").is_none(),
        "no sign-in: no token header"
    );
    assert_clean(&t, &c).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn event_stream_answers_are_read() {
    let t = start().await;
    let c = run(&t, "sse", None).await;
    let o = c.result.as_ref().expect("works over event streams");
    assert_eq!(o.tools.len(), 2);
    assert_clean(&t, &c).await;
}

/// Each hostile answer ends with a bounded read, a fixed error and the
/// connection closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_byte_and_every_second_is_bounded() {
    let t = start().await;
    for (path, want) in [
        ("bigjson", Failure::TooLarge),
        ("bigerror", Failure::Status(500)),
        ("endless", Failure::Timeout),
        ("drip", Failure::TooLarge),
        ("longline", Failure::TooLarge),
        ("hang", Failure::Timeout),
        ("wrongid", Failure::Protocol),
        ("textplain", Failure::ContentType),
        ("declared", Failure::TooLarge),
        ("longsession", Failure::BadAnswer("its session id")),
    ] {
        let started = Instant::now();
        let c = run(&t, path, None).await;
        assert_eq!(c.result.as_ref().unwrap_err(), &want, "{path}");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "{path} took {:?}",
            started.elapsed()
        );
        assert!(c.bytes_read <= CHECK_BYTES, "{path}: {}", c.bytes_read);
        if path == "declared" {
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "refused on its length"
            );
        }
        if path == "bigerror" {
            assert_eq!(c.bytes_read, 0, "an error body is never read");
        }
        assert_eq!(
            t.fake.settle().await,
            0,
            "{path}: a connection was left open"
        );
    }
}

/// Remote text in an error body or in an event reaches neither the error
/// nor the log, at TRACE for every crate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_text_stays_out_of_errors_and_logs() {
    let t = start().await;
    for (path, tok) in [
        ("e500", None),
        ("canary-event", None),
        ("auth", Some("mcp_wrong")),
        ("textplain", None),
        ("bigerror", None),
    ] {
        let c = run(&t, path, tok).await;
        let e = c.result.as_ref().unwrap_err().to_string();
        assert!(!e.contains(CANARY), "{path}: {e}");
        assert_clean(&t, &c).await;
    }
    let c = run(&t, "auth", Some("mcp_wrong")).await;
    assert_eq!(c.result.unwrap_err(), Failure::Refused(401));
    let l = log_text();
    assert!(!l.contains(CANARY), "remote text in the log");
    assert!(!l.contains("sess-1"), "the server's session id in the log");
    assert!(!l.contains("mcp_wrong"), "a token in the log");
}

/// A redirect is not followed: not to another origin, not to http, not
/// inward; the token went only to the connector's origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redirects_are_not_followed_and_the_token_stays_home() {
    let t = start().await;
    for path in ["redirect-other", "redirect-http", "redirect-inward"] {
        let before = t.fake.count();
        let c = run(&t, path, Some(GOOD)).await;
        assert_eq!(
            c.result.as_ref().unwrap_err(),
            &Failure::Redirected(307),
            "{path}"
        );
        assert_eq!(t.fake.count(), before + 1, "{path}: followed");
        assert_clean(&t, &c).await;
    }
    assert!(t.fake.seen().iter().all(|s| s.host == "mcp.test"));
    assert_eq!(
        t.net
            .lookup
            .asked()
            .iter()
            .filter(|n| *n == "other.test")
            .count(),
        0
    );
}

/// Without the test escape, a name that resolves to 127.0.0.1 is not
/// connected to; static rules refuse before any lookup.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_addresses_and_bad_urls_get_no_connection() {
    let mut t = start().await;
    t.net.allow_private = false;
    let c = run(&t, "ok", Some(GOOD)).await;
    assert_eq!(c.result.unwrap_err(), Failure::Connect);
    assert_eq!(t.fake.accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(t.net.lookup.asked().contains(&"mcp.test".to_string()));
    let asked = t.net.lookup.asked().len();
    for url in [
        "http://mcp.test/ok",
        "https://127.0.0.1/ok",
        "https://localhost/ok",
        "https://u:p@mcp.test/ok",
        "https://mcp.test:8443/ok",
    ] {
        let c = connectors::check("conn", url, token(GOOD), &t.net).await;
        assert_eq!(c.result.unwrap_err(), Failure::Connect, "{url}");
    }
    assert_eq!(
        t.net.lookup.asked().len(),
        asked,
        "no lookup for a static refusal"
    );
    assert_eq!(t.fake.accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paging_and_counts_are_capped() {
    let t = start().await;
    let c = run(&t, "paged", None).await;
    let o = c.result.as_ref().unwrap();
    assert_eq!(o.tools.len(), connectors::MAX_PAGES);
    assert!(o.more, "and more");
    let lists = t
        .fake
        .seen()
        .iter()
        .filter(|s| s.rpc() == Some("tools/list"))
        .count();
    assert_eq!(lists, connectors::MAX_PAGES);
    assert_clean(&t, &c).await;

    let c = run(&t, "many", None).await;
    let o = c.result.as_ref().unwrap();
    assert_eq!(o.tools.len(), MAX_TOOLS);
    assert!(o.more);
    assert_clean(&t, &c).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_names_and_text_are_cleaned_and_prefixed() {
    let t = start().await;
    let c = run(&t, "hostile", None).await;
    let o = c.result.as_ref().unwrap();
    assert_eq!(o.server_name.as_deref(), Some("linear"));
    let by: std::collections::HashMap<&str, &connectors::OfferedTool> =
        o.tools.iter().map(|x| (x.name.as_str(), x)).collect();
    assert_eq!(by["read_file"].agent_name, "mcp__conn__read_file");
    assert_eq!(
        by["read_file"].description.as_deref(),
        Some("<img src=x onerror=alert(1)>"),
        "kept as text; the UI renders text only"
    );
    let other = by["mcp__other__x"];
    assert_eq!(
        harness_core::agent::mcp_server_of(&other.agent_name),
        Some("conn"),
        "a tool named like another server's stays this connector's"
    );
    assert!(by.contains_key("evilname"), "{:?}", by.keys());
    assert_eq!(by["evilname"].description.as_deref(), Some("abcd"));
    assert_eq!(
        by["long"].description.as_ref().unwrap().chars().count(),
        512
    );
    assert_clean(&t, &c).await;
}

/// Environment proxies are not used: the check runs in a child process
/// with every proxy variable pointing at a listener that counts.
#[test]
fn environment_proxies_are_ignored() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (port, hits) = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((s, _)) = l.accept().await {
                n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                held.push(s);
            }
        });
        (port, n)
    });
    let proxy = format!("http://127.0.0.1:{port}");
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "proxy_child", "--ignored", "--nocapture"]);
    for k in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env(k, &proxy);
    }
    let out = cmd
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env("CONNECTOR_PROXY_CHILD", "1")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the proxy was used"
    );
    drop(rt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "run by environment_proxies_are_ignored"]
async fn proxy_child() {
    if std::env::var("CONNECTOR_PROXY_CHILD").is_err() {
        return;
    }
    let t = start().await;
    let c = run(&t, "ok", None).await;
    assert!(c.result.is_ok(), "{:?}", c.result);
}

/// Our client sends nothing to any URL but the connector's, whatever the
/// transport asks: not a message, not a session's DELETE.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_client_refuses_any_other_url() {
    use harness_master::connectors::client::{Budget, Http};
    use rmcp::transport::streamable_http_client::StreamableHttpClient as _;
    let t = start().await;
    let http = Http {
        client: harness_master::remote::client(
            Duration::from_secs(2),
            &t.net.lookup,
            false,
            &t.net.roots,
        )
        .unwrap(),
        url: "https://mcp.test/ok".into(),
        token: token(GOOD),
        budget: Budget::new(CHECK_BYTES),
        failure: Arc::default(),
    };
    let msg: rmcp::model::ClientJsonRpcMessage = serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}
    }))
    .unwrap();
    let r = http
        .post_message(
            "https://other.test/ok".into(),
            msg,
            None,
            None,
            Default::default(),
        )
        .await;
    assert!(r.is_err());
    assert_eq!(http.first_failure(), Some(Failure::OtherUrl));
    http.delete_session(
        "https://other.test/ok".into(),
        "sess-1".into(),
        None,
        Default::default(),
    )
    .await
    .unwrap();
    assert!(t.net.lookup.asked().is_empty(), "nothing was looked up");
    assert_eq!(t.fake.count(), 0);
}
