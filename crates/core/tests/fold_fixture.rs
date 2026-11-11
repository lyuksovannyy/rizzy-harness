//! Shared reducer fixture: `web/src/fold.test.ts` folds the same events with
//! the TypeScript port and must produce exactly `expected`.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test fold_fixture`.

use harness_core::agent::UiLevel;
use harness_core::artifacts::{ArtifactMeta, GitInfo, StagedBy};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::json;

/// A streamed message: deltas first, then the final message with its block.
const STREAMED: [&str; 2] = [
    "Let me ",
    "look.\n\n```ui:progress\n{\"id\": \"look\", \"title\": \"Looking\", \"steps\": [{\"label\": \"Test\", \"status\": \"active\"}]}\n```\n",
];

const BLOCKS_TEXT: &str = "Found it.\n\n```ui:progress\n{\"id\": \"fix\", \"title\": \"Fixing\", \"steps\": [{\"label\": \"Find\", \"status\": \"done\"}]}\n```\n\n```ui:choices\n{\"id\": \"next\"}\n```\n";

fn staged(call_id: Option<&str>, by: StagedBy) -> ArtifactStaged {
    ArtifactStaged {
        stage_id: "01a19bf2000070008000000000000a01".into(),
        call_id: call_id.map(str::to_string),
        by,
        name: "app".into(),
        filename: "app".into(),
        packed: None,
        size: 12,
        sha256: "ab".repeat(32),
        kind_hint: None,
        source_path: "/w/target/release/app".into(),
        notes: (by == StagedBy::Agent).then(|| "release build".into()),
        meta: ArtifactMeta {
            target: Some("x86_64-linux".into()),
            git: Some(GitInfo {
                commit: "abc123".into(),
                branch: "main".into(),
                dirty: false,
            }),
            ..Default::default()
        },
    }
}

