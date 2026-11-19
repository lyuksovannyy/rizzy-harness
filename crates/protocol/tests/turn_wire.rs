//! Wire snapshots of the three turn messages (`session.prompt`,
//! `session.callback`, `session.retry`). Each literal is the exact JSON the
//! messages produced before they moved into named structs: the `_FULL` ones
//! have every field set to a non-empty value, the `_EMPTY` ones leave every
//! optional field out. Refactors of the Rust types must not change a byte.

use harness_protocol::MasterToNode;

const PROMPT_FULL: &str = r#"{"type":"session.prompt","req_id":"r1","session_id":"0193f2a0-0000-7000-8000-000000000001","text":"hi","agent":{"agent_id":"a","name":"N","system_prompt":"S","tools":"all","ui":"rich","prompt_hash":"h"},"agent_named":true,"attachments":[{"name":"f.txt","mime":"text/plain","text":"x","truncated":true}],"compare":["m1","m2"],"savings":{"prompt_cache":true,"keep_tool_outputs":3,"filter_shell_output":true,"bounded_reads":true,"web_readability":true,"style":"lite","repo_map_tokens":100,"outline_tool":true,"graphify":true},"context":{"turn":"t","standing":[{"part":"memory","text":"m"}],"unattended":true},"reply":{"message_id":"m1","block_id":"b","values":{"k":"v"}},"images":[{"media_id":"m","sha256":"abababababababababababababababababababababababababababababababab","mime":"image/png","name":"a.png","width":2,"height":1,"data":"aGk="}],"files":["src/a.rs"],"owner":"u1","identity":{"login":"ann","name":"Ann A","email":"ann@example.org"},"peers":[{"agent_id":"p","name":"N","system_prompt":"S","tools":"all","ui":"rich","prompt_hash":"h"}],"tags":["t"]}"#;

const PROMPT_EMPTY: &str = r#"{"type":"session.prompt","req_id":"r1","session_id":"0193f2a0-0000-7000-8000-000000000001","text":"hi"}"#;

const CALLBACK_FULL: &str = r#"{"type":"session.callback","req_id":"r2","session_id":"0193f2a0-0000-7000-8000-000000000001","bg_ids":["bg-1"],"agent":{"agent_id":"a","name":"N","system_prompt":"S","tools":"all","ui":"rich","prompt_hash":"h"},"savings":{"prompt_cache":true,"keep_tool_outputs":3,"filter_shell_output":true,"bounded_reads":true,"web_readability":true,"style":"lite","repo_map_tokens":100,"outline_tool":true,"graphify":true},"context":{"turn":"t","standing":[{"part":"memory","text":"m"}],"unattended":true},"owner":"u1","identity":{"login":"ann","name":"Ann A","email":"ann@example.org"}}"#;

const CALLBACK_EMPTY: &str = r#"{"type":"session.callback","req_id":"r2","session_id":"0193f2a0-0000-7000-8000-000000000001","bg_ids":["bg-1"]}"#;

const RETRY_FULL: &str = r#"{"type":"session.retry","req_id":"r3","session_id":"0193f2a0-0000-7000-8000-000000000001","turn_id":"t1","agent":{"agent_id":"a","name":"N","system_prompt":"S","tools":"all","ui":"rich","prompt_hash":"h"},"savings":{"prompt_cache":true,"keep_tool_outputs":3,"filter_shell_output":true,"bounded_reads":true,"web_readability":true,"style":"lite","repo_map_tokens":100,"outline_tool":true,"graphify":true},"context":{"turn":"t","standing":[{"part":"memory","text":"m"}],"unattended":true},"agent_named":true,"owner":"u1","identity":{"login":"ann","name":"Ann A","email":"ann@example.org"}}"#;

const RETRY_EMPTY: &str = r#"{"type":"session.retry","req_id":"r3","session_id":"0193f2a0-0000-7000-8000-000000000001","turn_id":"t1"}"#;

/// Every field of the message, as the wire names it.
const PROMPT_FIELDS: &[&str] = &[
    "type",
    "req_id",
    "session_id",
    "text",
    "agent",
    "agent_named",
    "attachments",
    "compare",
    "savings",
    "context",
    "reply",
    "images",
    "files",
    "owner",
    "identity",
    "peers",
    "tags",
];
const CALLBACK_FIELDS: &[&str] = &[
    "type",
    "req_id",
    "session_id",
    "bg_ids",
    "agent",
    "savings",
    "context",
    "owner",
    "identity",
];
const RETRY_FIELDS: &[&str] = &[
    "type",
    "req_id",
    "session_id",
    "turn_id",
    "agent",
    "savings",
    "context",
    "agent_named",
    "owner",
    "identity",
];

fn snapshot(literal: &str, tag: &str, required: &[&str], all_fields: bool) {
    let v: serde_json::Value = serde_json::from_str(literal).unwrap();
    assert_eq!(v["type"], tag);
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    if all_fields {
        for f in required {
            assert!(keys.contains(f), "{tag}: the full literal lacks `{f}`");
        }
        assert_eq!(keys.len(), required.len(), "{tag}: {keys:?}");
    } else {
        // Only the fields the receiver cannot do without.
        assert!(keys.len() <= 5, "{tag}: {keys:?}");
    }
    let msg: MasterToNode = serde_json::from_str(literal).unwrap();
    assert_eq!(
        serde_json::to_string(&msg).unwrap(),
        literal,
        "{tag}: bytes differ"
    );
    let again: MasterToNode = serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
    assert_eq!(again, msg);
}

#[test]
fn prompt_wire_is_stable() {
    snapshot(PROMPT_FULL, "session.prompt", PROMPT_FIELDS, true);
    snapshot(PROMPT_EMPTY, "session.prompt", PROMPT_FIELDS, false);
}

#[test]
fn callback_wire_is_stable() {
    snapshot(CALLBACK_FULL, "session.callback", CALLBACK_FIELDS, true);
    snapshot(CALLBACK_EMPTY, "session.callback", CALLBACK_FIELDS, false);
}

#[test]
fn retry_wire_is_stable() {
    snapshot(RETRY_FULL, "session.retry", RETRY_FIELDS, true);
    snapshot(RETRY_EMPTY, "session.retry", RETRY_FIELDS, false);
}
