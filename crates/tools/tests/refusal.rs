//! `Tool::refusal` (PLAN.md T51): the loop asks it before the approval
//! card, so it must be the tool's own refusal, word for word what `run`
//! answers, and nothing where `run` would go ahead.

use harness_tools::{Tool, ToolCtx, builtin};
use serde_json::{Value, json};

fn tool(name: &str) -> std::sync::Arc<dyn Tool> {
    builtin().into_iter().find(|t| t.name() == name).unwrap()
}

#[tokio::test]
async fn the_refusal_is_what_run_answers_and_only_when_it_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(dir.path().join("out.txt"), "x\n").unwrap();
    std::fs::write(ws.join("in.txt"), "x\n").unwrap();
    let ctx = ToolCtx::new(&ws);
    let refused: [(&str, Value); 7] = [
        ("write", json!({"path": "../out.txt", "content": "e"})),
        ("write", json!({"path": ".git/config", "content": "e"})),
        (
            "edit",
            json!({"path": "../out.txt", "old_string": "x", "new_string": "y"}),
        ),
        ("read", json!({"path": "../out.txt"})),
        ("grep", json!({"pattern": "x", "path": ".."})),
        ("glob", json!({"pattern": "*", "path": "/"})),
        ("bash", json!({"command": "true", "workdir": ".."})),
    ];
    for (name, input) in refused {
        let t = tool(name);
        let early = t.refusal(&ctx, &input);
        let run = t.run(&ctx, input.clone()).await;
        assert!(!run.ok, "{name} {input}");
        assert_eq!(
            early.as_deref(),
            Some(run.output.as_str()),
            "{name} {input}"
        );
    }
    // Calls `run` carries out: no opinion.
    for (name, input) in [
        ("write", json!({"path": "new.txt", "content": "e"})),
        ("read", json!({"path": "in.txt"})),
        ("grep", json!({"pattern": "x"})),
        ("bash", json!({"command": "true"})),
    ] {
        assert_eq!(tool(name).refusal(&ctx, &input), None, "{name} {input}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.txt")).unwrap(),
        "x\n"
    );
}
