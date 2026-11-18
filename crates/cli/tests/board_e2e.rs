//! The board tools end to end (ADOPTION_PLAN X.32, PLAN.md T78): the real
//! binary with its demo (scripted) provider, a project chat of the
//! built-in Assistant: `board_edit` adds a card through the master,
//! `board` lists it, and the card on the project's board says which agent
//! made it.

use serde_json::{Value, json};

#[path = "support/serve.rs"]
mod real;

fn count(evs: &[Value], kind: &str) -> usize {
    evs.iter().filter(|e| e["kind"] == kind).count()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_adds_and_lists_a_card_of_its_projects_board() {
    let (_dir, data, _ws) = real::setup();
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let p = s.post("/api/projects", json!({"name": "Launch"})).await;
    let pid = p["project_id"].as_str().unwrap().to_string();
    let sid = s
        .post(
            "/api/sessions",
            json!({"profile": "chat", "project_id": pid}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(
        &sid,
        &[
            json!({"tool": "board_edit", "input": {"action": "add", "title": "Write the release notes"}}),
            json!({"tool": "board", "input": {}}),
        ],
    )
    .await;
    let evs = s.wait_for(&sid, |e| count(e, "turn_ended") == 1).await;
    let results: Vec<&Value> = evs
        .iter()
        .filter(|e| e["kind"] == "tool_call_finished")
        .map(|e| &e["payload"])
        .collect();
    assert_eq!(results.len(), 2, "{evs:#?}\n{}", s.log());
    assert_eq!(results[0]["ok"], true, "{:#?}", results[0]);
    assert!(
        results[0]["output"]
            .as_str()
            .unwrap()
            .starts_with("Added card ")
    );
    let listed = results[1]["output"].as_str().unwrap();
    assert!(
        listed.contains(
            "\"Write the release notes\", made by agent \"Assistant\", linked to this chat"
        ),
        "{listed}"
    );
    let board = s.get(&format!("/api/projects/{pid}/board")).await;
    let card = &board["columns"][0]["cards"][0];
    assert_eq!(card["title"], "Write the release notes");
    assert_eq!(card["agent"], "Assistant");
    assert_eq!(card["chat"]["session_id"], sid.as_str());
}
