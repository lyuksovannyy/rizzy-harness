//! A session's `push` (ADOPTION_PLAN X.27 step 4, PLAN.md T101,
//! `docs/forge-push.md` §5): who may, the card every time, the token from
//! the master only after the person's Allow, the push from the node's own
//! repository ([`crate::forge_push`]).
//!
//! **Which turns.** The master names a target only in a project chat's own
//! turn that its owner sent signed in (never a routine run, an API-token
//! or webhook message, a callback turn, a temporary or a forked chat). The
//! node keeps it only for a chat of its own that is not a sub-agent's or a
//! routine's, in a turn that is not a callback, on a node whose node.toml
//! pushes to that host ([`Session::push_for_turn`]); otherwise the tool is
//! not offered. A turn that only reads queued messages gets none.
//!
//! **Order.** Before the card: the branch's commit, read from the folder's
//! refs as files, and its subject, read from a first node repository of
//! links (no network). The card. After the person's Allow: the ref is read
//! again and must still name the commit the card showed; the objects are
//! listed and linked again; only then is the token asked for, with the
//! card's approval id, which the master checks against the Allow it
//! relayed itself.

use std::sync::Arc;
use std::time::Duration;

use harness_core::Secret;
use harness_core::event::{ApprovalRequested, EventKind, OUTPUT_QUOTE, ResolvedBy, quoted_line};
use harness_core::forge::{PUSH_TOOL, PushTarget};
use harness_core::policy::Decision;
use harness_protocol::NodeToMaster;
use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::Session;
use crate::forge_push;

/// How long a push waits for the master's credential.
pub const WAIT: Duration = Duration::from_secs(20);

/// Credentials waited for at once on the node.
const MAX_PENDING: usize = 8;

type Reply = oneshot::Sender<Result<(String, Secret<String>), String>>;

/// Pushes waiting for their `forge.push_credential_result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(std::sync::Mutex<std::collections::HashMap<String, Reply>>);

impl Pending {
    /// An answer arrived: hand it to its waiting push. An unknown id (late,
    /// after the wait gave up, or never asked) is dropped with its token.
    pub(crate) fn answer(
        &self,
        req_id: &str,
        username: Option<String>,
        token: Option<Secret<String>>,
        error: Option<String>,
    ) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match (username, token, error) {
                (_, _, Some(e)) => Err(e),
                (Some(u), Some(t), None) => Ok((u, t)),
                _ => Err("the server sent no credential".into()),
            });
        }
    }
}

/// Removes a waiting request however it ends.
struct Waiting<'a>(&'a Pending, String);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.1);
    }
}

/// The session's `push`, as its tools reach it.
pub(crate) struct LinkPush(pub(crate) Arc<Session>);

#[async_trait::async_trait]
impl harness_core::forge::PushHost for LinkPush {
    async fn push(
        &self,
        call_id: &str,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<String, String> {
        run(&self.0, call_id, input, cancel).await
    }
}

impl Session {
    /// The master's `target` for turn `turn_id`, kept only where this
    /// node and this session allow a push in it.
    pub(super) fn push_for_turn(
        &self,
        turn_id: &str,
        target: Option<PushTarget>,
    ) -> Option<PushTarget> {
        let target = target?;
        let callback = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|t| t.turn_id == turn_id && t.callback);
        let policy = &self.node.cfg.policy;
        let ok = self.profile == harness_core::event::Profile::Agentic
            && self.parent_session.is_none()
            && !self.no_callbacks
            && !self.incognito
            && !callback
            && policy.projects.staging.is_some()
            && policy.forge.allows_push(&target.host)
            && crate::policy::valid_forge_host(&target.host)
            && crate::forge_clone::valid_full_name(&target.full_name)
            && crate::forge_clone::valid_username(&target.login)
            && !policy.denies(PUSH_TOOL);
        ok.then_some(target)
    }

