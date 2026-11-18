//! The event scrubber keeps the schema (PLAN.md T3): whatever the secret set
//! holds, a stored event decodes as the same kind and keeps every field
//! name, type tag, enum value, number and harness-written value; only its
//! free text (what the model, a tool, a provider or a person wrote) is
//! scrubbed, and there exactly as before. Every event kind is built here
//! (an exhaustive `match` keeps a new kind from being forgotten) and
//! compared with an oracle that knows, kind by kind, which paths are free.

use std::collections::BTreeMap;

use harness_core::artifacts::{ArtifactMeta, Entry, GitInfo, Package, StagedBy};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::secret::schema::{Class, KEPT, Outcome, positions};
use harness_core::todo::{TodoItem, TodoStatus};
use harness_core::view::SessionView;
use harness_core::{RawEvent, Secret, SecretSet, SessionId};
use serde_json::{Value, json};

/// A random-looking 40-character value, as a real key would be.
const CANARY: &str = "k9Qz7WvR2pLx8TnB4cYh6JmD1sFa3GuE5oKiNwXe";

fn sid() -> SessionId {
    "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap()
}

/// What a tool's input, a ui spec or a form's values hold: model-written
/// JSON. Its keys are free too, even one named like a harness field.
fn free_json(w: &str) -> Value {
    // A renamed key inside a renamed key's object, too.
    let mut deep = serde_json::Map::new();
    deep.insert(w.to_string(), json!(w));
    let mut inner = serde_json::Map::new();
    inner.insert(
        w.to_string(),
        json!({ "model": w, "turn_id": [w, 7], "deep": Value::Object(deep) }),
    );
    json!({ "message_id": w, "cwd": [w], "nested": Value::Object(inner), "n": 3 })
}

fn usage(w: &str) -> Usage {
    Usage {
        input_tokens: 1,
        output_tokens: 2,
        context_tokens: 3,
        cache_read_tokens: 4,
        cache_write_tokens: 5,
        window: Some(6),
        summary_at: Some(7),
        model: Some(w.into()),
        by_model: BTreeMap::from([(
            w.to_string(),
            ModelTokens {
                input_tokens: 1,
                output_tokens: 2,
                cache_read_tokens: 3,
                cache_write_tokens: 4,
            },
        )]),
    }
}

fn agent(w: &str) -> AgentRef {
    AgentRef {
        agent_id: w.into(),
        name: w.into(),
        prompt_hash: w.into(),
        withheld: vec![w.into()],
        effort: Some(w.into()),
    }
}

fn seen(w: &str) -> FileSeen {
    FileSeen {
        path: w.into(),
        sha256: w.into(),
        base: Some(w.into()),
    }
}

fn todo(w: &str) -> Vec<TodoItem> {
    vec![TodoItem {
        content: w.into(),
        status: TodoStatus::InProgress,
    }]
}

