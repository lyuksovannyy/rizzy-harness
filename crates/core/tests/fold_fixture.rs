//! Shared reducer fixture: `web/src/fold.test.ts` folds the same events with
//! the TypeScript port and must produce exactly `expected`.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test fold_fixture`.

use harness_core::agent::UiLevel;
use harness_core::artifacts::{ArtifactMeta, GitInfo, StagedBy};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::todo::{TodoItem, TodoStatus};
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::json;

fn todo(content: &str, status: TodoStatus) -> TodoItem {
    TodoItem {
        content: content.into(),
        status,
    }
}

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

/// A usage naming `models`, `each` input tokens apiece.
fn forged(models: &[&str], each: u64) -> Usage {
    Usage {
        input_tokens: each * models.len() as u64,
        by_model: models
            .iter()
            .map(|m| {
                (
                    m.to_string(),
                    ModelTokens {
                        input_tokens: each,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..Default::default()
    }
}

fn events() -> Vec<RawEvent> {
    let sid: SessionId = "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap();
    let t = || "t1".to_string();
    let mut kinds = vec![
        EventKind::SessionCreated(SessionCreated {
            // The fold keeps the session's mode (N.10a).
            exec_mode: Some(ExecMode::Sandbox),
            no_callbacks: false,
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
            cause: None,
            agent: None,
            turn_id: t(),
            environment: None, continued_by: None,
            retry: None,
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: t(),
            text: "fix the bug".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            triggers: vec![],
            tags: vec![],
            files: vec![],
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
            thinking: None,
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
            chat_allow: false,
            preview: None,
        }),
        EventKind::ApprovalResolved(ApprovalResolved {
            approval_id: "a1".into(),
            decision: Decision::AllowSession,
            by: ResolvedBy::User,
            note: None,
        }),
        EventKind::ToolOutputDelta(ToolOutputDelta {
            call_id: "c1".into(),
            stream: OutputStream::Stdout,
            text: "running 3 tests\n".into(),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            image: None,
            seen: None,
            call_id: "c1".into(),
            ok: false,
            output: "1 failed".into(),
            exit_code: Some(101),
            truncated: false,
            sources: vec![],
            note: None,
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: t(),
            call_id: "c2".into(),
            tool: "read".into(),
            input: json!({"path": "src/lib.rs"}),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            image: None,
            seen: None,
            call_id: "c2".into(),
            ok: true,
            output: "     1\tfn main() {}".into(),
            exit_code: None,
            truncated: false,
            sources: vec![],
            note: None,
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
            image: None,
            seen: None,
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
            note: None,
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
            thinking: None,
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
            chat_allow: false,
            preview: Some(
                "rows 2-3 are replaced:\n     2\t<script>x</script>\n     3\t[a](http://e.test)\n"
                    .into(),
            ),
        }),
        EventKind::Checkpoint(Checkpoint {
            turn_id: t(),
            phase: CheckpointPhase::Before,
            git_ref: "refs/x".into(),
            files: vec![],
            excluded: vec![],
            excluded_more: 0,
            head_moved: None,
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
                withheld: vec![],
                effort: None,
            },
            model: Some("demo::mock".into()),
        }),
        EventKind::TurnStarted(TurnStarted {
            cause: None,
            turn_id: "t2".into(),
            agent: Some(AgentRef {
                agent_id: "writer".into(),
                name: "Writer".into(),
                prompt_hash: "h2".into(),
                withheld: vec![],
                effort: None,
            }),
            environment: Some("[harness] Environment of this turn: the node's clock reads Saturday 2026-11-14 10:32 (Europe/Riga, UTC+02:00).".into()), continued_by: None,
            retry: None,
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
            queued: false,
            triggers: vec![],
            tags: vec![],
            // A mentioned file (T63): the view shows its path, never its text.
            files: vec![FileMention {
                path: "CHANGELOG.md".into(),
                state: MentionState::File,
                text: "## 1.0\n".into(),
                lines: 1,
                shown: 1,
                bytes: 0,
                note: None,
                seen: None,
            }],
        }),
        EventKind::HistorySummarized(HistorySummarized {
            turn_id: "t2".into(),
            through_seq: 3,
            text: "User asked to fix a bug.".into(),
            ..Default::default()
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
            thinking: None,
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
            thinking: None,
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: "t2".into(),
            call_id: "c4".into(),
            tool: "read".into(),
            input: json!({"path": "CHANGELOG.md"}),
            author: Some("writer".into()),
        }),
        // Sent while turn t2 ran (X.5): both shown where they were sent;
        // the first read at the next step (seq 32), the second not yet.
        EventKind::UserMessage(UserMessage {
            turn_id: "t2".into(),
            text: "also bump the version".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: true,
            triggers: vec![],
            tags: vec![],
            files: vec![],
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: "t2".into(),
            text: "and tag it".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: true,
            triggers: vec![],
            tags: vec![],
            files: vec![],
        }),
        EventKind::QueuedDelivered(QueuedDelivered {
            turn_id: "t2".into(),
            seqs: vec![32],
        }),
        EventKind::VariantSelected(VariantSelected {
            group: "g1".into(),
            message_id: "m4".into(),
        }),
        // The owner set the chat's step limit; the node clamped it (X.14).
        EventKind::StepLimitChanged(StepLimitChanged {
            max_steps: 1000,
            requested: Some(5000),
        }),
        // A turn that used its step budget ends at a limit, not an error.
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t2".into(),
            reason: TurnEndReason::Limit,
            usage: None,
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
            queued: false,
            triggers: vec![],
            tags: vec![],
            files: vec![],
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
        // T.8d: one background command ends, one still runs.
        EventKind::BackgroundStarted(BackgroundStarted {
            child: None,
            bg_id: "bg-1".into(),
            call_id: "c1".into(),
            turn_id: t(),
            timeout_s: 600,
            ends_by_ms: 1_700_000_600_000,
        }),
        EventKind::BackgroundStarted(BackgroundStarted {
            child: None,
            bg_id: "bg-2".into(),
            call_id: "c1".into(),
            turn_id: t(),
            timeout_s: 60,
            ends_by_ms: 1_700_000_060_000,
        }),
        EventKind::BackgroundEnded(BackgroundEnded {
            child: None,
            bg_id: "bg-1".into(),
            reason: BackgroundReason::Exited,
            exit_code: Some(1),
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: 4200,
            output_bytes: 12,
            tail: "<b>not html</b>".into(),
            tail_cut: false,
            delivered: true,
        }),
        // T.8d step 3: an end while idle resumes the agent (a callback
        // turn, no user message), and a later one is not resumed.
        EventKind::BackgroundEnded(BackgroundEnded {
            child: None,
            bg_id: "bg-2".into(),
            reason: BackgroundReason::Exited,
            exit_code: Some(1),
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: 9000,
            output_bytes: 5,
            tail: "fail\n".into(),
            tail_cut: false,
            delivered: false,
        }),
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t4".into(),
            agent: None,
            cause: Some(TurnCause::Callback(vec!["bg-2".into()])),
            environment: None, continued_by: None,
            retry: None,
        }),
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: "t4".into(),
            message_id: "m5".into(),
            text: "The build failed.".into(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: None,
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t4".into(),
            reason: TurnEndReason::Completed,
            usage: None,
        }),
        EventKind::CallbackRefused(CallbackRefused {
            bg_ids: vec!["bg-5".into()],
            reason: "3 turns in a row without you (node limit)".into(),
        }),
        // X.24: a background sub-agent ends with its result and its cost
        // (the chat's usage counts it); another still runs.
        EventKind::BackgroundStarted(BackgroundStarted {
            bg_id: "bg-6".into(),
            call_id: "c1".into(),
            turn_id: t(),
            timeout_s: 1800,
            ends_by_ms: 1_700_001_800_000,
            child: Some(BackgroundChild {
                session: "01a19bf2-0000-7000-8000-0000000000c6".parse().unwrap(),
                agent_id: "coder".into(),
            }),
        }),
        EventKind::BackgroundStarted(BackgroundStarted {
            bg_id: "bg-7".into(),
            call_id: "c1".into(),
            turn_id: t(),
            timeout_s: 1800,
            ends_by_ms: 1_700_001_800_000,
            child: Some(BackgroundChild {
                session: "01a19bf2-0000-7000-8000-0000000000c7".parse().unwrap(),
                agent_id: "scout".into(),
            }),
        }),
        EventKind::BackgroundEnded(BackgroundEnded {
            bg_id: "bg-6".into(),
            reason: BackgroundReason::Finished,
            exit_code: None,
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: 61_000,
            output_bytes: 0,
            tail: String::new(),
            tail_cut: false,
            delivered: true,
            child: Some(Box::new(ChildEnded {
                session: "01a19bf2-0000-7000-8000-0000000000c6".parse().unwrap(),
                agent_id: "coder".into(),
                result: "session: 01a19bf2-0000-7000-8000-0000000000c6\nSub-agent @coder finished: done.\n| <b>not html</b>".into(),
                ok: true,
                usage: Some(forged(&["m-a", "m-b"], 100)),
            })),
        }),
        // Hand-off notes (X.16): one written, with its cost in the chat's
        // usage; one whose models failed (the fixed part alone).
        EventKind::HandoffWritten(HandoffWritten {
            through_seq: 40,
            text: Some("Status:\nThe build fails.".into()),
            model: Some("demo::small".into()),
            failed: vec![],
            usage: Some(Usage {
                input_tokens: 900,
                output_tokens: 60,
                by_model: [(
                    "demo::small".to_string(),
                    ModelTokens {
                        input_tokens: 900,
                        output_tokens: 60,
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            }),
        }),
        EventKind::HandoffWritten(HandoffWritten {
            through_seq: 41,
            text: None,
            model: None,
            failed: vec![NoteFailure {
                model: "demo::small".into(),
                reason: "unknown to this node".into(),
            }],
            usage: None,
        }),
        // A person's `/compact` (X.20): one that failed (its cost counted),
        // then one written between turns with its focus, its cost and the
        // context size the meter shows until the next turn reports.
        EventKind::CompactFailed(CompactFailed {
            reason: "The chat was not summarised: the summary was not smaller than what it \
                     would replace. Nothing was lost."
                .into(),
            usage: Some(Usage {
                input_tokens: 500,
                output_tokens: 400,
                ..Default::default()
            }),
        }),
        EventKind::HistorySummarized(HistorySummarized {
            turn_id: String::new(),
            through_seq: 42,
            text: "Goal\nMigrate the users table.".into(),
            manual: true,
            focus: Some("keep the database migration details".into()),
            usage: Some(Usage {
                input_tokens: 2_000,
                output_tokens: 300,
                by_model: [(
                    "demo::mock".to_string(),
                    ModelTokens {
                        input_tokens: 2_000,
                        output_tokens: 300,
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            }),
            tokens_after: Some(1_234),
        }),
        // The agent's task list: each event replaces the whole list.
        EventKind::TodoUpdated(TodoUpdated {
            turn_id: "t4".into(),
            call_id: "c-plan-1".into(),
            items: vec![
                todo("Read the code", TodoStatus::InProgress),
                todo("Fix the build", TodoStatus::Pending),
            ],
        }),
        EventKind::TodoUpdated(TodoUpdated {
            turn_id: "t4".into(),
            call_id: "c-plan-2".into(),
            items: vec![
                todo("Read the code", TodoStatus::Done),
                todo("Fix the build", TodoStatus::InProgress),
                todo("Run the tests", TodoStatus::Pending),
            ],
        }),
        // A denial with the person's note (T51): the card keeps it.
        EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: "t4".into(),
            call_id: "c-deny".into(),
            tool: "bash".into(),
            input: json!({"command": "rm -rf build"}),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: "a-deny".into(),
            call_id: "c-deny".into(),
            tool: "bash".into(),
            input: json!({"command": "rm -rf build"}),
            reason: "runs programs".into(),
            turn_allow: true,
            chat_allow: true,
            preview: None,
        }),
        EventKind::ApprovalResolved(ApprovalResolved {
            approval_id: "a-deny".into(),
            decision: Decision::Deny,
            by: ResolvedBy::User,
            note: Some("keep the build folder".into()),
        }),
        // A call allowed for the rest of the chat, another allowed and then
        // removed (T51): the view lists only the first.
        EventKind::ChatAllowAdded(ChatAllowAdded {
            grant_id: "g1".into(),
            approval_id: "a1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo test"}),
            key: "ab".repeat(32),
        }),
        EventKind::ChatAllowAdded(ChatAllowAdded {
            grant_id: "g2".into(),
            approval_id: "a1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo build"}),
            key: "cd".repeat(32),
        }),
        EventKind::ChatAllowRemoved(ChatAllowRemoved {
            grant_id: "g2".into(),
        }),
        // The owner's mode changes (T50): the latest is the chat's mode; a
        // clamped one says what was asked.
        EventKind::ApprovalModeChanged(ApprovalModeChanged {
            mode: ApprovalMode::Auto,
            requested: None,
        }),
        EventKind::ApprovalModeChanged(ApprovalModeChanged {
            mode: ApprovalMode::AcceptEdits,
            requested: Some(ApprovalMode::Auto),
        }),
        // A retry shows while the call waits, and goes once output resumes;
        // a second one is still waiting at the fixture's end.
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t5".into(),
            agent: None,
            cause: None,
            environment: None, continued_by: None,
            retry: None,
        }),
        EventKind::ModelRetry(ModelRetry {
            turn_id: "t5".into(),
            model: "openai::gpt-4o".into(),
            attempt: 1,
            of: 3,
            at_ms: 1_700_000_002_000,
            wait_ms: 2000,
            reason: RetryReason::Overloaded,
        }),
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: "t5".into(),
            message_id: "m6".into(),
            text: "Back.".into(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: None,
        }),
        // Model ids come from the node, so a forged event is folded like
        // any other: prototype names are plain buckets ("constructor" is
        // in web/src/fold.test.ts: vitest's strict equality compares
        // `.constructor`), and with two
        // slots left at the cap "10" and "11" are admitted before "9"
        // (byte order, as `Usage::add` admits; review of PR 119 M1).
        EventKind::TurnEnded(TurnEnded {
            turn_id: "forged-1".into(),
            reason: TurnEndReason::Completed,
            usage: Some(forged(
                &[
                    "openai::gpt-4o",
                    "__proto__",
                    "toString",
                    "p::f00",
                    "p::f01",
                    "p::f02",
                    "p::f03",
                    "p::f04",
                    "p::f05",
                    "p::f06",
                    "p::f07",
                    "p::f08",
                    "p::f09",
                ],
                7,
            )),
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: "forged-2".into(),
            reason: TurnEndReason::Completed,
            usage: Some(forged(&["9", "10", "11"], 3)),
        }),
        // The turn's limits reach the view; the totals never carry them.
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t5".into(),
            reason: TurnEndReason::Completed,
            // Who used which tokens (the cost estimate's input): the main
            // model with cache reads and writes, a sub-agent on the small
            // model, and 100 input tokens no model was stamped on.
            usage: Some(Usage {
                input_tokens: 900,
                output_tokens: 10,
                context_tokens: 800,
                cache_read_tokens: 300,
                cache_write_tokens: 100,
                window: Some(128_000),
                summary_at: Some(102_400),
                model: Some("openai::gpt-4o".into()),
                by_model: [
                    (
                        "openai::gpt-4o".to_string(),
                        ModelTokens {
                            input_tokens: 600,
                            output_tokens: 8,
                            cache_read_tokens: 300,
                            cache_write_tokens: 100,
                        },
                    ),
                    (
                        "demo::small".to_string(),
                        ModelTokens {
                            input_tokens: 200,
                            output_tokens: 2,
                            ..Default::default()
                        },
                    ),
                ]
                .into(),
            }),
        }),
        // A sub-agent's follow-up from its parent's call (T68).
        // A turn that failed partway, then the person's Retry of it: a new
        // turn naming it, with no second message (X.21).
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t7".into(),
            agent: None,
            cause: None,
            environment: None,
            retry: None,
            continued_by: None,
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: "t7".into(),
            text: "Migrate the schema.".into(),
            attachments: vec![],
            files: vec![],
            reply: None,
            images: vec![],
            queued: false,
            triggers: vec![],
            tags: vec![],
        }),
        EventKind::Error(ErrorEvent {
            turn_id: Some("t7".into()),
            message: "The model is not available.".into(),
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t7".into(),
            reason: TurnEndReason::Error,
            usage: None,
        }),
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t8".into(),
            agent: None,
            cause: None,
            environment: None,
            retry: Some(RetryOf {
                turn_id: "t7".into(),
                reason: TurnEndReason::Error,
                error: Some("The model is not available.".into()),
                failed_model: Some("demo::small".into()),
                model: Some("openai::gpt-4o".into()),
            }),
            continued_by: None,
        }),
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: "t8".into(),
            message_id: "m8".into(),
            text: "Migrated.".into(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: None,
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t8".into(),
            reason: TurnEndReason::Completed,
            usage: None,
        }),
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t6".into(),
            agent: None,
            cause: None,
            environment: None,
            continued_by: Some("call-follow-up".into()),
            retry: None,
        }),
        EventKind::ContextShortened(ContextShortened {
            turn_id: "t6".into(),
            cleared: Some(Cleared {
                turn_id: "t6".into(),
                results: 1,
            }),
            folded: Some(Folded {
                steps: 2,
                text: "Read <b>main.rs</b>; the bug is in parse.".into(),
                todo: None,
            }),
            thinking: None,
            tokens_before: 31_000,
            tokens_after: 9_000,
        }),
        // X.25, before the waiting retry (an answer ends a wait): a step's thinking is one row
        // before what the step said, also when the text streamed first;
        // only the readable text shows, a redacted block is counted, and
        // the opaque parts never reach the view.
        EventKind::AssistantDelta(AssistantDelta {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m-th1".into(),
            text: "Checking ".into(),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m-th1".into(),
            text: "Checking <b>now</b>.".into(),
            tool_calls: vec![ToolCallRef {
                call_id: "c-th1".into(),
                tool: "bash".into(),
                input: json!({"command": "ls"}),
            }],
            blocks: vec![],
            thinking: Some(Thinking {
                model: "anthropic::claude-x".into(),
                blocks: vec![
                    ThinkingBlock::Signed {
                        text: "The <script>tests</script> first.".into(),
                        signature: "c2lnbmVk".into(),
                    },
                    ThinkingBlock::Redacted {
                        data: "b3BhcXVl".into(),
                    },
                    ThinkingBlock::Signed {
                        text: String::new(),
                        signature: "ZW1wdHk=".into(),
                    },
                ],
                prefix: "ab".repeat(32),
                unsent: false,
            }),
        }),
        // A step that only calls a tool, with plain reasoning text.
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m-th2".into(),
            text: String::new(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: Some(Thinking {
                model: "deepseek::chat".into(),
                blocks: vec![ThinkingBlock::Text {
                    text: "Plain reasoning.".into(),
                }],
                prefix: String::new(),
                unsent: true,
            }),
        }),
        // Thinking with nothing readable (omitted display): no row.
        EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t(),
            message_id: "m-th3".into(),
            text: "Done.".into(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: Some(Thinking {
                model: "anthropic::claude-x".into(),
                blocks: vec![ThinkingBlock::Signed {
                    text: "  ".into(),
                    signature: "c2ln".into(),
                }],
                prefix: String::new(),
                unsent: false,
            }),
        }),
        // The oldest steps' thinking left out of the requests: a line says so.
        EventKind::ContextShortened(ContextShortened {
            turn_id: t(),
            cleared: None,
            folded: None,
            thinking: Some(Unthought {
                turn_id: t(),
                steps: 2,
            }),
            tokens_before: 190_000,
            tokens_after: 60_000,
        }),
        // A refused replay: recorded for the requests, shows nothing.
        EventKind::ThinkingWithheld(ThinkingWithheld {
            turn_id: t(),
            model: "anthropic::claude-x".into(),
            status: 400,
            reason: "thinking blocks cannot be modified".into(),
        }),
        EventKind::ModelRetry(ModelRetry {
            turn_id: "t6".into(),
            model: "openai::gpt-4o".into(),
            attempt: 2,
            of: 3,
            at_ms: 1_700_000_004_000,
            wait_ms: 4000,
            reason: RetryReason::NoAnswer,
        }),
        // What the model was sent with the turn's message (X.1): never
        // shown, and the wait it is in stays shown.
        EventKind::ContextSent(ContextSent {
            turn_id: "t6".into(),
            frame: ContextFrame::Person,
            sections: vec![SentSection {
                key: "instructions".into(),
                digest: "ab12".into(),
                text: "Run cargo fmt.".into(),
                note: None,
            }],
        }),
        // X.23, last so no seq above moves: the provider refused a request
        // carrying c2's image; its row says so (an unknown id is ignored).
        EventKind::ImagesWithheld(ImagesWithheld {
            turn_id: t(),
            calls: vec!["c2".into(), "nope".into()],
            status: 400,
            reason: "Could not process image".into(),
        }),
    ];
    // X.37: a message that called three agents; the second answers in a turn
    // of its own (a mention turn), the third was not asked (Stop).
    let pos = kinds
        .iter()
        .position(|k| matches!(k, EventKind::CallbackRefused(_)))
        .unwrap()
        + 1;
    let at = pos as u64 + 2;
    let agent_ref = |id: &str| AgentRef {
        agent_id: id.into(),
        name: id.into(),
        prompt_hash: "h".into(),
        withheld: vec![],
        effort: None,
    };
    kinds.splice(
        pos..pos,
        [
            EventKind::TurnStarted(TurnStarted {
                turn_id: "t70".into(),
                agent: Some(agent_ref("coder")),
                cause: None,
                environment: None,
                continued_by: None,
                retry: None,
            }),
            EventKind::UserMessage(UserMessage {
                turn_id: "t70".into(),
                text: "@coder @writer @editor, yo".into(),
                attachments: vec![],
                files: vec![],
                reply: None,
                images: vec![],
                queued: false,
                triggers: vec!["coder".into(), "writer".into(), "editor".into()],
                tags: vec!["advisor".into()],
            }),
            EventKind::AssistantMessage(AssistantMessage {
                author: None,
                variant: None,
                turn_id: "t70".into(),
                message_id: "m70".into(),
                text: "Coder here.".into(),
                tool_calls: vec![],
                blocks: vec![],
                thinking: None,
            }),
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t70".into(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
            EventKind::TurnStarted(TurnStarted {
                turn_id: "t71".into(),
                agent: Some(agent_ref("writer")),
                cause: Some(TurnCause::Mention(MentionCall {
                    message_seq: at,
                    by: CalledBy::Person,
                    tags: vec!["advisor".into()],
                })),
                environment: None,
                continued_by: None,
                retry: None,
            }),
            EventKind::AssistantMessage(AssistantMessage {
                author: None,
                variant: None,
                turn_id: "t71".into(),
                message_id: "m71".into(),
                text: "Writer here.".into(),
                tool_calls: vec![],
                blocks: vec![],
                thinking: None,
            }),
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t71".into(),
                reason: TurnEndReason::Cancelled,
                usage: None,
            }),
            EventKind::Checkpoint(Checkpoint {
                turn_id: t(),
                phase: CheckpointPhase::After,
                git_ref: "refs/y".into(),
                files: vec!["a.txt".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: Some(HeadMoved {
                    from: HeadAt {
                        branch: Some("main".into()),
                        commit: "a".repeat(40),
                    },
                    to: HeadAt {
                        branch: None,
                        commit: "b".repeat(40),
                    },
                }),
            }),
            // X.42: an undone turn remembers the checkpoint its undo saved.
            EventKind::Checkpoint(Checkpoint {
                turn_id: "t9".into(),
                phase: CheckpointPhase::Before,
                git_ref: "refs/t9-before".into(),
                files: vec![],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }),
            EventKind::Checkpoint(Checkpoint {
                turn_id: "t9".into(),
                phase: CheckpointPhase::After,
                git_ref: "refs/t9-after".into(),
                files: vec!["b.txt".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }),
            EventKind::Checkpoint(Checkpoint {
                turn_id: "restore-z".into(),
                phase: CheckpointPhase::BeforeRestore,
                git_ref: "refs/restore-z-beforerestore".into(),
                files: vec!["b.txt".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }),
            EventKind::Checkpoint(Checkpoint {
                turn_id: "restore-z".into(),
                phase: CheckpointPhase::After,
                git_ref: "refs/restore-z-after".into(),
                files: vec!["b.txt".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }),
            EventKind::Checkpoint(Checkpoint {
                turn_id: "t9".into(),
                phase: CheckpointPhase::Restored,
                git_ref: "refs/t9-restored".into(),
                files: vec!["b.txt".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }),
            EventKind::PeersSkipped(PeersSkipped {
                message_seq: at,
                agents: vec!["editor".into()],
                reason: PeersSkipReason::Stopped,
            }),
        ],
    );
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
    // The events as a browser is sent them (X.25: no signature, redacted
    // data or prefix; `RawEvent::for_browser`); the view is the same either
    // way, and the TypeScript fold reads only what a browser gets.
    let sent: Vec<serde_json::Value> = evs.iter().map(RawEvent::for_browser).collect();
    let doc = json!({ "events": sent, "expected": expected });
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
    assert_eq!(users[4]["reply"]["block_id"], "next");
    // Sent while a turn ran: where it was sent, read once delivered.
    assert_eq!(
        users[2]["queued"],
        serde_json::json!({"seq": 32, "read": true})
    );
    assert_eq!(
        users[3]["queued"],
        serde_json::json!({"seq": 33, "read": false})
    );
    assert!(users[0].get("queued").is_none());
    // Images are copied as references, and left out when there are none.
    assert!(users[0].get("images").is_none());
    assert_eq!(users[1]["images"][0]["name"], "screenshot.png");
    assert_eq!(users[1]["images"][0]["width"], 1280);
    assert!(users[1]["images"][0].get("data").is_none());
    assert!(view["meta"]["parent_session"].is_string());
    // The latest mode change is the chat's mode (T50).
    assert_eq!(view["meta"]["approval_mode"], "accept_edits");
}

/// A callback turn stands where a message would, with the ids it was
/// started for; a refusal is its own item with its reason.
#[test]
fn a_callback_turn_and_a_refusal_are_items() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let items = view["items"].as_array().unwrap();
    let cb = items.iter().position(|i| i["type"] == "callback").unwrap();
    assert_eq!(items[cb]["bg_ids"], json!(["bg-2"]));
    assert_eq!(items[cb]["turn_id"], "t4");
    assert_eq!(items[cb + 1]["message_id"], "m5");
    let r = items
        .iter()
        .find(|i| i["type"] == "callback_refused")
        .unwrap();
    assert_eq!(r["bg_ids"], json!(["bg-5"]));
    assert!(r["reason"].as_str().unwrap().contains("node limit"));
}

/// X.37: the message records who it called; the turn of an agent called
/// after the first is a `called` item (not a callback) where its answer
/// begins; the agents never asked are one quiet item.
#[test]
fn a_mention_turn_and_the_agents_not_asked_are_items() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let items = view["items"].as_array().unwrap();
    let u = items
        .iter()
        .find(|i| i["type"] == "user" && i["turn_id"] == "t70")
        .unwrap();
    assert_eq!(u["triggers"], json!(["coder", "writer", "editor"]));
    assert_eq!(u["tags"], json!(["advisor"]));
    let c = items.iter().position(|i| i["type"] == "called").unwrap();
    assert_eq!(items[c]["turn_id"], "t71");
    assert_eq!(items[c]["agent"]["agent_id"], "writer");
    let evs = events();
    let msg_seq = evs
        .iter()
        .find(|e| e.kind == "user_message" && e.payload["turn_id"] == "t70")
        .unwrap()
        .seq;
    assert_eq!(items[c]["message_seq"], msg_seq);
    assert_eq!(items[c + 1]["message_id"], "m71");
    assert_eq!(items.iter().filter(|i| i["type"] == "callback").count(), 1);
    let p = items.iter().find(|i| i["type"] == "peers_skipped").unwrap();
    assert_eq!(p["agents"], json!(["editor"]));
    assert_eq!(p["reason"], "stopped");
}

