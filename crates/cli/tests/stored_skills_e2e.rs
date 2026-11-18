//! Stored skills end to end (ADOPTION_PLAN X.34 step B, PLAN.md T81): the
//! real binary with its demo (scripted) provider. A workspace's admin
//! stores a skill; a chat of the built-in Assistant (no file access, on
//! the master's own chat runtime) loads it with the `skill` tool through
//! the link; its `file` argument is refused; nothing on the disk is read.

use serde_json::{Value, json};

#[path = "support/serve.rs"]
mod real;

#[tokio::test(flavor = "multi_thread")]
async fn the_assistant_loads_a_skill_stored_in_its_workspace() {
    let (_dir, data, _ws) = real::setup();
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let v = s.get("/api/workspaces").await;
    let ws = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let made = s
        .post(
            &format!("/api/workspaces/{ws}/skills"),
            json!({"scope": "workspace", "skill_md":
                "---\nname: release-notes\ndescription: Write release notes from the log\n\
                 allowed-tools: Bash\n---\nGroup the changes by area.\n"}),
        )
        .await;
    assert_eq!(made["ignored"], json!(["allowed-tools"]), "{made}");
    let sid = s
        .post(
            "/api/sessions",
            json!({"profile": "chat", "workspace_id": ws}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(
        &sid,
        &[
            json!({"tool": "skill", "input": {"name": "release-notes"}}),
            json!({"tool": "skill", "input": {"name": "release-notes", "file": "../config.toml"}}),
        ],
    )
    .await;
    let evs = s
        .wait_for(&sid, |e| e.iter().any(|e| e["kind"] == "turn_ended"))
        .await;
    let results: Vec<&Value> = evs
        .iter()
        .filter(|e| e["kind"] == "tool_call_finished")
        .map(|e| &e["payload"])
        .collect();
    assert_eq!(results.len(), 2, "{evs:#?}\n{}", s.log());
    assert_eq!(results[0]["ok"], true, "{:#?}\n{}", results[0], s.log());
    let out = results[0]["output"].as_str().unwrap();
    assert!(out.ends_with("Group the changes by area."), "{out}");
    assert!(out.contains("written by this workspace's admins"), "{out}");
    assert_eq!(results[1]["ok"], false, "{:#?}", results[1]);
    assert!(
        results[1]["output"].as_str().unwrap().contains("text only"),
        "{:#?}",
        results[1]
    );
    // The list went with the turn as a standing section.
    let ctx = evs
        .iter()
        .find(|e| e["kind"] == "context_sent")
        .map(|e| e["payload"].to_string())
        .unwrap_or_default();
    assert!(
        ctx.contains("- release-notes: Write release notes from the log"),
        "{ctx}"
    );
}