    /// The running turn's target, if this very turn has one.
    fn push_target_now(&self) -> Option<PushTarget> {
        let open = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| t.turn_id.clone())?;
        match &*self.push_now.lock().unwrap_or_else(|p| p.into_inner()) {
            Some((t, target)) if *t == open => Some(target.clone()),
            _ => None,
        }
    }

    /// Where this session's `push` runs, when the running turn may push.
    pub(super) fn push_link(&self) -> Option<Arc<dyn harness_core::forge::PushHost>> {
        self.push_target_now()?;
        let me = self.node.session(self.id)?;
        Some(Arc::new(LinkPush(me)))
    }

    /// A card for a call of an External tool (`push`): every time, in every
    /// mode but plan (which refuses), with no "for this turn" or "for this
    /// chat" offered and none taken; the node's `denied_tools` refuses it.
    /// `Ok(approval_id)` only for a person's Allow (any allow answer counts
    /// as this one call); `Err` with the text for the model otherwise.
    pub(super) async fn ask_external(
        &self,
        call_id: &str,
        tool: &str,
        input: &Value,
        reason: String,
        preview: String,
    ) -> Result<String, String> {
        if self.node.cfg.policy.denies(tool) || self.current_mode().refuses(tool) {
            return Err(format!("`{tool}` is not allowed here"));
        }
        let approval_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(approval_id.clone(), tx);
        let requested = self
            .emit(EventKind::ApprovalRequested(ApprovalRequested {
                approval_id: approval_id.clone(),
                call_id: call_id.into(),
                tool: tool.into(),
                input: input.clone(),
                reason,
                turn_allow: false,
                chat_allow: false,
                preview: Some(preview),
            }))
            .await;
        if requested.is_err() {
            self.approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&approval_id);
            return Err("the card could not be recorded; nothing was pushed".into());
        }
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(approval_id.clone());
        let cancel = self.current_cancel().unwrap_or_default();
        let (decision, by, note) = tokio::select! {
            biased;
            _ = cancel.cancelled() => (Decision::Deny, ResolvedBy::Cancelled, None),
            r = rx => match r {
                Ok((d, note)) => (d, ResolvedBy::User, note),
                Err(_) => (Decision::Deny, ResolvedBy::Cancelled, None),
            },
        };
        // Any allow answer is this one call (none other was offered).
        let decision = if decision.allows() {
            Decision::Allow
        } else {
            decision
        };
        let plan_denied = self
            .plan_denied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        let (decision, by, note) =
            if plan_denied || (decision.allows() && self.current_mode().refuses(tool)) {
                (
                    Decision::Deny,
                    ResolvedBy::Policy,
                    Some(super::PLAN_CARD_DENIED.to_string()),
                )
            } else {
                (decision, by, note)
            };
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        if !self
            .resolve(approval_id.clone(), decision, by, note.clone())
            .await
        {
            return Err("the answer could not be recorded; nothing was pushed".into());
        }
        if decision.allows() && by == ResolvedBy::User {
            return Ok(approval_id);
        }
        Err(match (by, note) {
            (ResolvedBy::Cancelled, _) => "[cancelled before approval]".into(),
            (ResolvedBy::User, n) => harness_core::event::denied_text(n.as_deref()),
            (_, Some(n)) => n,
            (_, None) => "not allowed".into(),
        })
    }
}

/// The text for the model: `[harness]` lines, the forge's words quoted.
fn said(head: &str, forge: &str) -> String {
    let mut out = format!("[harness] {head}");
    if !forge.is_empty() {
        out.push_str("\n[harness] the forge answered:");
        for l in forge.lines() {
            out.push('\n');
            out.push_str(OUTPUT_QUOTE);
            out.push_str(&quoted_line(l));
        }
    }
    out
}

/// Ask the master for the credential of approval `approval_id`.
async fn credential(s: &Session, approval_id: String) -> Result<(String, Secret<String>), String> {
    let node = &s.node;
    if node.outbox().is_none() {
        return Err("this node is not connected to the server; nothing was pushed".into());
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut p = node.pushes.0.lock().unwrap_or_else(|p| p.into_inner());
        if p.len() >= MAX_PENDING {
            return Err("too many pushes are waiting for the server; try again".into());
        }
        p.insert(req_id.clone(), tx);
    }
    let _waiting = Waiting(&node.pushes, req_id.clone());
    let send = node.publish_wait(NodeToMaster::PushCredential {
        req_id,
        session_id: s.id,
        approval_id,
    });
    let asked = async {
        send.await;
        rx.await
    };
    match tokio::time::timeout(WAIT, asked).await {
        Ok(Ok(r)) => r.map_err(|e| {
            format!(
                "the server gave no credential: {}; nothing was pushed",
                harness_core::text::clean_line(&e, 300)
            )
        }),
        Ok(Err(_)) => Err("the server did not answer; nothing was pushed".into()),
        Err(_) => Err(format!(
            "the server did not answer within {} seconds; nothing was pushed",
            WAIT.as_secs()
        )),
    }
}