/// Every kind, every optional part present, every `String` set to `w`.
fn every_kind(w: &str) -> Vec<EventKind> {
    let s = || w.to_string();
    vec![
        EventKind::SessionCreated(SessionCreated {
            title: s(),
            cwd: s(),
            model: s(),
            backend: s(),
            node_id: s(),
            approval_mode: ApprovalMode::AcceptEdits,
            profile: Profile::Agentic,
            agent_id: Some(s()),
            incognito: true,
            parent_session: Some(sid()),
            no_callbacks: true,
            exec_mode: Some(ExecMode::Sandbox),
        }),
        EventKind::TurnStarted(TurnStarted {
            turn_id: s(),
            agent: Some(agent(w)),
            cause: Some(TurnCause::Callback(vec![s()])),
            environment: Some(s()),
            continued_by: Some(s()),
            retry: Some(RetryOf {
                turn_id: s(),
                reason: TurnEndReason::Interrupted,
                error: Some(s()),
                failed_model: Some(s()),
                model: Some(s()),
            }),
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: s(),
            text: s(),
            attachments: vec![Attachment {
                name: s(),
                mime: s(),
                text: s(),
                truncated: true,
            }],
            files: vec![FileMention {
                path: s(),
                state: MentionState::TooLarge,
                text: s(),
                lines: 1,
                shown: 2,
                bytes: 3,
                note: Some(s()),
                seen: Some(seen(w)),
            }],
            reply: Some(FormReply {
                message_id: s(),
                block_id: s(),
                values: free_json(w).as_object().unwrap().clone(),
            }),
            images: vec![ImageRef {
                media_id: s(),
                sha256: s(),
                mime: s(),
                name: s(),
                width: 1,
                height: 2,
            }],
            queued: true,
        }),
        EventKind::AssistantDelta(AssistantDelta {
            turn_id: s(),
            message_id: s(),
            text: s(),
            author: Some(s()),
            variant: Some(Variant {
                group: s(),
                index: 1,
                model: Some(s()),
            }),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: s(),
            message_id: s(),
            text: s(),
            tool_calls: vec![ToolCallRef {
                call_id: s(),
                tool: s(),
                input: free_json(w),
            }],
            author: Some(s()),
            variant: Some(Variant {
                group: s(),
                index: 2,
                model: Some(s()),
            }),
            blocks: vec![UiBlock {
                ordinal: 1,
                hash: s(),
                kind: UiKind::Progress,
                id: s(),
                spec: Some(free_json(w)),
                error: Some(s()),
            }],
            thinking: Some(Thinking {
                model: s(),
                blocks: vec![
                    ThinkingBlock::Signed {
                        text: s(),
                        signature: s(),
                    },
                    ThinkingBlock::Redacted { data: s() },
                    ThinkingBlock::Text { text: s() },
                ],
                prefix: s(),
                unsent: true,
            }),
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: s(),
            call_id: s(),
            tool: s(),
            input: free_json(w),
            author: Some(s()),
        }),
        EventKind::ToolOutputDelta(ToolOutputDelta {
            call_id: s(),
            stream: OutputStream::Stderr,
            text: s(),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: s(),
            ok: true,
            output: s(),
            exit_code: Some(1),
            truncated: true,
            sources: vec![Source {
                id: s(),
                url: s(),
                title: s(),
            }],
            note: Some(s()),
            seen: Some(seen(w)),
            image: Some(ToolImage {
                sha256: s(),
                mime: s(),
                width: 1,
                height: 2,
                bytes: 3,
            }),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: s(),
            call_id: s(),
            tool: s(),
            input: free_json(w),
            reason: s(),
            turn_allow: true,
            chat_allow: true,
        }),
        EventKind::ApprovalResolved(ApprovalResolved {
            approval_id: s(),
            decision: Decision::AllowSession,
            by: ResolvedBy::Cancelled,
            note: Some(s()),
        }),
        EventKind::Checkpoint(Checkpoint {
            turn_id: s(),
            phase: CheckpointPhase::BeforeRestore,
            git_ref: s(),
            files: vec![s()],
            excluded: vec![s()],
            excluded_more: 1,
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: s(),
            reason: TurnEndReason::Completed,
            usage: Some(usage(w)),
        }),
        EventKind::Error(ErrorEvent {
            turn_id: Some(s()),
            message: s(),
        }),
        EventKind::AgentChanged(AgentChanged {
            agent: agent(w),
            model: Some(s()),
        }),
        EventKind::VariantSelected(VariantSelected {
            group: s(),
            message_id: s(),
        }),
        EventKind::HistorySummarized(HistorySummarized {
            turn_id: s(),
            through_seq: 1,
            text: s(),
            manual: true,
            focus: Some(s()),
            usage: Some(usage(w)),
            tokens_after: Some(2),
        }),
        EventKind::ContextShortened(ContextShortened {
            turn_id: s(),
            cleared: Some(Cleared {
                turn_id: s(),
                results: 1,
            }),
            folded: Some(Folded {
                steps: 2,
                text: s(),
                todo: Some(todo(w)),
            }),
            thinking: Some(Unthought {
                turn_id: s(),
                steps: 3,
            }),
            tokens_before: 4,
            tokens_after: 5,
        }),
        EventKind::ModelFallback(ModelFallback {
            turn_id: s(),
            model: s(),
            from: s(),
            reason: Some(s()),
        }),
        EventKind::ModelRetry(ModelRetry {
            turn_id: s(),
            model: s(),
            attempt: 1,
            of: 2,
            at_ms: 3,
            wait_ms: 4,
            reason: RetryReason::Overloaded,
        }),
        EventKind::RetryGivenUp(RetryGivenUp {
            turn_id: s(),
            model: s(),
        }),
        EventKind::ArtifactStaged(Box::new(ArtifactStaged {
            stage_id: s(),
            call_id: Some(s()),
            by: StagedBy::Agent,
            name: s(),
            filename: s(),
            packed: Some(s()),
            size: 1,
            sha256: s(),
            kind_hint: Some(s()),
            source_path: s(),
            notes: Some(s()),
            meta: ArtifactMeta {
                toolchain: Some(s()),
                package: Some(Package {
                    name: s(),
                    version: s(),
                }),
                git: Some(GitInfo {
                    commit: s(),
                    branch: s(),
                    dirty: true,
                }),
                target: Some(s()),
                entries: Some(vec![Entry { path: s(), size: 2 }]),
                entries_total: Some(3),
                skipped: Some(BTreeMap::from([(s(), 4)])),
                source_path: Some(s()),
            },
        })),
        EventKind::ArtifactStageFailed(ArtifactStageFailed {
            path: s(),
            name: Some(s()),
            error: s(),
        }),
        EventKind::BackgroundStarted(BackgroundStarted {
            bg_id: s(),
            call_id: s(),
            turn_id: s(),
            timeout_s: 1,
            ends_by_ms: 2,
            child: Some(BackgroundChild {
                session: sid(),
                agent_id: s(),
            }),
        }),
        EventKind::BackgroundEnded(BackgroundEnded {
            bg_id: s(),
            reason: BackgroundReason::StoppedByAgent,
            exit_code: Some(1),
            signal: Some(2),
            confirmed: BackgroundConfirmed::Unconfirmed,
            ran_ms: 3,
            output_bytes: 4,
            tail: s(),
            tail_cut: true,
            delivered: true,
            child: Some(Box::new(ChildEnded {
                session: sid(),
                agent_id: s(),
                result: s(),
                ok: true,
                usage: Some(usage(w)),
            })),
        }),
        EventKind::CallbackRefused(CallbackRefused {
            bg_ids: vec![s()],
            reason: s(),
        }),
        EventKind::QueuedDelivered(QueuedDelivered {
            turn_id: s(),
            seqs: vec![1, 2],
        }),
        EventKind::TodoUpdated(TodoUpdated {
            turn_id: s(),
            call_id: s(),
            items: todo(w),
        }),
        EventKind::ApprovalModeChanged(ApprovalModeChanged {
            mode: ApprovalMode::AcceptEdits,
            requested: Some(ApprovalMode::Auto),
        }),
        EventKind::ContextSent(ContextSent {
            turn_id: s(),
            frame: ContextFrame::Callback,
            sections: vec![SentSection {
                key: s(),
                digest: s(),
                text: s(),
                note: Some(s()),
            }],
        }),
        EventKind::ChatAllowAdded(ChatAllowAdded {
            grant_id: s(),
            approval_id: s(),
            tool: s(),
            input: free_json(w),
            key: s(),
        }),
        EventKind::ChatAllowRemoved(ChatAllowRemoved { grant_id: s() }),
        EventKind::StepLimitChanged(StepLimitChanged {
            max_steps: 1,
            requested: Some(-2),
        }),
        EventKind::HandoffWritten(HandoffWritten {
            through_seq: 1,
            text: Some(s()),
            model: Some(s()),
            failed: vec![NoteFailure {
                model: s(),
                reason: s(),
            }],
            usage: Some(usage(w)),
        }),
        EventKind::CompactFailed(CompactFailed {
            reason: s(),
            usage: Some(usage(w)),
        }),
        EventKind::ImagesWithheld(ImagesWithheld {
            turn_id: s(),
            calls: vec![s()],
            status: 400,
            reason: s(),
        }),
        EventKind::ThinkingWithheld(ThinkingWithheld {
            turn_id: s(),
            model: s(),
            status: 400,
            reason: s(),
        }),
    ]
}

