//! A person's Retry of the chat's failed last turn (ADOPTION_PLAN X.21,
//! README "Retry"). The node decides, against its own log, under the
//! turn slot: only the last turn, one the person started, that ended
//! `error`, `interrupted` or `stopped` with nothing after it that changes
//! what it was (a person's message, an undo of its changes). The retry is
//! a new turn of the person's: `turn_started` names the failed turn
//! (`retry`), and no `user_message` is written, so the message stays in the
//! log once. The failed attempt's steps stay in the history the model is
//! sent, followed by the harness's line saying they happened
//! (`retry_opening`): a command it ran or a file it wrote is not repeated
//! blindly. Nothing here can be started by a model, a tool or a routine:
//! only `session.retry`, which the master sends for the chat's owner.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness_core::agent::AgentSpec;
use harness_core::backend::TurnExtra;
use harness_core::event::{EventKind, RetryOf, TurnEndReason, TurnStarted};
use harness_core::new_id;
use harness_core::retry::check as retry_check;
use harness_protocol::RETRY_REFUSED;
use tokio_util::sync::CancellationToken;

use super::{RunningTurn, Session};

impl Session {
    /// Run turn `failed` again, as its chat's owner asked (`session.retry`):
    /// the same message, with the agent, context and savings the master
    /// resolved now, and this node's own bounds. Refused while a turn or
    /// `/compact` runs (no queueing: a second click while a retry runs is
    /// refused), on a stopping node, in a sub-agent's chat, and for any
    /// turn `harness_core::retry::check` refuses. Acked once `turn_started` is written.
    pub(crate) async fn retry(
        self: &Arc<Self>,
        failed: &str,
        agent: Option<AgentSpec>,
        agent_named: bool,
        extra: TurnExtra,
    ) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err(format!(
                "{RETRY_REFUSED}: a sub-agent's turn is retried by the agent that delegated it"
            ));
        }
        self.check_cwd_still_allowed()?;
        let limits = &self.node.cfg.policy.limits;
        let agent = agent.map(|a| limits.bound_delegation(a));
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        let queue = self.queue.lock().await;
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: extra.savings.clone(),
            context: extra.context.clone(),
            standing: extra.standing.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
            agent_id: agent.as_ref().map(|a| a.agent_id.clone()),
            callback: false,
            compare: false,
            began: std::time::SystemTime::now(),
        })?;
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // A failed turn whose end could not be written is closed first:
        // then it is the failed turn this retries.
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        // As for any turn (S3-4): no background end lands between the read
        // and `turn_started`.
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
        let retry = match retry_check(&history, failed) {
            // The model it runs on, as `run_turn` picks it.
            Ok(r) => RetryOf {
                model: Some(
                    agent
                        .as_ref()
                        .and_then(|a| a.model.clone())
                        .unwrap_or_else(|| self.model.clone()),
                ),
                ..r
            },
            Err(e) => {
                drop(gate);
                self.release(&turn_id);
                return Err(format!("{RETRY_REFUSED}: {e}"));
            }
        };
        // The owner acts in their chat: background commands may start
        // again after a revocation closed the table, as for a prompt.
        if let Some(bg) = &self.bg {
            bg.reopen();
        }
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let environment = self.environment_line(&turn_id).await;
        // A switch since the failed turn, above the retry: it runs under
        // the agent the chat has now (T69).
        if let Some(switch) = self.switch_of(&history, agent.as_ref(), agent_named)
            && let Err(e) = self.emit(EventKind::AgentChanged(switch)).await
        {
            drop(gate);
            self.release(&turn_id);
            return Err(e.to_string());
        }
        let started = self
            .emit(EventKind::TurnStarted(TurnStarted {
                turn_id: turn_id.clone(),
                agent: agent.as_ref().map(AgentSpec::agent_ref),
                cause: None,
                environment,
                retry: Some(retry.clone()),
                continued_by: None,
            }))
            .await;
        drop(gate);
        if let Err(e) = started {
            self.release(&turn_id);
            return Err(e.to_string());
        }
        drop(queue);
        // Messages sent while the failed turn ran that it never read wait
        // for the turn after this one, as after a callback turn (ST-2).
        let after = TurnExtra {
            savings: extra.savings.clone(),
            ..Default::default()
        };
        let extra = TurnExtra {
            retry: Some(retry),
            ..extra
        };
        // The message's own text, as the person's words of this turn: a
        // `/skill` at its start loads as it did for the failed turn, the
        // in-turn fold's summariser is told what was asked, and a link in
        // it is the person's (review of PR 128). It is not sent again: the
        // opening is the harness's line, the message is in the history.
        let asked = harness_core::retry::message(&history, failed).unwrap_or_default();
        let this = self.clone();
        tokio::spawn(async move {
            let (reason, _) = this
                .clone()
                .run_turn(
                    turn_id.clone(),
                    asked,
                    history,
                    agent.clone(),
                    vec![],
                    extra,
                    cancel,
                    by_stop,
                )
                .await;
            this.keep_reader(agent.clone(), after.clone());
            this.read_queued(reason, agent, after, turn_id).await;
        });
        Ok(())
    }
}
