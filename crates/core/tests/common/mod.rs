//! Every event kind, built in full: every optional part present, every
//! `String` set to one word. Shared by the scrubber's schema tests
//! (`scrub_event.rs`) and the browser's schema list (`browser_schema.rs`).
//! No wildcard arm in [`slot`]: a new kind does not compile until it is
//! listed there, and struct literals name every field, so a new field does
//! not compile until it is built here.
#![allow(dead_code)]

use std::collections::BTreeMap;

use harness_core::SessionId;
use harness_core::artifacts::{ArtifactMeta, Entry, GitInfo, Package, StagedBy};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::todo::{TodoItem, TodoStatus};
use serde_json::{Value, json};

pub fn sid() -> SessionId {
    "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap()
}

/// What a tool's input, a ui spec or a form's values hold: model-written
/// JSON. Its keys are free too, even one named like a harness field.
pub fn free_json(w: &str) -> Value {
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

pub fn usage(w: &str) -> Usage {
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

pub fn agent(w: &str) -> AgentRef {
    AgentRef {
        agent_id: w.into(),
        name: w.into(),
        prompt_hash: w.into(),
        withheld: vec![w.into()],
        effort: Some(w.into()),
    }
}

pub fn seen(w: &str) -> FileSeen {
    FileSeen {
        path: w.into(),
        sha256: w.into(),
        base: Some(w.into()),
    }
}

pub fn todo(w: &str) -> Vec<TodoItem> {
    vec![TodoItem {
        content: w.into(),
        status: TodoStatus::InProgress,
    }]
}

/// Every kind, every optional part present, every `String` set to `w`.
pub fn every_kind(w: &str) -> Vec<EventKind> {
    let s = || w.to_string();
    vec![
        EventKind::SessionCreated(SessionCreated {
            forked: Some(ForkedFrom {
                parent: sid(),
                through_seq: 5,
            }),
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
            sub_folder: Some(harness_core::subagent::SubFolder::Own),
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
            triggers: vec![s()],
            tags: vec![s()],
            uploads: vec![UploadedFile {
                name: s(),
                path: s(),
                bytes: 1,
                sha256: s(),
                mime: s(),
            }],
            attached: vec![AttachedFile {
                file_id: s(),
                name: s(),
                bytes: 1,
                sha256: s(),
                mime: s(),
            }],
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
            made: None,
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: s(),
            call_id: s(),
            tool: s(),
            input: free_json(w),
            reason: s(),
            turn_allow: true,
            chat_allow: true,
            preview: Some(s()),
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
            head_moved: Some(HeadMoved {
                from: HeadAt {
                    branch: Some("main".into()),
                    commit: "a".repeat(40),
                },
                to: HeadAt {
                    branch: Some("x".into()),
                    commit: "b".repeat(40),
                },
            }),
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: s(),
            reason: TurnEndReason::Completed,
            usage: Some(usage(w)),
        }),
        EventKind::TurnUsage(TurnUsage {
            turn_id: s(),
            usage: usage(w),
        }),
        EventKind::Error(ErrorEvent {
            notice: true,
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
        EventKind::ExecModeChanged(ExecModeChanged {
            mode: ExecMode::Host,
            from: ExecMode::Sandbox,
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
        EventKind::PeersSkipped(PeersSkipped {
            message_seq: 1,
            agents: vec![s()],
            reason: PeersSkipReason::Restarted,
        }),
        EventKind::ConversationRewound(ConversationRewound {
            message_seq: 2,
            turn_id: s(),
            files: true,
        }),
        EventKind::ConversationRewindUndone(ConversationRewindUndone { rewind_seq: 3 }),
        EventKind::MessagePinned(MessagePinned {
            message_seq: 2,
            pinned: true,
        }),
        EventKind::SessionForked(SessionForked {
            parent: sid(),
            through_seq: 5,
            turn_id: s(),
        }),
    ]
}

/// Values of a kind whose shape has alternatives (an enum with data): the
/// ones [`every_kind`] does not build. The browser's schema takes a path
/// found only in some of a kind's values as optional.
pub fn alternatives(w: &str) -> Vec<EventKind> {
    let s = || w.to_string();
    vec![EventKind::TurnStarted(TurnStarted {
        turn_id: s(),
        agent: Some(agent(w)),
        cause: Some(TurnCause::Mention(MentionCall {
            message_seq: 1,
            by: CalledBy::Person,
            tags: vec![s()],
        })),
        environment: Some(s()),
        continued_by: Some(s()),
        retry: Some(RetryOf {
            turn_id: s(),
            reason: TurnEndReason::Interrupted,
            error: Some(s()),
            failed_model: Some(s()),
            model: Some(s()),
        }),
    })]
}

/// One slot per kind. No wildcard arm: a new kind does not compile until
/// it is listed here, and then fails [`every_kind_is_built`] until
/// [`every_kind`] builds it and [`FREE`] says what of it is free.
pub fn slot(k: &EventKind) -> usize {
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
        EventKind::PeersSkipped(_) => 36,
        EventKind::TurnUsage(_) => 37,
        EventKind::ConversationRewound(_) => 38,
        EventKind::ConversationRewindUndone(_) => 39,
        EventKind::SessionForked(_) => 40,
        EventKind::MessagePinned(_) => 41,
        EventKind::ExecModeChanged(_) => 42,
    }
}
pub const KINDS: usize = 43;
