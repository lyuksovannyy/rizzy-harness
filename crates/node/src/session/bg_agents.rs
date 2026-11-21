//! Background sub-agents (ADOPTION_PLAN X.24, PLAN.md T72): `delegate`
//! with `background: true`. The child is the same child session a
//! foreground `delegate` makes (its own folder when it can change files,
//! this chat's folder when it only reads), started the same way; only who
//! waits differs. Its run is held as an entry of this chat's background
//! table (`harness_tools::background::SessionBg`), beside the background
//! commands, so everything that ends or lists those ends and lists it too:
//! `bash_stop`, the person's Stop and Stop all, the owner's revocation,
//! the chat's deletion, the node's stop, `bash_output`'s list, an undo's
//! refusal while one runs. Its end goes through the commands' one path:
//! recorded by this session (its one writer) at a step boundary of a
//! running turn or between turns, where its changes are merged back and
//! its result is built, then a callback turn by the master's rules (T.8d
//! step 3). Bounds are node.toml's: `[limits] background_agents_per_chat`
//! running at once, `background_agent_timeout_s` of wall clock each, the
//! node's step cap and the token budget left in the turn that started it.

use super::*;
use harness_core::backend::BackgroundDelegated;
use harness_tools::background::{Ended, SessionBg};

/// A background sub-agent whose run ended and whose end is not recorded.
pub(super) enum ChildEnd {
    /// Its run ended: its merge-back and its result wait for the end's
    /// delivery.
    Ran(Box<RanChild>),
    /// Merged and built once; kept while the log does not take it.
    Ready(Box<BackgroundEnded>),
}

pub(super) struct RanChild {
    child: Arc<Session>,
    iso: Option<Isolation>,
    shared: bool,
    turn_id: String,
    reason: TurnEndReason,
    usage: Option<Usage>,
    agent_id: String,
}

