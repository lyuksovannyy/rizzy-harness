//! Node setup over HTTP against an in-process SSH server: probe, manual
//! mode, harness's own key, and no stored credentials.

use std::time::Duration;

use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_provision::testing as fake;
use harness_store::Store;
use serde_json::{Value, json};

#[tokio::test]
async fn manual_setup_installs_harness_key_and_forgets_the_password() {
    let ssh = fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    let (rt, _) =
        ProviderRuntime::start(Config::default(), dir.path(), Default::default()).unwrap();
    master.set_provider_runtime(std::sync::Arc::new(rt));
    master.set_node_listener(7443, "sha256:abc".into());
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let c = reqwest::Client::new();
    let base = format!("http://{addr}");
    let post = |p: &str, b: Value| {
        c.post(format!("{base}{p}"))
            .bearer_auth("tok")
            .json(&b)
            .send()
    };

    let v: Value = post(
        "/api/admin/provision/probe",
        json!({"host": "127.0.0.1", "port": ssh.port}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(v["fingerprint"], ssh.fingerprint.as_str());

    // Automatic setup needs root.
    let r = post("/api/admin/provision", json!({"host": "127.0.0.1", "port": ssh.port, "user": "deploy", "fingerprint": ssh.fingerprint, "password": fake::PASSWORD, "mode": "auto"})).await.unwrap();
    assert_eq!(r.status(), 400);
    assert!(r.text().await.unwrap().contains("needs the root login"));
    // Unconfirmed fingerprint or no credentials: refused.
    let r = post("/api/admin/provision", json!({"host": "127.0.0.1", "port": ssh.port, "user": "deploy", "fingerprint": "", "password": fake::PASSWORD, "mode": "manual"})).await.unwrap();
    assert_eq!(r.status(), 400);
    let r = post("/api/admin/provision", json!({"host": "127.0.0.1; rm", "port": ssh.port, "user": "deploy", "fingerprint": ssh.fingerprint, "password": "x", "mode": "manual"})).await.unwrap();
    assert_eq!(r.status(), 400);

    let v: Value = post(
        "/api/admin/provision",
        json!({"host": "127.0.0.1", "port": ssh.port, "user": "deploy", "fingerprint": ssh.fingerprint, "password": fake::PASSWORD, "mode": "manual"}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let id = v["id"].as_str().unwrap().to_string();
    let get_state = || async {
        c.get(format!("{base}/api/admin/provision/{id}"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()
    };
    // This test's SSH server runs commands as the test process; under root
    // harness asks whether to create its own user. Answering "create" would
    // add a real system user here, so the test keeps root (the "create"
    // path is covered by harness-provision's scripted tests).
    let mut asked = Value::Null;
    for _ in 0..200 {
        asked = get_state().await;
        if asked["finished"] == true
            || asked["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["status"] == "waiting")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let waits_for_user = asked["steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "own_user" && s["status"] == "waiting");
    if waits_for_user {
        let r = post(
            &format!("/api/admin/provision/{id}/answer"),
            json!({"step": "own_user", "choice": "delete-everything"}),
        )
        .await
        .unwrap();
        assert_eq!(r.status(), 409, "only offered choices are accepted");
        let r = post(
            &format!("/api/admin/provision/{id}/answer"),
            json!({"step": "own_user", "choice": "keep"}),
        )
        .await
        .unwrap();
        assert_eq!(r.status(), 200);
    }
    let mut state = Value::Null;
    for _ in 0..200 {
        state = c
            .get(format!("{base}/api/admin/provision/{id}"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if state["finished"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(state["finished"], true, "{state}");
    assert!(state["error"].is_null(), "{state}");
    let steps = state["steps"].as_array().unwrap();
    assert_eq!(steps[0]["id"], "login");
    assert_eq!(steps[0]["status"], "ok");
    let harness = steps.iter().find(|s| s["id"] == "harness").unwrap();
    assert!(
        harness["script"].as_str().unwrap().contains("--token "),
        "{harness}"
    );

    // Harness's key is on the server; the password is nowhere on disk.
    let ak = std::fs::read_to_string(ssh.home.path().join(".ssh/authorized_keys")).unwrap();
    assert!(ak.contains("ssh-ed25519 ") && ak.contains("harness"));
    let mut found = false;
    for e in walk(dir.path()) {
        if std::fs::read(&e)
            .unwrap()
            .windows(fake::PASSWORD.len())
            .any(|w| w == fake::PASSWORD.as_bytes())
        {
            found = true;
        }
    }
    assert!(!found, "the password was written to disk");

    // Re-check works with harness's key alone (passwords now refused).
    ssh.set_password_login(false);
    let r = post(&format!("/api/admin/provision/{id}/recheck"), json!({}))
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());

    // The setup is saved and can be managed afterwards.
    let get = |p: &str| c.get(format!("{base}{p}")).bearer_auth("tok").send();
    let v: Value = get("/api/admin/node-setups")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rec = v["setups"][0]["setup"].clone();
    assert_eq!(rec["mode"], "manual", "{v}");
    assert_eq!(rec["watch_ssh"], true);
    assert!(
        v["setups"][0]["commands"]["ssh_off"]
            .as_str()
            .unwrap()
            .contains("10-rizzy-harness.conf")
    );
    let sid = rec["id"].as_str().unwrap().to_string();
    // Manual mode: harness does not touch the firewall itself.
    let r = post(
        &format!("/api/admin/node-setups/{sid}/firewall"),
        json!({"enable": false}),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 409);
    // Switch to automatic; still no root access kept, so a root login is asked for.
    let r = c
        .patch(format!("{base}/api/admin/node-setups/{sid}"))
        .bearer_auth("tok")
        .json(&json!({"mode": "auto"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = post(
        &format!("/api/admin/node-setups/{sid}/firewall"),
        json!({"enable": false}),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 502);
    assert!(r.text().await.unwrap().contains("give a root login"));
    // The watchdog: this test server offers password logins, so SSH is reported,
    // and the problem shows up in "waiting for you".
    let v: Value = post(&format!("/api/admin/node-setups/{sid}/check"), json!({}))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["problems"][0]["kind"], "ssh", "{v}");
    let a: Value = get("/api/attention").await.unwrap().json().await.unwrap();
    assert_eq!(a["nodes"][0]["setup_id"], sid.as_str(), "{a}");
    // Turning the SSH watch off clears it.
    c.patch(format!("{base}/api/admin/node-setups/{sid}"))
        .bearer_auth("tok")
        .json(&json!({"watch_ssh": false}))
        .send()
        .await
        .unwrap();
    let a: Value = get("/api/attention").await.unwrap().json().await.unwrap();
    assert!(a["nodes"].as_array().unwrap().is_empty(), "{a}");
}

fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(walk(&p));
        } else {
            v.push(p);
        }
    }
    v
}