/// One slot per kind. No wildcard arm: a new kind does not compile until
/// it is listed here, and then fails [`every_kind_is_built`] until
/// [`every_kind`] builds it and [`FREE`] says what of it is free.
fn slot(k: &EventKind) -> usize {
    match k {
        EventKind::SessionCreated(_) => 0,
        EventKind::TurnStarted(_) => 1,
        EventKind::UserMessage(_) => 2,
        EventKind::AssistantDelta(_) => 3,
        EventKind::AssistantMessage(_) => 4,
        EventKind::ToolCallStarted(_) => 5,
        EventKind::ToolOutputDelta(_) => 6,
        EventKind::ToolCallFinished(_) => 7,
        EventKind::ApprovalRequested(_) => 8,
        EventKind::ApprovalResolved(_) => 9,
        EventKind::Checkpoint(_) => 10,
        EventKind::TurnEnded(_) => 11,
        EventKind::Error(_) => 12,
        EventKind::AgentChanged(_) => 13,
        EventKind::VariantSelected(_) => 14,
        EventKind::HistorySummarized(_) => 15,
        EventKind::ContextShortened(_) => 16,
        EventKind::ModelFallback(_) => 17,
        EventKind::ModelRetry(_) => 18,
        EventKind::RetryGivenUp(_) => 19,
        EventKind::ArtifactStaged(_) => 20,
        EventKind::ArtifactStageFailed(_) => 21,
        EventKind::BackgroundStarted(_) => 22,
        EventKind::BackgroundEnded(_) => 23,
        EventKind::CallbackRefused(_) => 24,
        EventKind::QueuedDelivered(_) => 25,
        EventKind::TodoUpdated(_) => 26,
        EventKind::ApprovalModeChanged(_) => 27,
        EventKind::ContextSent(_) => 28,
        EventKind::ChatAllowAdded(_) => 29,
        EventKind::ChatAllowRemoved(_) => 30,
        EventKind::StepLimitChanged(_) => 31,
        EventKind::HandoffWritten(_) => 32,
        EventKind::CompactFailed(_) => 33,
        EventKind::ImagesWithheld(_) => 34,
        EventKind::ThinkingWithheld(_) => 35,
    }
}
const KINDS: usize = 36;