impl Session {
    /// Whether this chat may start background sub-agents now, or why not
    /// (the model reads the reason). The node's rules only; the agent's
    /// own switch is the loop's to read.
    pub(super) fn may_start_background(&self) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err(
                "a sub-agent cannot start sub-agents in the background (delegation is one \
                        level deep): do the work yourself, or report what is left"
                    .into(),
            );
        }
        if self.profile != Profile::Agentic {
            return Err("background sub-agents run only in a chat on a node".into());
        }
        if self.no_callbacks {
            return Err(
                "this chat takes no callback turns (a routine's run), so a background \
                        sub-agent's result would reach nobody: delegate in the foreground"
                    .into(),
            );
        }
        // No refusal by execution mode: a `host` chat gets no limit a
        // `sandbox` chat does not have (the owner, 2026-11-20: "dont, like if
        // user selected host project workspace, why do we need to limit it?
        // its his choice"). The child runs in its parent's mode (`start_child`
        // copies `self.exec_mode`; nothing in a call names one).
        let Some(bg) = &self.bg else {
            return Err("this chat has no background table: delegate in the foreground".into());
        };
        if self.node.cfg.policy.limits.background_agents_per_chat == 0 {
            return Err("background sub-agents are off on this node ([limits] \
                        background_agents_per_chat = 0 in its node.toml): delegate in the \
                        foreground"
                .into());
        }
        if bg.is_closed() {
            return Err(
                "this chat's background work was stopped (its owner lost access); none \
                        starts again until its owner sends it a message"
                    .into(),
            );
        }
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        Ok(())
    }

    /// Start `task` (or, with `follow_up`, the next message to that
    /// sub-agent of this chat) in the background and return at once.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_background(
        &self,
        agent: AgentSpec,
        task: String,
        follow_up: Option<SessionId>,
        call_id: String,
        observed: Arc<Vec<String>>,
        max_tokens: u64,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<BackgroundDelegated, String> {
        self.may_start_background()?;
        let bg = self.bg.clone().ok_or("this chat has no background table")?;
        let me = self
            .node
            .session(self.id)
            .ok_or("this chat is being deleted")?;
        // Admitted before anything is made; given back if the start fails.
        let admitted = bg.reserve_agent()?;
        // Its own token: the turn that started it ends without it.
        let cancel = CancellationToken::new();
        // Its own citation book: its labels cannot be this chat's, whose
        // next turn numbers from its own log.
        let refs: SharedRefs = Arc::default();
        let started: Box<dyn FnOnce(SessionId) + Send> = Box::new(|_| {});
        let p = match follow_up {
            None => {
                self.start_child(
                    agent.clone(),
                    task.clone(),
                    refs,
                    observed,
                    cancel,
                    started,
                    ChildRun::Background { max_tokens },
                    folder,
                )
                .await?
            }
            Some(c) => {
                self.start_continue(
                    c,
                    agent.clone(),
                    task.clone(),
                    call_id,
                    refs,
                    observed,
                    cancel,
                    started,
                    ChildRun::Background { max_tokens },
                )
                .await?
            }
        };
        let timeout = self.node.cfg.policy.limits.agent_timeout();
        let (num, st, stop) = bg.adopt_agent(admitted, &agent.agent_id, &task, timeout);
        let child = p.child.id;
        tracing::info!(session = %self.id, child = %child, bg = %st.bg_id, agent = %agent.agent_id, "a background sub-agent started");
        tokio::spawn(run_in_background(
            Arc::downgrade(&me),
            Arc::downgrade(&bg),
            num,
            st.bg_id.clone(),
            p,
            stop,
            timeout,
        ));
        Ok(BackgroundDelegated {
            child,
            bg_id: st.bg_id,
            timeout_s: st.timeout_s,
            ends_by_ms: st.ends_by_ms,
        })
    }

    /// The background entry of `child` whose end waits to be recorded.
    pub(super) fn undelivered_child(&self, child: SessionId) -> Option<String> {
        self.bg_children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|(_, e)| match e {
                ChildEnd::Ran(r) => r.child.id == child,
                ChildEnd::Ready(b) => b.child.as_ref().is_some_and(|c| c.session == child),
            })
            .map(|(id, _)| id.clone())
    }

    /// The event for end `e`. A background sub-agent's is built here, once:
    /// its changes merged back into this chat's folder (the foreground's
    /// three-way merge and its report), its result read from its own log
    /// and rendered as a foreground `delegate` result.
    pub(super) async fn end_event(&self, e: &Ended, delivered: bool) -> BackgroundEnded {
        let mut ev = e.event(delivered);
        let entry = self
            .bg_children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&e.bg_id);
        match entry {
            None => ev,
            Some(ChildEnd::Ready(mut b)) => {
                b.delivered = delivered;
                *b
            }
            Some(ChildEnd::Ran(r)) => {
                let r = *r;
                let o = self
                    .child_outcome(&r.child, r.iso, r.shared, &r.turn_id, r.reason, r.usage)
                    .await;
                use harness_core::subagent::{ChildResult, ChildStatus, render, status_of};
                let status = status_of(o.finish.as_ref(), o.reason);
                let ok = o.reason == TurnEndReason::Completed
                    && status != ChildStatus::Failed
                    && o.applied;
                let (result, _) = render(&ChildResult {
                    child: o.child,
                    agent_id: &r.agent_id,
                    reason: o.reason,
                    finish: o.finish.as_ref(),
                    text: &o.text,
                    error: o.error.as_deref(),
                    note: o.note.as_deref(),
                    unapplied: o.unapplied.as_deref(),
                    applied: o.applied,
                });
                ev.child = Some(Box::new(ChildEnded {
                    session: o.child,
                    agent_id: r.agent_id,
                    result: self.node.cfg.secrets.scrub(&result),
                    ok,
                    usage: o.usage,
                }));
                ev
            }
        }
    }

    /// An end the log did not take: a background sub-agent's stays built
    /// (it was merged already) for the next try.
    pub(super) fn keep_child_end(&self, ev: BackgroundEnded) {
        if ev.child.is_some() {
            self.bg_children
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(ev.bg_id.clone(), ChildEnd::Ready(Box::new(ev)));
        }
    }

    /// Every background sub-agent end not recorded yet goes (its chat is
    /// being deleted): their folders too, unmerged.
    pub(crate) async fn drop_child_ends(&self) {
        let ids: Vec<String> = self
            .bg_children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.drop_child_end(&id).await;
        }
    }

    /// A background sub-agent's end that is not recorded (its chat is being
    /// deleted, or its start was never recorded): its folder goes,
    /// unmerged.
    pub(super) async fn drop_child_end(&self, bg_id: &str) {
        let entry = self
            .bg_children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(bg_id);
        if let Some(ChildEnd::Ran(r)) = entry
            && let Some(i) = r.iso
        {
            crate::git::remove_folder(&i.child).await;
        }
    }
}