fn events() -> Vec<RawEvent> {
    let sid: SessionId = "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap();
    let t = || "t1".to_string();
    let kinds = vec![
        EventKind::SessionCreated(SessionCreated {
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            // A sub-agent's session: the view's meta keeps its parent.
            parent_session: Some("01a19bf2-0000-7000-8000-00000000f1f1".parse().unwrap()),
            title: "fixture".into(),
            cwd: "/w".into(),
            model: "mock".into(),
            backend: "native".into(),
            node_id: "local".into(),
            approval_mode: ApprovalMode::Ask,
        }),
        EventKind::TurnStarted(TurnStarted {
            agent: None,
            turn_id: t(),
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: t(),
            text: "fix the bug".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
        }),
        EventKind::AssistantDelta(AssistantDelta {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m1".into(),
            text: STREAMED[0].into(),
        }),
        EventKind::AssistantDelta(AssistantDelta {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m1".into(),
            text: STREAMED[1].into(),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m1".into(),
            text: STREAMED.concat(),
            tool_calls: vec![ToolCallRef {
                call_id: "c1".into(),
                tool: "bash".into(),
                input: json!({"command": "cargo test"}),
            }],
            blocks: harness_core::ui::extract(&STREAMED.concat(), UiLevel::Rich),
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: t(),
            call_id: "c1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo test"}),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: "a1".into(),
            call_id: "c1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo test"}),
            reason: "runs programs".into(),
            turn_allow: false,
        }),
        EventKind::ApprovalResolved(ApprovalResolved {
            approval_id: "a1".into(),
            decision: Decision::AllowSession,
            by: ResolvedBy::User,
        }),
        EventKind::ToolOutputDelta(ToolOutputDelta {
            call_id: "c1".into(),
            stream: OutputStream::Stdout,
            text: "running 3 tests\n".into(),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c1".into(),
            ok: false,
            output: "1 failed".into(),
            exit_code: Some(101),
            truncated: false,
            sources: vec![],
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: t(),
            call_id: "c2".into(),
            tool: "read".into(),
            input: json!({"path": "src/lib.rs"}),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c2".into(),
            ok: true,
            output: "     1\tfn main() {}".into(),
            exit_code: None,
            truncated: false,
            sources: vec![],
        }),
        // A web tool's result carries the sources behind its labels.
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: t(),
            call_id: "c5".into(),
            tool: "web_search".into(),
            input: json!({"query": "tokio"}),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c5".into(),
            ok: true,
            output: "[S1] Tokio\n    https://tokio.rs/\n    An async runtime".into(),
            exit_code: None,
            truncated: false,
            sources: vec![Source {
                id: "S1".into(),
                url: "https://tokio.rs/".into(),
                title: "Tokio".into(),
            }],
        }),
        // A message with no preceding deltas, with one valid block and one
        // that is an error.
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m2".into(),
            text: BLOCKS_TEXT.into(),
            tool_calls: vec![],
            blocks: harness_core::ui::extract(BLOCKS_TEXT, UiLevel::Rich),
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: t(),
            call_id: "c3".into(),
            tool: "edit".into(),
            input: json!({"path": "a", "old_string": "x", "new_string": "y"}),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: "a2".into(),
            call_id: "c3".into(),
            tool: "edit".into(),
            input: json!({}),
            reason: "changes files".into(),
            turn_allow: false,
        }),
        EventKind::Checkpoint(Checkpoint {
            turn_id: t(),
            phase: CheckpointPhase::Before,
            git_ref: "refs/x".into(),
            files: vec![],
            excluded: vec![],
            excluded_more: 0,
        }),
        EventKind::Error(ErrorEvent {
            turn_id: Some(t()),
            message: "node restarted".into(),
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: t(),
            reason: TurnEndReason::Interrupted,
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 2,
                context_tokens: 1,
                ..Default::default()
            }),
        }),
        // v2: agents, authors, variants.
        EventKind::AgentChanged(AgentChanged {
            agent: AgentRef {
                agent_id: "coder".into(),
                name: "Coder".into(),
                prompt_hash: "h1".into(),
            },
        }),
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t2".into(),
            agent: Some(AgentRef {
                agent_id: "writer".into(),
                name: "Writer".into(),
                prompt_hash: "h2".into(),
            }),
        }),
        // The turn's model failed before any output; a fallback answers.
        EventKind::ModelFallback(ModelFallback {
            turn_id: "t2".into(),
            model: "loc::small".into(),
            from: "loc::big".into(),
            reason: Some("unavailable".into()),
        }),
        EventKind::ModelFallback(ModelFallback {
            turn_id: "t2".into(),
            model: "loc::other".into(),
            from: "loc::big".into(),
            reason: None,
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: "t2".into(),
            text: "@writer draft the changelog".into(),
            attachments: vec![Attachment {
                name: "notes.md".into(),
                mime: "text/markdown".into(),
                text: "- fixed a bug".into(),
                truncated: false,
            }],
            reply: None,
            // Images go into the view as references; no bytes in events.
            images: vec![ImageRef {
                media_id: "01a19bf2000070008000000000000b01".into(),
                sha256: "cd".repeat(32),
                mime: "image/png".into(),
                name: "screenshot.png".into(),
                width: 1280,
                height: 720,
            }],
        }),
        EventKind::HistorySummarized(HistorySummarized {
            turn_id: "t2".into(),
            through_seq: 3,
            text: "User asked to fix a bug.".into(),
        }),
        EventKind::AssistantDelta(AssistantDelta {
            turn_id: "t2".into(),
            message_id: "m3".into(),
            text: "Draft A".into(),
            author: Some("writer".into()),
            variant: Some(Variant {
                group: "g1".into(),
                index: 0,
                model: None,
            }),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: "t2".into(),
            message_id: "m3".into(),
            text: "Draft A.".into(),
            tool_calls: vec![],
            blocks: vec![],
            author: Some("writer".into()),
            variant: Some(Variant {
                group: "g1".into(),
                index: 0,
                model: None,
            }),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: "t2".into(),
            message_id: "m4".into(),
            text: "Draft B.".into(),
            tool_calls: vec![],
            blocks: vec![],
            author: Some("writer".into()),
            variant: Some(Variant {
                group: "g1".into(),
                index: 1,
                model: None,
            }),
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: "t2".into(),
            call_id: "c4".into(),
            tool: "read".into(),
            input: json!({"path": "CHANGELOG.md"}),
            author: Some("writer".into()),
        }),
        EventKind::VariantSelected(VariantSelected {
            group: "g1".into(),
            message_id: "m4".into(),
        }),
        // An answer to a question: the canonical text plus the reply.
        EventKind::UserMessage(UserMessage {
            turn_id: "t3".into(),
            text: "Choice answer · next: o1".into(),
            attachments: vec![],
            reply: Some(FormReply {
                message_id: "m2".into(),
                block_id: "next".into(),
                values: json!({"selected": ["o1"]}).as_object().unwrap().clone(),
            }),
            images: vec![],
        }),
        // A `publish` call and the file the node staged for it, then a
        // file the user saved, and one that could not be saved.
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: "t3".into(),
            call_id: "c-pub".into(),
            tool: "publish".into(),
            input: json!({"path": "target/release/app"}),
        }),
        EventKind::ArtifactStaged(Box::new(staged(Some("c-pub"), StagedBy::Agent))),
        EventKind::ArtifactStaged(Box::new(staged(None, StagedBy::User))),
        EventKind::ArtifactStageFailed(ArtifactStageFailed {
            path: "out/missing.bin".into(),
            name: None,
            error: "out/missing.bin: no such file or folder".into(),
        }),
    ];
    let mut out: Vec<RawEvent> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 1_700_000_000_000 + i as i64,
                kind,
                payload,
            }
        })
        .collect();
    let n = out.len() as u64;
    out.push(RawEvent {
        session_id: sid,
        seq: n + 1,
        ts_ms: 0,
        kind: "from_a_newer_node".into(),
        payload: json!({"anything": true}),
    });
    out
}