/// A retry stands right after the failed turn's end, naming it; the
/// message is one item, before the failed attempt (X.21).
#[test]
fn a_retry_is_an_item_after_the_failed_turn() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let items = view["items"].as_array().unwrap();
    let r = items.iter().position(|i| i["type"] == "retry").unwrap();
    assert_eq!(items[r]["of"], "t7");
    assert_eq!(items[r]["turn_id"], "t8");
    assert_eq!(items[r]["reason"], "error");
    assert_eq!(items[r - 1]["type"], "turn_end");
    assert_eq!(items[r - 1]["reason"], "error");
    assert_eq!(items[r + 1]["message_id"], "m8");
    assert_eq!(
        items
            .iter()
            .filter(|i| i["type"] == "user" && i["text"] == "Migrate the schema.")
            .count(),
        1
    );
}

/// A shortened context is a quiet item in its turn: whether results were
/// cleared, how many steps were summarised, the summary as plain text (the
/// UI renders it as text, never as markup), and the sizes.
#[test]
fn a_shortened_context_is_an_item() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let items = view["items"].as_array().unwrap();
    let s = items.iter().find(|i| i["type"] == "shortened").unwrap();
    assert_eq!(s["turn_id"], "t6");
    assert_eq!(s["cleared"], true);
    assert_eq!(s["steps"], 2);
    assert_eq!(s["text"], "Read <b>main.rs</b>; the bug is in parse.");
    assert_eq!(
        (s["tokens_before"].as_u64(), s["tokens_after"].as_u64()),
        (Some(31_000), Some(9_000))
    );
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

