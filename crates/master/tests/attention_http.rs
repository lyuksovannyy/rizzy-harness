//! "Needs you" list: pending approvals and unanswered questions.

mod common;
use common::SameOrigin as _;

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::Method;
use serde_json::{Value, json};

async fn rt(root: &std::path::Path, id: &str, profile: Profile) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: id.into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            if profile == Profile::Agentic {
                harness_tools::builtin()
            } else {
                vec![]
            },
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn approvals_and_questions_need_attention_until_handled() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim = dir.path().join("claim");
    std::fs::write(&claim, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim,
        }),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    master
        .add_link(
            rt(dir.path(), "master", Profile::Chat).await.attach().await,
            true,
        )
        .await
        .unwrap();
    master
        .add_link(
            rt(dir.path(), "local", Profile::Agentic)
                .await
                .attach()
                .await,
            true,
        )
        .await
        .unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let c = reqwest::Client::new();
    let r = c
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let ck = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let req = |m: Method, p: &str, b: Option<Value>| {
        let mut rb = c
            .request(m, format!("{base}{p}"))
            .header("cookie", &ck)
            .same_origin();
        if let Some(b) = b {
            rb = rb.json(&b);
        }
        async move {
            rb.send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap_or(Value::Null)
        }
    };
    let items = || async {
        req(Method::GET, "/api/attention", None).await["items"]
            .as_array()
            .unwrap()
            .clone()
    };
    let wait = |want: usize| async move {
        for _ in 0..300 {
            let v = req(Method::GET, "/api/attention", None).await["items"]
                .as_array()
                .unwrap()
                .clone();
            if v.len() == want {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("attention never reached {want}");
    };
    assert!(items().await.is_empty());

    // A chat whose answer ends with a question.
    let q = req(
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
    )
    .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    req(
        Method::POST,
        &format!("/api/sessions/{q}/prompt"),
        Some(json!({"text": "shall we?"})),
    )
    .await;
    let v = wait(1).await;
    assert_eq!(v[0]["reason"], "question");
    // A plain answer does not need attention.
    let plain = req(
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
    )
    .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    req(
        Method::POST,
        &format!("/api/sessions/{plain}/prompt"),
        Some(json!({"text": "hello"})),
    )
    .await;

    // An agentic turn waiting for approval.
    let ws = dir.path().display().to_string();
    let a = req(
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "agentic", "cwd": ws})),
    )
    .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    req(
        Method::POST,
        &format!("/api/sessions/{a}/prompt"),
        Some(json!({"text": "run: echo hi"})),
    )
    .await;
    let v = wait(2).await;
    let appr = v.iter().find(|i| i["session_id"] == a.as_str()).unwrap();
    assert_eq!(appr["reason"], "approval");

    // Seeing the question clears it; approving clears the other.
    let seq = v.iter().find(|i| i["session_id"] == q.as_str()).unwrap()["seq"].clone();
    req(
        Method::POST,
        &format!("/api/sessions/{q}/seen"),
        Some(json!({"seq": seq})),
    )
    .await;
    wait(1).await;
    let ev = req(Method::GET, &format!("/api/sessions/{a}/events"), None).await;
    let aid = ev["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "approval_requested")
        .unwrap()["payload"]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    req(
        Method::POST,
        &format!("/api/sessions/{a}/approvals/{aid}"),
        Some(json!({"decision": "allow"})),
    )
    .await;
    wait(0).await;
    // A new question in the same chat needs attention again.
    req(
        Method::POST,
        &format!("/api/sessions/{q}/prompt"),
        Some(json!({"text": "again?"})),
    )
    .await;
    wait(1).await;
}