/// The free paths of each kind, written by hand (the oracle; the scrubber
/// itself has no such table). `*` is any array index, a trailing `**`
/// everything below (keys included), a trailing `{}` a map's keys. Every other position is schema: field
/// names, tags, enum values, numbers, ids and the values the harness writes.
const FREE: &[(&str, &[&str])] = &[
    ("session_created", &["title", "backend", "node_id"]),
    (
        "turn_started",
        &[
            "agent.name",
            "agent.prompt_hash",
            "agent.withheld.*",
            "agent.effort",
            "environment",
            "continued_by",
            "retry.error",
            "retry.failed_model",
            "retry.model",
        ],
    ),
    (
        "user_message",
        &[
            "text",
            "attachments.*.name",
            "attachments.*.mime",
            "attachments.*.text",
            "files.*.path",
            "files.*.text",
            "files.*.note",
            "files.*.seen.path",
            "reply.block_id",
            "reply.values.**",
            "images.*.mime",
            "images.*.name",
        ],
    ),
    ("assistant_delta", &["text", "author", "variant.model"]),
    (
        "assistant_message",
        &[
            "text",
            "author",
            "variant.model",
            "blocks.*.hash",
            "tool_calls.*.call_id",
            "tool_calls.*.tool",
            "tool_calls.*.input.**",
            "blocks.*.id",
            "blocks.*.spec.**",
            "blocks.*.error",
            "thinking.blocks.*.text",
            "thinking.blocks.*.signature",
            "thinking.blocks.*.data",
            "thinking.prefix",
        ],
    ),
    (
        "tool_call_started",
        &["call_id", "tool", "input.**", "author"],
    ),
    ("tool_output_delta", &["call_id", "text"]),
    (
        "tool_call_finished",
        &[
            "call_id",
            "output",
            "sources.*.id",
            "sources.*.url",
            "sources.*.title",
            "note",
            "seen.path",
            "image.mime",
        ],
    ),
    (
        "approval_requested",
        &["call_id", "tool", "input.**", "reason"],
    ),
    ("approval_resolved", &["note"]),
    ("checkpoint", &["files.*", "excluded.*"]),
    ("turn_ended", &[]),
    ("error", &["message"]),
    (
        "agent_changed",
        &[
            "agent.name",
            "agent.prompt_hash",
            "agent.withheld.*",
            "agent.effort",
        ],
    ),
    ("variant_selected", &[]),
    ("history_summarized", &["text", "focus"]),
    (
        "context_shortened",
        &["folded.text", "folded.todo.*.content"],
    ),
    ("model_fallback", &["model", "from", "reason"]),
    ("model_retry", &["model"]),
    ("retry_given_up", &["model"]),
    (
        "artifact_staged",
        &[
            "call_id",
            "name",
            "filename",
            "packed",
            "kind_hint",
            "source_path",
            "notes",
            "meta.toolchain",
            "meta.package.name",
            "meta.package.version",
            "meta.git.commit",
            "meta.git.branch",
            "meta.target",
            "meta.entries.*.path",
            "meta.skipped.{}",
            "meta.source_path",
        ],
    ),
    ("artifact_stage_failed", &["path", "name", "error"]),
    ("background_started", &["call_id", "child.agent_id"]),
    (
        "background_ended",
        &["tail", "child.agent_id", "child.result"],
    ),
    ("callback_refused", &["reason"]),
    ("queued_delivered", &[]),
    ("todo_updated", &["call_id", "items.*.content"]),
    ("approval_mode_changed", &[]),
    // `sections.*.key` holds `approval_mode` here, not one of the
    // harness's own keys: scrubbed.
    (
        "context_sent",
        &["sections.*.key", "sections.*.text", "sections.*.note"],
    ),
    ("chat_allow_added", &["tool", "input.**"]),
    ("chat_allow_removed", &[]),
    ("step_limit_changed", &[]),
    (
        "handoff_written",
        &["text", "model", "failed.*.model", "failed.*.reason"],
    ),
    ("compact_failed", &["reason"]),
    ("images_withheld", &["calls.*", "reason"]),
    ("thinking_withheld", &["reason"]),
];

/// `path` (segments; an array index is `*`) is free in `kind`. `deeper`:
/// how many segments a trailing `**` must cover at least (1 for a key: the
/// field holding the free JSON is itself schema).
fn free(kind: &str, path: &[String], deeper: usize) -> bool {
    let (_, pats) = FREE
        .iter()
        .find(|(k, _)| *k == kind)
        .unwrap_or_else(|| panic!("FREE has no entry for {kind}"));
    pats.iter().any(|p| {
        let segs: Vec<&str> = p.split('.').collect();
        match segs.split_last() {
            Some((&"{}", head)) => {
                deeper == 1
                    && path.len() == head.len() + 1
                    && head.iter().zip(path).all(|(a, b)| *a == b.as_str())
            }
            Some((&"**", head)) => {
                path.len() >= head.len() + deeper
                    && head.iter().zip(path).all(|(a, b)| *a == b.as_str())
            }
            // A key is free only inside free JSON.
            _ => {
                deeper == 0
                    && segs.len() == path.len()
                    && segs.iter().zip(path).all(|(a, b)| *a == b.as_str())
            }
        }
    })
}

