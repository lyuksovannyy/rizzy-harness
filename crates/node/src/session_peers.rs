//! A person's message that called several agents by mention (ADOPTION_PLAN
//! X.37, README "Calling several agents"). The node drives the order: the
//! master resolved every agent and sent them with the prompt
//! (`session.prompt.peers`); the first agent answers the message's own turn,
//! and when that turn ends the node starts a turn for each of the others,
//! one after another, each under its own spec, each reading the chat so far
//! (the earlier agents' answers included). Never two turns at once: the
//! next one is claimed through the same turn slot as any turn, after the
//! last one released it.
//!
//! What is kept, and where. The specs of the turns that have not started
//! are held in memory only ([`Peers`]): a node restart drops them, and the
//! node says so once when it comes back (`peers_skipped`, from the log
//! alone: [`harness_core::mention::unasked`]). Nothing a model wrote starts
//! one of these turns: only a person's `session.prompt`, which the master
//! sends for the chat's owner.
//!
//! The rules:
//! - A turn that ends `completed`, `limit` or with an `error` is followed
//!   by the next agent's. Stop (`Cancelled`) ends that turn and the rest of
//!   the list, and `peers_skipped` names the agents that were not asked.
//!   A node stop or restart ends it too; the restart says so.
//! - A message the person sends while the list runs follows the queue's
//!   rule for the RUNNING turn (X.5): the turn reads it at its next step or
//!   it waits. The turns of the list do not read it at their start: it is
//!   read by the turn that follows the last agent's, as the chat's last
//!   agent. A message that itself calls several agents waits until the list
//!   is over (refused with the queue's sentence). In the instant between
//!   two turns the next agent's turn comes first: a message arriving then
//!   is refused, not run in between.
//! - A mention turn is an ordinary turn for approvals, modes, Undo,
//!   checkpoints, compaction and cost, and the chat's step limit, with no
//!   master context and no connector, memory or board tool (it is no send
//!   of the master's).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness_core::agent::AgentSpec;
use harness_core::backend::TurnExtra;
use harness_core::event::{
    CalledBy, EventKind, MentionCall, PeersSkipReason, PeersSkipped, TurnEndReason, TurnStarted,
};
use harness_core::new_id;
use tokio_util::sync::CancellationToken;

use super::{RunningTurn, Session};

/// The agents of a message still to be asked, in order. In memory only.
pub(crate) struct Peers {
    /// The seq of the person's `user_message` that called them.
    pub message_seq: u64,
    pub rest: VecDeque<AgentSpec>,
    /// The message's tags: each later agent's opening names the ones it
    /// has among its delegates.
    pub tags: Vec<String>,
    /// The person pressed Stop: nothing more of the list starts.
    pub stopped: bool,
}

/// The tags of a prompt, checked: plain handles, each once, at most
/// [`harness_core::mention::MAX_RESOLVED`].
pub(crate) fn checked_tags(tags: Vec<String>) -> Result<Vec<String>, String> {
    if tags.len() > harness_core::mention::MAX_RESOLVED {
        return Err("a message mentions at most 16 agents and names".into());
    }
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        if !harness_core::mention::valid_handle(&t) || out.contains(&t) {
            return Err("a tag is an agent's plain handle, once".into());
        }
        out.push(t);
    }
    Ok(out)
}

/// The tags `agent` has among its own delegates, in the message's order
/// (after the node's bounds on delegation: nothing is offered that its
/// definition or this node does not allow).
pub(crate) fn allowed_tags(agent: &AgentSpec, tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|t| agent.delegates.iter().any(|d| &d.agent_id == *t))
        .cloned()
        .collect()
}

/// The refusal of a message while the list's turns run.
pub(crate) const PEERS_PENDING: &str = "the agents the last message called are still taking their turns; send this when they have answered";

impl Session {
    /// The specs a prompt's `peers` ask for, checked against the node's own
    /// bounds: at most [`harness_protocol::MAX_PEERS`], each agent once
    /// (the first included), no connector tool (their turns are no send of
    /// the master's), delegation as `[limits]` allow.
    pub(crate) fn checked_peers(
        &self,
        first: Option<&AgentSpec>,
        peers: Vec<AgentSpec>,
    ) -> Result<Vec<AgentSpec>, String> {
        if peers.is_empty() {
            return Ok(peers);
        }
        let Some(first) = first else {
            return Err("a message that calls several agents names the first".into());
        };
        if peers.len() > harness_protocol::MAX_PEERS {
            return Err(format!(
                "at most {} agents may be called by one message",
                harness_core::mention::MAX_CALLED
            ));
        }
        let mut seen = vec![first.agent_id.clone()];
        let limits = &self.node.cfg.policy.limits;
        let mut out = Vec::new();
        for p in peers {
            if seen.contains(&p.agent_id) {
                return Err("an agent is called once per message".into());
            }
            seen.push(p.agent_id.clone());
            let mut p = limits.bound_delegation(p);
            p.connector_tools.clear();
            out.push(p);
        }
        Ok(out)
    }

