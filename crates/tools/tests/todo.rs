//! The `todo` tool: the agent's own task list, replaced whole by each call.
//! It checks the list and hands it to the loop (which records it); it
//! changes nothing outside the chat.

use harness_core::policy::ToolRisk;
use harness_core::todo::{TodoItem, TodoStatus};
use harness_tools::{Tool, ToolCtx, todo::Todo};
use serde_json::{Value, json};

async fn run(input: Value) -> harness_tools::ToolOutcome {
    let dir = tempfile::tempdir().unwrap();
    Todo.run(&ToolCtx::new(dir.path()), input).await
}

fn item(content: &str, status: &str) -> Value {
    json!({"content": content, "status": status})
}

#[test]
fn it_is_read_only_and_says_when_to_use_it() {
    assert_eq!(Todo.name(), "todo");
    // It writes only the chat's own log: never asked about (TD-6).
    assert_eq!(Todo.risk(), ToolRisk::ChatOnly);
    let d = Todo.description();
    assert!(
        d.contains("multi-step") || d.contains("several steps"),
        "{d}"
    );
    assert!(d.contains("replaces"), "{d}");
    // So the pinned list goes away when the work is (TD-2).
    assert!(d.contains("clear it with [] when the work is done"), "{d}");
    let s = Todo.schema();
    assert_eq!(s["properties"]["items"]["maxItems"], 50);
}

#[tokio::test]
async fn a_valid_list_is_handed_to_the_loop() {
    let out = run(json!({"items": [
        item("Read the code", "done"),
        item("  Write the tests  ", "in_progress"),
        item("Implement", "pending"),
    ]}))
    .await;
    assert!(out.ok, "{}", out.output);
    assert_eq!(
        out.todo,
        Some(vec![
            TodoItem {
                content: "Read the code".into(),
                status: TodoStatus::Done
            },
            TodoItem {
                content: "Write the tests".into(),
                status: TodoStatus::InProgress
            },
            TodoItem {
                content: "Implement".into(),
                status: TodoStatus::Pending
            },
        ])
    );
    assert!(out.output.contains("3 items"), "{}", out.output);
}

#[tokio::test]
async fn an_empty_list_clears_it() {
    let out = run(json!({"items": []})).await;
    assert!(out.ok, "{}", out.output);
    assert_eq!(out.todo, Some(vec![]));
}

/// Each refusal says why, and hands nothing to the loop.
#[tokio::test]
async fn invalid_lists_are_refused_with_the_reason() {
    let long = "x".repeat(201);
    let many: Vec<Value> = (0..51)
        .map(|i| item(&format!("step {i}"), "pending"))
        .collect();
    let cases = [
        (
            json!({"items": [item("a", "in_progress"), item("b", "in_progress")]}),
            "in_progress",
        ),
        (json!({"items": many}), "at most 50"),
        (json!({"items": [item(&long, "pending")]}), "at most 200"),
        (json!({"items": [item("one\ntwo", "pending")]}), "one line"),
        (json!({"items": [item("   ", "pending")]}), "empty"),
        (json!({"items": [item("a", "completed")]}), "\"done\""),
        (json!({"items": [{"status": "pending"}]}), "content"),
        (json!({"list": []}), "items"),
    ];
    for (input, why) in cases {
        let out = run(input.clone()).await;
        assert!(!out.ok, "accepted {input}");
        assert!(out.todo.is_none(), "{input}");
        assert!(out.output.contains(why), "{input}: {}", out.output);
    }
    // 200 characters (not bytes) is still one item.
    let out = run(json!({"items": [item(&"é".repeat(200), "pending")]})).await;
    assert!(out.ok, "{}", out.output);
}