/// The payload as the scrubber must leave it: free strings and the keys of
/// free JSON scrubbed, everything else as it was.
fn oracle(set: &SecretSet, kind: &str, v: &Value, path: &mut Vec<String>) -> Value {
    match v {
        Value::String(s) if free(kind, path, 0) => Value::String(set.scrub(s)),
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| {
                    path.push("*".into());
                    let out = oracle(set, kind, x, path);
                    path.pop();
                    out
                })
                .collect(),
        ),
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, x) in m {
                path.push(k.clone());
                let key = if free(kind, path, 1) {
                    set.scrub(k)
                } else {
                    k.clone()
                };
                out.insert(key, oracle(set, kind, x, path));
                path.pop();
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Every key and string of 8 or more bytes in `v`.
fn harvest(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.len() >= 8 => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| harvest(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                if k.len() >= 8 {
                    out.push(k.clone());
                }
                harvest(x, out);
            }
        }
        _ => {}
    }
}

/// Enum values of the schema the constructors above do not all use (the
/// list in b-scrubber/NOTES.md, from the type definitions).
const ENUM_WORDS: &[&str] = &[
    "accept_edits",
    "access_revoked",
    "agentic",
    "allow_chat",
    "allow_session",
    "before_restore",
    "callback",
    "cancelled",
    "completed",
    "finished",
    "in_progress",
    "interrupted",
    "network",
    "no_answer",
    "node_restarted",
    "node_stopping",
    "overloaded",
    "progress",
    "rate_limit",
    "redacted",
    "restored",
    "sandbox",
    "server_error",
    "session_deleted",
    "still_alive",
    "stopped",
    "stopped_by_agent",
    "stopped_by_user",
    "timed_out",
    "too_large",
    "unconfirmed",
];