/// One `push` call of session `s`.
pub(crate) async fn run(
    s: &Arc<Session>,
    call_id: &str,
    input: &Value,
    cancel: CancellationToken,
) -> Result<String, String> {
    let Some(target) = s.push_target_now() else {
        return Err(harness_tools::push::NOT_HERE.into());
    };
    let policy = s.node.cfg.policy.clone();
    if !policy.forge.allows_push(&target.host) {
        return Err("this node does not push to that host ([forge] push, [forge] hosts)".into());
    }
    let staging = policy
        .projects
        .staging
        .clone()
        .ok_or("this node has no staging folder ([projects] dir)")?;
    let branch = harness_tools::push::branch_arg(input)?;
    let folder = s.checked_folder()?;
    let top = match crate::git::user_repo::find(&folder, Some(&folder)) {
        crate::git::user_repo::Found::Repo(r) if r.top == folder => r.top,
        _ => {
            return Err(
                "this chat's folder is not a plain git repository at its top \
                 (a .git directory); nothing was pushed"
                    .into(),
            );
        }
    };
    // One at a time on this node.
    let Ok(_slot) = s.node.push_slots.clone().try_acquire_owned() else {
        return Err("another push is running on this node; try again when it has finished".into());
    };
    let bounds = forge_push::Bounds::of(&policy);
    let jail = s.node.cfg.git_sandbox.clone();
    let blocking = |f: Box<dyn FnOnce() -> Result<forge_push::Staged, String> + Send>| async move {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| format!("reading the folder: {e}"))?
    };
    let pin1 = {
        let (t, b) = (top.clone(), branch.clone());
        tokio::task::spawn_blocking(move || forge_push::pin(&t, b.as_deref()))
            .await
            .map_err(|e| format!("reading the folder: {e}"))??
    };
    let format = if pin1.commit.len() == 64 {
        "sha256"
    } else {
        "sha1"
    };
    // Before the card: the subject, from a first repository of links.
    let subject = {
        let (st, t, b) = (staging.clone(), top.clone(), bounds.clone());
        let staged = blocking(Box::new(move || forge_push::stage(&st, &t, format, &b))).await?;
        forge_push::subject(&staged, &pin1.commit, jail.as_deref()).await?
    };
    let short = &pin1.commit[..12];
    let reason = format!(
        "push branch `{}` to {}/{} as {}",
        pin1.branch, target.host, target.full_name, target.login
    );
    let preview = format!(
        "repository: {}/{}\nbranch: {}\nas: {}\ncommit: {} {}",
        target.host, target.full_name, pin1.branch, target.login, short, subject
    );
    let approval_id = s
        .ask_external(call_id, PUSH_TOOL, input, reason, preview)
        .await?;
    if cancel.is_cancelled() {
        return Err("cancelled; nothing was pushed".into());
    }
    // After the Allow: the turn still the same, the ref still the pin.
    if s.push_target_now().as_ref() != Some(&target) {
        return Err("the turn that asked has ended; nothing was pushed".into());
    }
    let pin2 = {
        let (t, b) = (top.clone(), pin1.branch.clone());
        tokio::task::spawn_blocking(move || forge_push::pin(&t, Some(&b)))
            .await
            .map_err(|e| format!("reading the folder: {e}"))??
    };
    if pin2.commit != pin1.commit {
        return Err(format!(
            "the branch `{}` moved after the card was shown (it showed {}, it is now {}); \
             nothing was pushed: call push again to show the new commit",
            pin1.branch,
            short,
            &pin2.commit[..12.min(pin2.commit.len())]
        ));
    }
    let staged = {
        let (st, t, b) = (staging.clone(), top.clone(), bounds.clone());
        blocking(Box::new(move || forge_push::stage(&st, &t, format, &b))).await?
    };
    forge_push::subject(&staged, &pin1.commit, jail.as_deref()).await?;
    let (login, token) = credential(s, approval_id).await?;
    let result = forge_push::push(
        &staged,
        jail.as_deref(),
        &target.host,
        &target.full_name,
        &pin1.branch,
        &pin1.commit,
        &login,
        &token,
        &bounds,
    )
    .await;
    drop(token);
    let secrets = &s.node.cfg.secrets;
    let mut detail = serde_json::json!({
        "session_id": s.id,
        "host": target.host,
        "full_name": target.full_name,
        "branch": pin1.branch,
        "commit": pin1.commit,
        "linked": staged.linked,
        "copied": staged.copied,
    });
    let out = match result {
        Ok(p) => {
            let summary = secrets.scrub(&p.summary);
            detail["ok"] = (p.flag != '!').into();
            detail["result"] = summary.clone().into();
            let head = match p.flag {
                '!' => format!(
                    "the forge refused the push of {short} to branch `{}` of {}/{}; nothing \
                     changed there",
                    pin1.branch, target.host, target.full_name
                ),
                '=' => format!(
                    "branch `{}` of {}/{} already is {short}; nothing was sent",
                    pin1.branch, target.host, target.full_name
                ),
                '*' => format!(
                    "pushed {short} to the new branch `{}` of {}/{} as {login}",
                    pin1.branch, target.host, target.full_name
                ),
                _ => format!(
                    "pushed {short} to branch `{}` of {}/{} as {login}",
                    pin1.branch, target.host, target.full_name
                ),
            };
            if p.flag == '!' {
                Err(said(&head, &summary))
            } else {
                Ok(said(&head, &summary))
            }
        }
        Err(e) => {
            let e = secrets.scrub(&e);
            detail["ok"] = false.into();
            detail["error"] = e.clone().into();
            Err(e)
        }
    };
    s.node.audit_log.record("forge.push", detail.to_string());
    out
}