/// The view holds the latest task list only, as the last event set it.
#[test]
fn the_latest_task_list_is_the_list() {
    let view = SessionView::fold(&events()).unwrap();
    assert_eq!(
        view.todo,
        vec![
            todo("Read the code", TodoStatus::Done),
            todo("Fix the build", TodoStatus::InProgress),
            todo("Run the tests", TodoStatus::Pending),
        ]
    );
    // No list: nothing serialized, as the web fold leaves it out.
    let empty = serde_json::to_value(SessionView::default()).unwrap();
    assert!(empty.get("todo").is_none(), "{empty}");
}

/// The second retry still waits at the end; the first was cleared by the
/// answer. The finished turn's limits are the view's, not the totals'.
#[test]
fn a_waiting_retry_and_the_turns_limits_are_in_the_view() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    assert_eq!(view["retry"]["attempt"], 2);
    assert_eq!(view["retry"]["reason"], "no_answer");
    assert_eq!(view["retry"]["at_ms"], 1_700_000_004_000_i64);
    assert_eq!(view["context_window"], 128_000);
    assert_eq!(view["summary_at"], 102_400);
    assert_eq!(view["context_tokens"], 800);
    assert_eq!(view["context_model"], "openai::gpt-4o");
    assert!(view["usage"].get("window").is_none());
}

/// The card's preview survives the fold, as it is, and only where there is
/// one (the TypeScript fold reads the same fixture).
#[test]
fn the_fold_keeps_an_approvals_preview() {
    let view = serde_json::to_value(SessionView::fold(&events()).unwrap()).unwrap();
    let previews: Vec<&str> = view["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["approval"]["preview"].as_str())
        .collect();
    assert_eq!(previews.len(), 1, "{previews:?}");
    assert!(previews[0].contains("<script>x</script>"));
}