/// Every schema word of 8 or more bytes: each kind's tag, every key and
/// every string of every kind as built here (field names, enum values,
/// the session id), and [`ENUM_WORDS`].
fn schema_words() -> Vec<String> {
    let mut out: Vec<String> = ENUM_WORDS.iter().map(|s| s.to_string()).collect();
    for k in every_kind("approval_mode") {
        out.push(k.name().into());
        let v = serde_json::to_value(&k).unwrap();
        harvest(&v, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn set_of(words: &[String]) -> SecretSet {
    SecretSet::new(words.iter().map(|w| Secret::new(w.clone())).collect())
}

#[test]
fn every_kind_is_built() {
    let mut seen = [false; KINDS];
    for k in every_kind("x") {
        seen[slot(&k)] = true;
        // Each kind's free table exists, and every kind round-trips: the
        // scrubber's check relies on it.
        let (kind, payload) = k.clone().into_parts();
        assert!(FREE.iter().any(|(n, _)| *n == kind), "{kind}");
        assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
    }
    assert!(seen.iter().all(|s| *s), "kinds not built: {seen:?}");
    assert_eq!(FREE.len(), KINDS);
}

/// The reviewer's finding, kind by kind: a secret set holding every
/// schema word of 8 or more bytes (field names, tags, enum values, the
/// words the harness writes) leaves every kind decodable and unchanged
/// outside its free text, and still scrubs that free text where the word
/// is in it (it is the value of every string here).
#[test]
fn every_kind_keeps_its_schema_under_every_schema_word() {
    let words = schema_words();
    for must in [
        "approval_mode",
        "message_id",
        "assistant_message",
        "tool_call_finished",
        "session_created",
        "by_model",
        "accept_edits",
        "in_progress",
    ] {
        assert!(words.iter().any(|w| w == must), "{must} not harvested");
    }
    let set = set_of(&words);
    for k in every_kind("approval_mode") {
        let (kind, payload) = k.into_parts();
        let mut got = payload.clone();
        // Never by the fallback: every class learned is right for it.
        let out = set.scrub_event(&kind, &mut got);
        assert!(
            matches!(out, Outcome::Scrubbed | Outcome::Clean),
            "{kind}: {out:?}"
        );
        let want = oracle(&set, &kind, &payload, &mut vec![]);
        assert_eq!(got, want, "{kind}");
        let back = EventKind::from_parts(&kind, got).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(back.name(), kind);
    }
}

/// A real secret, wherever outside text can put it (model text and
/// thinking, a tool's input as a value and as a key, its output, an MCP
/// result, a provider's error, a sub-agent's report, an attachment), is
/// gone from every free position, and only the harness's own positions
/// keep what the harness wrote there.
#[test]
fn a_real_secret_is_gone_from_every_free_position() {
    let set = set_of(&[CANARY.to_string()]);
    let w = format!("before {CANARY} after");
    for k in every_kind(&w) {
        let (kind, payload) = k.into_parts();
        let mut got = payload.clone();
        // Never by the fallback: every class learned is right for it.
        let out = set.scrub_event(&kind, &mut got);
        assert!(
            matches!(out, Outcome::Scrubbed | Outcome::Clean),
            "{kind}: {out:?}"
        );
        assert_eq!(got, oracle(&set, &kind, &payload, &mut vec![]), "{kind}");
    }
    // The tool input with the secret as a key and as a value.
    let mut input = serde_json::Map::new();
    input.insert(CANARY.into(), json!(CANARY));
    input.insert("model".into(), json!(format!("x{CANARY}")));
    let (kind, mut p) = EventKind::ToolCallStarted(ToolCallStarted {
        turn_id: "t1".into(),
        call_id: "c1".into(),
        tool: "mcp__x__echo".into(),
        input: Value::Object(input),
        author: None,
    })
    .into_parts();
    set.scrub_event(&kind, &mut p);
    assert!(!p.to_string().contains(CANARY), "{p}");
    assert_eq!(p["input"]["[REDACTED]"], "[REDACTED]");
    assert_eq!(p["input"]["model"], "x[REDACTED]");
}

/// A schema word that is also a secret is removed from free text and left
/// in the schema: `session_created` keeps `approval_mode`, the text loses it.
#[test]
fn a_schema_word_secret_leaves_the_schema_and_not_the_text() {
    let set = set_of(&["approval_mode".into(), "accept_edits".into()]);
    let (kind, mut p) = EventKind::SessionCreated(SessionCreated {
        title: "set approval_mode to accept_edits".into(),
        cwd: "/w".into(),
        model: "mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: ApprovalMode::AcceptEdits,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
        no_callbacks: false,
        exec_mode: None,
    })
    .into_parts();
    set.scrub_event(&kind, &mut p);
    assert_eq!(p["approval_mode"], "accept_edits", "{p}");
    assert_eq!(p["title"], "set [REDACTED] to [REDACTED]");
    assert!(EventKind::from_parts(&kind, p).is_ok());
}

/// A configured value that is the chat's folder, its model or an id the
/// harness wrote does not rewrite them (an env `ROOT=/home/bob/proj`).
#[test]
fn harness_written_values_are_kept() {
    let set = set_of(&["/home/bob/proj".into(), "deepseek-chat".into()]);
    let (kind, mut p) = EventKind::SessionCreated(SessionCreated {
        title: "work in /home/bob/proj/src".into(),
        cwd: "/home/bob/proj".into(),
        model: "deepseek-chat".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: ApprovalMode::Ask,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
        no_callbacks: false,
        exec_mode: None,
    })
    .into_parts();
    set.scrub_event(&kind, &mut p);
    assert_eq!(p["cwd"], "/home/bob/proj");
    assert_eq!(p["model"], "deepseek-chat");
    assert_eq!(p["title"], "work in [REDACTED]/src");
}

fn fixture_events() -> Vec<RawEvent> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/session.json");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    serde_json::from_value(doc["events"].clone()).unwrap()
}

/// An event with nothing to scrub is stored byte for byte as it was.
#[test]
fn nothing_to_scrub_changes_nothing() {
    let set = set_of(&["zz-nowhere-in-the-fixture-zz".into()]);
    for e in fixture_events() {
        let mut p = e.payload.clone();
        set.scrub_event(&e.kind, &mut p);
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&e.payload).unwrap()
        );
    }
}

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// Random secret sets cut from the fold fixture's own text (keys, tags,
/// ids, enum values, pieces of each) never leave an event that decoded
/// undecodable or of another kind, and the log still folds.
#[test]
fn random_secret_sets_never_break_the_fixture() {
    let events = fixture_events();
    let text: Vec<String> = events
        .iter()
        .map(|e| {
            let mut s = e.kind.clone();
            s.push_str(&e.payload.to_string());
            s
        })
        .collect();
    let mut r = Lcg(0x5eed);
    for _ in 0..300 {
        let mut words = vec![];
        for _ in 0..1 + r.below(6) {
            let t = &text[r.below(text.len())];
            let len = 8 + r.below(17);
            if t.len() <= len {
                continue;
            }
            let mut at = r.below(t.len() - len);
            while !t.is_char_boundary(at) {
                at -= 1;
            }
            let mut end = at + len;
            while !t.is_char_boundary(end) {
                end += 1;
            }
            words.push(t[at..end].to_string());
        }
        let set = set_of(&words);
        let mut out = vec![];
        for e in &events {
            let before = e.typed();
            // The fixture holds events as a browser gets them; some of those
            // no longer decode, and only decodable ones are ever scrubbed.
            if before.is_err() && e.kind != "from_a_newer_node" {
                out.push(e.clone());
                continue;
            }
            let mut p = e.payload.clone();
            let how = set.scrub_event(&e.kind, &mut p);
            assert!(
                !matches!(how, Outcome::Fallback { .. }),
                "{words:?} {}: {how:?}",
                e.kind
            );
            let after = EventKind::from_parts(&e.kind, p.clone());
            if let Ok(b) = before {
                let a = after.unwrap_or_else(|err| panic!("{words:?} broke {}: {err}", e.kind));
                assert_eq!(a.name(), b.name());
            }
            out.push(RawEvent {
                payload: p,
                ..e.clone()
            });
        }
        SessionView::fold(&out).unwrap_or_else(|err| panic!("{words:?}: {err}"));
    }
}