/// The task that owns one background sub-agent's run: its turn, its
/// wall-clock limit and its stop. It holds its chat and table weakly: a
/// chat deleted meanwhile stops it through the table.
async fn run_in_background(
    parent: std::sync::Weak<Session>,
    bg: std::sync::Weak<SessionBg>,
    num: u64,
    bg_id: String,
    p: Prepared,
    mut stop: oneshot::Receiver<BackgroundReason>,
    timeout: std::time::Duration,
) {
    let began = std::time::Instant::now();
    let Prepared {
        child,
        iso,
        shared,
        msg,
        history,
        agent,
        extra,
        cancel,
        by_stop,
    } = p;
    let agent_id = agent.agent_id.clone();
    let turn_id = msg.turn_id.clone();
    let run = child.clone().run_turn(
        msg.turn_id.clone(),
        msg.model_text(),
        history,
        Some(agent),
        vec![],
        extra,
        cancel.clone(),
        by_stop,
    );
    tokio::pin!(run);
    let mut why: Option<BackgroundReason> = None;
    let (reason, usage) = tokio::select! {
        r = &mut run => r,
        _ = tokio::time::sleep(timeout) => {
            why = Some(BackgroundReason::TimedOut);
            cancel.cancel();
            run.await
        }
        // A closed channel: the table went with its chat.
        r = &mut stop => {
            why = Some(r.unwrap_or(BackgroundReason::SessionDeleted));
            cancel.cancel();
            run.await
        }
    };
    let parent = parent.upgrade();
    // The node's stop cancels every running turn itself; such an end is
    // the node's, not the sub-agent's own finish (no callback for it).
    let reason_now = match why {
        Some(r) => r,
        None if reason != TurnEndReason::Completed
            && parent.as_ref().is_some_and(|p| p.node.is_stopping()) =>
        {
            BackgroundReason::NodeStopping
        }
        None => BackgroundReason::Finished,
    };
    let (Some(parent), Some(bg)) = (parent, bg.upgrade()) else {
        // Its chat is gone: nothing records or merges it.
        if let Some(i) = iso {
            crate::git::remove_folder(&i.child).await;
        }
        return;
    };
    tracing::info!(session = %parent.id, child = %child.id, bg = %bg_id, reason = ?reason_now, "a background sub-agent ended");
    parent
        .bg_children
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(
            bg_id.clone(),
            ChildEnd::Ran(Box::new(RanChild {
                child,
                iso,
                shared,
                turn_id,
                reason,
                usage,
                agent_id,
            })),
        );
    let sent = bg.agent_ended(
        num,
        Ended {
            bg_id: bg_id.clone(),
            reason: reason_now,
            exit_code: None,
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: began.elapsed().as_millis() as u64,
            output_bytes: 0,
            tail: String::new(),
            tail_cut: false,
        },
    );
    if !sent {
        // Its start was never recorded, or the chat's recorder is gone.
        parent.drop_child_end(&bg_id).await;
    }
}