#[test]
fn fixture_is_current() {
    let evs = events();
    let expected = SessionView::fold(&evs).unwrap();
    let doc = json!({ "events": evs, "expected": expected });
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/session.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk =
        std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1");
    // Compare values, not text: JSON key order depends on serde_json's
    // `preserve_order` feature, which other crates may enable workspace-wide.
    let on_disk: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(on_disk, doc, "fixture is stale; run with UPDATE_FIXTURES=1");
}

/// What the fixture must hold, so a regression cannot just be regenerated
/// into it: the streamed message takes its blocks from the final event.
#[test]
fn a_streamed_message_gets_its_final_blocks() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let m1 = view["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["message_id"] == "m1")
        .unwrap();
    assert_eq!(m1["text"], STREAMED.concat());
    assert_eq!(m1["blocks"][0]["id"], "look");
    assert!(m1["blocks"][0]["spec"].is_object());
    let users: Vec<&serde_json::Value> = view["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "user")
        .collect();
    assert!(users[0].get("reply").is_none());
    assert_eq!(users[2]["reply"]["block_id"], "next");
    // Images are copied as references, and left out when there are none.
    assert!(users[0].get("images").is_none());
    assert_eq!(users[1]["images"][0]["name"], "screenshot.png");
    assert_eq!(users[1]["images"][0]["width"], 1280);
    assert!(users[1]["images"][0].get("data").is_none());
    assert!(view["meta"]["parent_session"].is_string());
}

/// Staged files: the agent's joins its `publish` card, the user's and the
/// failure stand alone.
#[test]
fn staged_files_join_their_card_or_stand_alone() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let items = view["items"].as_array().unwrap();
    let card = items.iter().find(|i| i["call_id"] == "c-pub").unwrap();
    assert_eq!(card["staged"]["name"], "app");
    assert_eq!(card["staged"]["by"], "agent");
    assert!(card["staged"]["packed"].is_null());
    let alone: Vec<&serde_json::Value> = items.iter().filter(|i| i["type"] == "artifact").collect();
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0]["staged"]["by"], "user");
    assert!(alone[0]["staged"].get("call_id").is_none());
    let failed = items
        .iter()
        .find(|i| i["type"] == "artifact_failed")
        .unwrap();
    assert!(failed["name"].is_null());
    assert_eq!(failed["path"], "out/missing.bin");
    let other_tools = items
        .iter()
        .filter(|i| i["type"] == "tool" && i["call_id"] != "c-pub");
    assert!(other_tools.into_iter().all(|t| t.get("staged").is_none()));
}