/// A kind this build does not know (a newer node's) has no schema to keep:
/// every string value is scrubbed, no key is touched (a key may be its
/// schema).
#[test]
fn an_unknown_kind_has_its_values_scrubbed_and_its_keys_kept() {
    let set = set_of(&[CANARY.to_string()]);
    let mut p = json!({ CANARY: { "turn_id": CANARY, "list": [format!("a{CANARY}")] } });
    assert_eq!(
        set.scrub_event("from_a_newer_node", &mut p),
        Outcome::Unknown
    );
    assert_eq!(
        p,
        json!({ CANARY: { "turn_id": "[REDACTED]", "list": ["a[REDACTED]"] } })
    );
}

/// The allowlist names only positions that exist and are text (or a map's
/// keys), in kinds that exist; every kind has its entry. A position not on
/// it is scrubbed (the oracle of [`every_kind_keeps_its_schema_under_every_schema_word`]
/// holds the other half: every text position not kept is free).
#[test]
fn the_allowlist_names_real_text_positions_of_every_kind() {
    assert_eq!(KEPT.len(), KINDS);
    for k in every_kind("approval_mode") {
        let (kind, payload) = k.into_parts();
        let (_, kept) = KEPT
            .iter()
            .find(|(n, _)| *n == kind)
            .unwrap_or_else(|| panic!("KEPT has no entry for {kind}"));
        let pos = positions(&kind, &payload);
        let text: Vec<String> = pos
            .iter()
            .filter_map(|(g, c)| match c {
                Class::Text => Some(g.clone()),
                Class::Map => Some(format!("{g}.{{}}")),
                _ => None,
            })
            .collect();
        for g in *kept {
            assert!(
                text.iter().any(|t| t == g),
                "{kind}: KEPT names {g}, which is not a text position: {text:?}"
            );
        }
    }
}

/// The classification is per path, learned once: it rests on a path
/// having one class in every element and every instance. Checked here
/// over every kind built with several fillers and every event of the fold
/// fixture, each position probed by itself.
#[test]
fn a_path_has_one_class_in_every_instance() {
    let mut seen: BTreeMap<(String, String), (Class, String)> = BTreeMap::new();
    let mut instances: Vec<(String, Value, String)> = vec![];
    for w in [
        "approval_mode",
        CANARY,
        "x",
        "01a19bf2-0000-7000-8000-00000000f1f0",
    ] {
        for k in every_kind(w) {
            let (kind, p) = k.into_parts();
            instances.push((kind, p, format!("built with {w}")));
        }
    }
    for e in fixture_events() {
        if let Ok(k) = e.typed() {
            let (kind, p) = k.into_parts();
            instances.push((kind, p, format!("fixture seq {}", e.seq)));
        }
    }
    for (kind, p, from) in &instances {
        for (g, c) in positions(kind, p) {
            match seen.get(&(kind.clone(), g.clone())) {
                Some((was, at)) => {
                    assert_eq!(*was, c, "{kind} {g}: {was:?} in {at}, {c:?} in {from}")
                }
                None => {
                    seen.insert((kind.clone(), g), (c, from.clone()));
                }
            }
        }
    }
    assert!(seen.len() > 300, "{}", seen.len());
}

/// `continued_by` is a call id the provider or model minted: scrubbed as
/// every call id is, so the two still match.
#[test]
fn a_call_id_and_its_continued_by_scrub_alike() {
    let set = set_of(&[CANARY.to_string()]);
    let id = format!("call_{CANARY}");
    let (k1, mut a) = EventKind::TurnStarted(TurnStarted {
        turn_id: "t".into(),
        agent: None,
        cause: None,
        environment: None,
        continued_by: Some(id.clone()),
        retry: None,
    })
    .into_parts();
    let (k2, mut b) = EventKind::ToolCallStarted(ToolCallStarted {
        turn_id: "t".into(),
        call_id: id,
        tool: "delegate".into(),
        input: json!({}),
        author: None,
    })
    .into_parts();
    set.scrub_event(&k1, &mut a);
    set.scrub_event(&k2, &mut b);
    assert_eq!(a["continued_by"], b["call_id"]);
    assert_eq!(a["continued_by"], "call_[REDACTED]");
}

