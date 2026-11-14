//! The review of pull request 102's probe, turned round: what it showed
//! refused or ignored now works, against the built-in tools' own schemas.

use harness_tools::args::{Closed, check};
use harness_tools::builtin;
use serde_json::{Value, json};

fn schema(n: &str) -> Value {
    builtin()
        .into_iter()
        .find(|t| t.name() == n)
        .unwrap()
        .schema_for(std::time::Duration::from_secs(600))
}

fn ck(n: &str, input: Value) -> Result<Value, String> {
    check(&schema(n), &input, Closed::Builtin)
}

#[test]
fn nulls_and_json_strings_are_taken() {
    assert_eq!(
        ck("read", json!({"path": "a", "offset": null, "limit": null})).unwrap(),
        json!({"path": "a"})
    );
    assert_eq!(
        ck("bash", json!({"command": "ls", "workdir": null})).unwrap(),
        json!({"command": "ls"})
    );
    let edits = ck(
        "edit",
        json!({"path": "a", "edits": "[{\"old_string\":\"x\",\"new_string\":\"y\"}]"}),
    )
    .unwrap();
    assert!(edits["edits"].is_array(), "{edits}");
    let todo = ck(
        "todo",
        json!({"items": "[{\"content\":\"a\",\"status\":\"pending\"}]"}),
    )
    .unwrap();
    assert!(todo["items"].is_array(), "{todo}");
    // An MCP server's schema is open unless it says otherwise.
    let s = json!({"type": "object", "properties": {"q": {"type": "string"}}});
    assert!(check(&s, &json!({"q": "x", "extra": 1}), Closed::Schema).is_ok());
}

#[tokio::test]
async fn an_integral_float_is_read_as_the_integer() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("f.txt"),
        (1..=20).map(|i| format!("line{i}\n")).collect::<String>(),
    )
    .unwrap();
    let ctx = harness_tools::ToolCtx::new(d.path());
    let read = builtin().into_iter().find(|t| t.name() == "read").unwrap();
    let v = ck(
        "read",
        json!({"path": "f.txt", "offset": 15.0, "limit": 2.0}),
    )
    .unwrap();
    assert_eq!(v["offset"], json!(15));
    let out = read.run(&ctx, v).await.output;
    assert!(out.contains("line15") && !out.contains("line1\n"), "{out}");
    assert!(!out.contains("line17"), "the limit held: {out}");
}
