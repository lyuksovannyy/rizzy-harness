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

    async fn open_pull_request(
        &self,
        call_id: &str,
        input: &Value,
        cancel: CancellationToken,
    ) -> Result<String, String> {
        run_pr(&self.0, call_id, input, cancel).await
    }
}

/// How long a pull request waits for the master (two forge calls at most,
/// each bounded there).
pub const PR_WAIT: Duration = Duration::from_secs(60);

/// What the master answered a `forge.pull_request`.
pub(crate) struct PrAnswer {
    pub number: Option<u64>,
    pub url: Option<String>,
    pub existed: bool,
}

type PrReply = oneshot::Sender<Result<PrAnswer, String>>;

/// Pull requests waiting for their `forge.pull_request_result`.
#[derive(Default)]
pub(crate) struct PrPending(std::sync::Mutex<std::collections::HashMap<String, PrReply>>);

impl PrPending {
    pub(crate) fn answer(
        &self,
        req_id: &str,
        number: Option<u64>,
        url: Option<String>,
        existed: bool,
        error: Option<String>,
    ) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match error {
                Some(e) => Err(e),
                None => Ok(PrAnswer {
                    number,
                    url,
                    existed,
                }),
            });
        }
    }
}

/// One `pull_request` call of session `s`: the card with the call's own
/// input (the page draws the same event the master reads), then, after a
/// person's Allow, one request to the master naming only the approval.
pub(crate) async fn run_pr(
    s: &Arc<Session>,
    call_id: &str,
    input: &Value,
    cancel: CancellationToken,
) -> Result<String, String> {
    use harness_core::forge::{PR_TOOL, parse_pr};
    let Some(target) = s.push_target_now() else {
        return Err(harness_tools::pull_request::NOT_HERE.into());
    };
    if !target.pull_requests {
        return Err(harness_tools::pull_request::NOT_HERE.into());
    }
    if s.node.cfg.policy.denies(PR_TOOL) || !s.node.cfg.policy.forge.push {
        return Err("this node does not open pull requests ([forge] push, denied_tools)".into());
    }
    let pr = parse_pr(input)?;
    let reason = format!(
        "open a pull request in {}/{} as {}",
        target.host, target.full_name, target.login
    );
    // No preview of the node's own: the page draws the call's input itself
    // (head, base, title, body, draft), the very event the master reads.
    let _ = pr;
    let approval_id = s
        .ask_external(call_id, PR_TOOL, input, reason, String::new())
        .await?;
    if cancel.is_cancelled() {
        return Err("cancelled; nothing was opened".into());
    }
    if s.push_target_now().as_ref() != Some(&target) {
        return Err("the turn that asked has ended; nothing was opened".into());
    }
    let node = &s.node;
    if node.outbox().is_none() {
        return Err("this node is not connected to the server; nothing was opened".into());
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut p = node.prs.0.lock().unwrap_or_else(|p| p.into_inner());
        if p.len() >= MAX_PENDING {
            return Err("too many requests are waiting for the server; try again".into());
        }
        p.insert(req_id.clone(), tx);
    }
    struct Gone<'a>(&'a PrPending, String);
    impl Drop for Gone<'_> {
        fn drop(&mut self) {
            self.0
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&self.1);
        }
    }
    let _gone = Gone(&node.prs, req_id.clone());
    let send = node.publish_wait(NodeToMaster::PullRequest {
        req_id,
        session_id: s.id,
        approval_id,
    });
    let asked = async {
        send.await;
        rx.await
    };
    let answer = match tokio::time::timeout(PR_WAIT, asked).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err("the server did not answer".into()),
        Err(_) => Err(format!(
            "the server did not answer within {} seconds; the pull request may or may not \
             have been opened",
            PR_WAIT.as_secs()
        )),
    };
    let detail = |ok: bool| {
        serde_json::json!({
            "session_id": s.id, "host": target.host, "full_name": target.full_name,
            "head": pr.head, "ok": ok,
        })
    };
    match answer {
        Ok(a) => {
            node.audit_log
                .record("forge.pull_request", detail(true).to_string());
            let what = match (a.number, a.existed) {
                (Some(n), true) => {
                    format!("a pull request for these branches is already open: #{n}")
                }
                (None, true) => "a pull request for these branches is already open".to_string(),
                (Some(n), false) => format!("opened pull request #{n}"),
                (None, false) => "opened the pull request".to_string(),
            };
            let mut out = format!(
                "[harness] {what} in {}/{} as {}",
                target.host, target.full_name, target.login
            );
            if let Some(u) = a.url {
                out.push_str("\n[harness] its address:\n");
                out.push_str(OUTPUT_QUOTE);
                out.push_str(&quoted_line(&u));
            }
            Ok(out)
        }
        Err(e) => {
            node.audit_log
                .record("forge.pull_request", detail(false).to_string());
            Err(said("the pull request was not opened", &e))
        }
    }
}