    /// Whether agents of a message wait for their turn.
    pub(crate) fn peers_pending(&self) -> bool {
        self.peers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|p| !p.rest.is_empty())
    }

    /// Keep the agents after the first of the message recorded at
    /// `message_seq`.
    pub(crate) fn set_peers(&self, message_seq: u64, rest: Vec<AgentSpec>, tags: Vec<String>) {
        *self.peers.lock().unwrap_or_else(|p| p.into_inner()) = Some(Peers {
            message_seq,
            rest: rest.into(),
            tags,
            stopped: false,
        });
    }

    /// The person pressed Stop: nothing more of the list starts. Called
    /// with the turn's own cancel (`Session::cancel`), also between two
    /// turns.
    pub(crate) fn stop_peers(&self) {
        if let Some(p) = self
            .peers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
        {
            p.stopped = true;
        }
    }

    /// The seq of the `user_message` of `turn_id` recorded after `after`.
    pub(crate) async fn message_seq(&self, after: u64, turn_id: &str) -> Option<u64> {
        let id = self.id;
        let evs = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, after, 16))
            .await
            .ok()?;
        evs.iter().find_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) if m.turn_id == turn_id && !m.queued => Some(e.seq),
            _ => None,
        })
    }

    /// Say which agents were not asked, and drop the list.
    async fn skip_peers(&self, message_seq: u64, agents: Vec<String>, reason: PeersSkipReason) {
        if agents.is_empty() {
            return;
        }
        if let Err(e) = self
            .append(EventKind::PeersSkipped(PeersSkipped {
                message_seq,
                agents,
                reason,
            }))
            .await
        {
            tracing::warn!(session = %self.id, error = %e, "could not record the agents that were not asked");
        }
    }

    /// After the turn that ended with `reason`: ask the next agent of the
    /// message's list, while there is one and the turns end without the
    /// person's Stop; then the turn that reads the messages sent meanwhile
    /// (X.5) with the agent that answered last. Replaces
    /// [`Session::read_queued`] at the end of a person's turn.
    pub(crate) async fn finish_turn(
        self: Arc<Self>,
        mut reason: TurnEndReason,
        mut agent: Option<AgentSpec>,
        after: TurnExtra,
        mut ended: String,
    ) {
        loop {
            // The slot is free: a test holds the gap open here.
            #[cfg(feature = "testing")]
            crate::testing::peers_hold(self.id).await;
            let Some((seq, next, tags)) = self.next_peer(reason).await else {
                break;
            };
            let started = self.start_mention(seq, next.clone(), tags).await;
            let (turn_id, user_text, history, spec, extra, cancel, by_stop) = match started {
                Ok(s) => s,
                Err(why) => {
                    // The node's own reason (its log, a restore, its stop):
                    // the person is told who was not asked.
                    let mut rest: Vec<String> = vec![next.agent_id.clone()];
                    if let Some(p) = self.peers.lock().unwrap_or_else(|p| p.into_inner()).take() {
                        rest.extend(p.rest.into_iter().map(|s| s.agent_id));
                    }
                    tracing::warn!(session = %self.id, error = %why, "could not start the next called agent's turn");
                    if !self.node.is_stopping() {
                        self.skip_peers(seq, rest, PeersSkipReason::Unavailable)
                            .await;
                    }
                    break;
                }
            };
            ended = turn_id.clone();
            let extra = TurnExtra {
                savings: after.savings.clone(),
                ..extra
            };
            reason = self
                .clone()
                .run_turn(
                    turn_id,
                    user_text,
                    history,
                    Some(spec.clone()),
                    vec![],
                    extra,
                    cancel,
                    by_stop,
                )
                .await
                .0;
            agent = Some(spec);
        }
        self.keep_reader(agent.clone(), after.clone());
        self.read_queued(reason, agent, after, ended).await;
    }

    /// The next agent to ask (with the message's seq), or `None` when the
    /// list is over: empty, or ended by `reason`. Records the note for an
    /// agent that will not be asked.
    async fn next_peer(&self, reason: TurnEndReason) -> Option<(u64, AgentSpec, Vec<String>)> {
        let mut skipped: Option<(u64, Vec<String>, PeersSkipReason)> = None;
        let next = {
            let mut g = self.peers.lock().unwrap_or_else(|p| p.into_inner());
            let p = g.as_mut()?;
            let ids = |p: &Peers| {
                p.rest
                    .iter()
                    .map(|s| s.agent_id.clone())
                    .collect::<Vec<_>>()
            };
            match reason {
                // The node is going down: the restart's note says who was
                // not asked, from the log (nothing can be relied on to be
                // written now).
                TurnEndReason::Interrupted | TurnEndReason::Stopped => {
                    *g = None;
                    None
                }
                _ if p.stopped || reason == TurnEndReason::Cancelled => {
                    skipped = Some((p.message_seq, ids(p), PeersSkipReason::Stopped));
                    *g = None;
                    None
                }
                _ => match p.rest.pop_front() {
                    Some(s) => Some((p.message_seq, s, p.tags.clone())),
                    None => {
                        *g = None;
                        None
                    }
                },
            }
        };
        if let Some((seq, agents, why)) = skipped {
            self.skip_peers(seq, agents, why).await;
        }
        next
    }

    /// Claim the slot and record the start of `spec`'s turn for the message
    /// at `message_seq`. Everything the turn runs with comes from the log
    /// and the node's own bounds.
    #[allow(clippy::type_complexity)]
    async fn start_mention(
        self: &Arc<Self>,
        message_seq: u64,
        spec: AgentSpec,
        tags: Vec<String>,
    ) -> Result<
        (
            String,
            String,
            Vec<harness_core::RawEvent>,
            AgentSpec,
            TurnExtra,
            CancellationToken,
            Arc<AtomicBool>,
        ),
        String,
    > {
        self.check_cwd_still_allowed()?;
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        let queue = self.queue.lock().await;
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: Default::default(),
            context: None,
            standing: None,
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
            agent_id: Some(spec.agent_id.clone()),
            callback: false,
            compare: false,
            began: std::time::SystemTime::now(),
            delegated_by: None,
        })?;
        // A Stop between the last turn and this claim: it found no turn to
        // cancel, so it left its flag.
        let stopped = self
            .peers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|p| p.stopped);
        if stopped {
            self.release(&turn_id);
            return Err("stopped".into());
        }
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        self.record_clamp().await;
        let gate = self.bg_gate.lock().await;
        let id = self.id;
        let history = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(h) => h,
            Err(e) => {
                drop(gate);
                self.release(&turn_id);
                return Err(e.to_string());
            }
        };
        // The message must be the one the list was kept for.
        let asked = history.iter().find_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) if e.seq == message_seq => Some(m.text),
            _ => None,
        });
        let Some(asked) = asked else {
            drop(gate);
            self.release(&turn_id);
            return Err("the message that called this agent is not in the log".into());
        };
        let limits = &self.node.cfg.policy.limits;
        let mut spec = limits.bound_delegation(spec);
        spec.connector_tools.clear();
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let environment = self.environment_line(&turn_id).await;
        let cause = harness_core::event::TurnCause::Mention(MentionCall {
            message_seq,
            by: CalledBy::Person,
            tags: allowed_tags(&spec, &tags),
        });
        let started = self
            .emit(EventKind::TurnStarted(TurnStarted {
                turn_id: turn_id.clone(),
                agent: Some(spec.agent_ref()),
                cause: Some(cause.clone()),
                environment,
                continued_by: None,
                retry: None,
            }))
            .await;
        drop(gate);
        if let Err(e) = started {
            self.release(&turn_id);
            return Err(e.to_string());
        }
        drop(queue);
        let extra = TurnExtra {
            cause: Some(cause),
            ..Default::default()
        };
        Ok((turn_id, asked, history, spec, extra, cancel, by_stop))
    }

    /// After a restart: the agents a message called that never had their
    /// turn (their specs were kept in memory only), said once.
    pub(crate) async fn recover_peers(&self) {
        if self.parent_session.is_some() {
            return;
        }
        let id = self.id;
        let Ok(history) = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        else {
            return;
        };
        if let Some((seq, agents)) = harness_core::mention::unasked(&history) {
            self.skip_peers(seq, agents, PeersSkipReason::Restarted)
                .await;
        }
    }
}