fn sections(keys: &[&str]) -> EventKind {
    EventKind::ContextSent(ContextSent {
        turn_id: "t".into(),
        frame: ContextFrame::Person,
        sections: keys
            .iter()
            .map(|k| SentSection {
                key: k.to_string(),
                digest: "ab".into(),
                text: "x".into(),
                note: None,
            })
            .collect(),
    })
}

/// A section's key: the harness's own keys as they are, even when a
/// configured value equals one; a subfolder file's key keeps its prefix
/// and has its path scrubbed; other text is scrubbed. The same in every
/// turn, so what the history holds still compares.
#[test]
fn section_keys_keep_the_harness_keys_and_scrub_the_rest() {
    let set = set_of(&[
        "instructions".into(),
        "workspace".into(),
        "instructions:".into(),
        CANARY.into(),
    ]);
    let nested = format!("instructions:{CANARY}/AGENTS.md");
    let other = format!("x {CANARY}");
    let keys = [
        "instructions",
        "workspace",
        "memory_recall",
        "instructions:web/AGENTS.md",
        nested.as_str(),
        other.as_str(),
    ];
    let mut out = vec![];
    for _turn in 0..2 {
        let (kind, mut p) = sections(&keys).into_parts();
        set.scrub_event(&kind, &mut p);
        let got: Vec<String> = p["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["key"].as_str().unwrap().to_string())
            .collect();
        out.push(got);
    }
    assert_eq!(out[0], out[1]);
    assert_eq!(
        out[0],
        [
            "instructions",
            "workspace",
            "memory_recall",
            "instructions:web/AGENTS.md",
            "instructions:[REDACTED]/AGENTS.md",
            "x [REDACTED]",
        ]
    );
}

/// Usage by model reads as `usage.model` does, and two models whose names
/// are secrets stay two (R5 of the review).
#[test]
fn usage_by_model_keeps_its_models_apart() {
    let a = "aaaaaaaaaaaaKEY1";
    let b = "bbbbbbbbbbbbKEY2";
    let set = set_of(&[a.into(), b.into()]);
    let mut u = usage(a);
    u.by_model.insert(b.into(), u.by_model[a].clone());
    let (kind, mut p) = EventKind::TurnEnded(TurnEnded {
        turn_id: "t".into(),
        reason: TurnEndReason::Completed,
        usage: Some(u),
    })
    .into_parts();
    set.scrub_event(&kind, &mut p);
    let by = p["usage"]["by_model"].as_object().unwrap();
    assert_eq!(by.len(), 2, "{p}");
    assert!(
        by.contains_key(p["usage"]["model"].as_str().unwrap()),
        "{p}"
    );
}

/// Two keys of a tool's JSON that scrub to the same text both stay.
#[test]
fn colliding_free_keys_lose_nothing() {
    let a = "aaaaaaaaaaaaKEY1";
    let b = "bbbbbbbbbbbbKEY2";
    let set = set_of(&[a.into(), b.into()]);
    let (kind, mut p) = EventKind::ToolCallStarted(ToolCallStarted {
        turn_id: "t".into(),
        call_id: "c".into(),
        tool: "x".into(),
        input: json!({ a: 1, b: 2 }),
        author: None,
    })
    .into_parts();
    set.scrub_event(&kind, &mut p);
    let input = p["input"].as_object().unwrap();
    assert_eq!(input.len(), 2, "{p}");
    assert!(!p.to_string().contains("KEY"), "{p}");
}

/// A configured value inside the marker is never redacted, so scrubbing
/// twice gives what once did (R3 of the review).
#[test]
fn scrubbing_twice_is_scrubbing_once() {
    for w in ["REDACTED", "[REDACTED]", "EDACTED]", "[REDACTE"] {
        let set = set_of(&[w.to_string(), CANARY.into()]);
        let (kind, mut once) = EventKind::UserMessage(UserMessage {
            turn_id: "t".into(),
            text: format!("x {w} y {CANARY}"),
            attachments: vec![],
            files: vec![],
            reply: None,
            images: vec![],
            queued: false,
        })
        .into_parts();
        set.scrub_event(&kind, &mut once);
        let mut twice = once.clone();
        set.scrub_event(&kind, &mut twice);
        assert_eq!(once, twice, "{w}");
        assert_eq!(once["text"], format!("x {w} y [REDACTED]"), "{w}");
    }
}

/// A payload that decodes but is not as the typed event writes it (a
/// defaulted field left out) is stored as the typed event writes it, and
/// its free text is still scrubbed.
#[test]
fn a_payload_not_in_its_written_form_is_still_scrubbed() {
    let set = set_of(&[CANARY.into()]);
    let mut p = json!({ "turn_id": "t", "message_id": "m", "text": format!("say {CANARY}") });
    let out = set.scrub_event("assistant_message", &mut p);
    assert_eq!(out, Outcome::Scrubbed);
    assert_eq!(p["text"], "say [REDACTED]");
    assert!(EventKind::from_parts("assistant_message", p).is_ok());
}