impl Session {
    /// The master's `target` for turn `turn_id`, kept where this node and
    /// this session allow `push` or `pull_request` in it, and the forge
    /// tools not allowed in it (withheld from the turn's agent).
    pub(super) fn forge_for_turn(
        &self,
        turn_id: &str,
        target: Option<PushTarget>,
    ) -> (Option<PushTarget>, Vec<&'static str>) {
        let all = harness_core::forge::FORGE_TOOLS.to_vec();
        let Some(target) = target else {
            return (None, all);
        };
        let callback = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|t| t.turn_id == turn_id && t.callback);
        let policy = &self.node.cfg.policy;
        let chat = self.profile == harness_core::event::Profile::Agentic
            && self.parent_session.is_none()
            && !self.no_callbacks
            && !self.incognito
            && !callback
            && crate::policy::valid_forge_host(&target.host)
            && crate::forge_clone::valid_full_name(&target.full_name)
            && crate::forge_clone::valid_username(&target.login);
        let push = chat
            && policy.projects.staging.is_some()
            && policy.forge.allows_push(&target.host)
            && !policy.denies(PUSH_TOOL);
        // `[forge] push = false` is the operator's "this node sends nothing
        // to a forge": it withholds pull requests too (the session lead's
        // decision of 2026-11-24), though the master makes that call.
        // Only where the master said it opens pull requests for this turn:
        // an older master never gets a request it does not know.
        let pr = chat
            && target.pull_requests
            && policy.forge.push
            && !policy.denies(harness_core::forge::PR_TOOL);
        let mut refused = Vec::new();
        if !push {
            refused.push(PUSH_TOOL);
        }
        if !pr {
            refused.push(harness_core::forge::PR_TOOL);
        }
        ((push || pr).then_some(target), refused)
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
    // The chat's folder, opened once and checked to be the session's pinned
    // directory; every read of the repository for this push goes through
    // this one descriptor (no path is resolved again).
    let path = s.checked_folder()?;
    let pinned = s.pin().map(|p| p.identity());
    if pinned.is_none() {
        return Err("this chat's folder is not pinned; nothing was pushed".into());
    }
    let folder = Arc::new(forge_push::Folder::open(&path, pinned)?);
    let bounds = forge_push::Bounds::of(&policy);
    let jail = s.node.cfg.git_sandbox.clone();
    let blocking = |f: Box<dyn FnOnce() -> Result<forge_push::Staged, String> + Send>| async move {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| format!("reading the folder: {e}"))?
    };
    let pin1 = {
        let (f, b) = (folder.clone(), branch.clone());
        tokio::task::spawn_blocking(move || forge_push::pin(&f, b.as_deref()))
            .await
            .map_err(|e| format!("reading the folder: {e}"))??
    };
    let format = if pin1.commit.len() == 64 {
        "sha256"
    } else {
        "sha1"
    };
    // Before the card: the subject, from a first repository of links. A
    // few of these at a time on the node (they need no push slot, so an
    // unanswered card holds nothing of anyone else's).
    let subject = {
        let Ok(_reading) = s.node.push_previews.clone().try_acquire_owned() else {
            return Err(
                "too many pushes are being prepared on this node; try again in a moment".into(),
            );
        };
        let (st, f, b) = (staging.clone(), folder.clone(), bounds.clone());
        let staged = blocking(Box::new(move || forge_push::stage(&st, &f, format, &b))).await?;
        forge_push::subject(&staged, &pin1.commit, jail.as_deref()).await?
    };
    let short = &pin1.commit[..12];
    let reason = format!(
        "push branch `{}` to {}/{} as {}",
        pin1.branch, target.host, target.full_name, target.login
    );
    // The destination is the master's (it shows its own on the page); the
    // folder supplies the branch, the commit and its subject.
    let preview = format!("branch: {}\ncommit: {} {}", pin1.branch, short, subject);
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
    // One push at a time on this node, from the Allow on.
    let Ok(_slot) = s.node.push_slots.clone().try_acquire_owned() else {
        return Err("another push is running on this node; try again when it has finished".into());
    };
    let pin2 = {
        let (f, b) = (folder.clone(), pin1.branch.clone());
        tokio::task::spawn_blocking(move || forge_push::pin(&f, Some(&b)))
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
        let (st, f, b) = (staging.clone(), folder.clone(), bounds.clone());
        blocking(Box::new(move || forge_push::stage(&st, &f, format, &b))).await?
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
