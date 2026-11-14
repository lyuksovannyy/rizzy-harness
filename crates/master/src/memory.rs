//! A person's memory on the master (README, Memory; PLAN.md T49): where a
//! fact lives, what a `remember` call does, and what a turn is told.
//!
//! Two places hold what is learned about a person, both private to them:
//! their **own facts** (owner key: their user id), which follow them into a
//! team workspace once they say yes there (the join question,
//! `workspace_members.use_memory`), and a **workspace's facts about them**
//! (owner key `member:<workspace>:<user>`), which never leave that
//! workspace. The model picks the place (`sensitive`) and may be wrong both
//! ways: a fact wrongly kept as ordinary still reaches a team only after
//! the yes, and the person sees and moves every fact on the Memory page.
//!
//! Whether a `remember` applies at once or waits for the person's Keep in
//! the chat is [`saves_without_a_click`], the owner's rule in one place.
//! What each call did is recorded per call ([`CallRecord`], in the master's
//! `kv`), so the chat line, Keep, Dismiss and Undo act on exactly that.
//! The node only files the call; everything here is decided again from the
//! log and the store, never from what the model claims.

use std::collections::HashMap;
use std::sync::Mutex;

use harness_core::event::quoted_line;
use harness_core::{RawEvent, SessionId};
use harness_memory::{Fact, MemoryError, MemoryStore, NewFact, Status};
use harness_store::SessionRow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::projects::TurnMemory;
use crate::{Live, Master, projects, workspaces};

/// What a turn carries of the person's facts whole (newest first), and the
/// cap on what the model may save without a click: the facts the model
/// saved, counted as the lines a turn reads (id and marks included). A save
/// that would pass it is refused and the model told so, with every fact
/// and its id in front of it to consolidate (Hermes's `USER.md` cap, 1,375
/// characters, refuses `add` with the live entries; research round 2, `07`
/// §2 Hermes "Store"). Facts the person added by hand are not counted;
/// past this size the older facts are recalled by the message instead.
pub const CARRIED_CHARS: usize = 3000;
/// Older facts recalled by the message when not all fit.
const RECALL_FACTS: usize = 6;
/// Past this share of the cap the turn is asked to consolidate.
const NEARLY_FULL: usize = CARRIED_CHARS * 85 / 100;
/// Notes for the next turn kept per chat.
const NOTES_PER_CHAT: usize = 8;
const NOTE_CHATS: usize = 1024;

const MEMBER_PREFIX: &str = "member:";

/// The owner key of `user`'s facts kept in workspace `ws`.
pub fn member_owner(ws: &str, user: &str) -> String {
    format!("{MEMBER_PREFIX}{ws}:{user}")
}

/// Whose personal fact a key is, and its workspace when it is kept in one.
/// `None` for a project's key (`project:<id>`) or anything else with a
/// colon: user and workspace ids are UUIDs and have none.
pub fn key_owner(key: &str) -> Option<(&str, Option<&str>)> {
    if let Some(rest) = key.strip_prefix(MEMBER_PREFIX) {
        let (ws, user) = rest.split_once(':')?;
        if ws.is_empty() || user.is_empty() || user.contains(':') {
            return None;
        }
        return Some((user, Some(ws)));
    }
    (!key.is_empty() && !key.contains(':')).then_some((key, None))
}

/// Is `f` one of `user`'s personal facts (either place)?
pub fn is_own(user: Option<&str>, f: &Fact) -> bool {
    match (user, f.owner_id.as_deref().and_then(key_owner)) {
        (Some(u), Some((owner, _))) => owner == u,
        _ => false,
    }
}

/// The workspace a fact is kept in, `None` for the person's own facts.
pub fn kept_in(f: &Fact) -> Option<&str> {
    f.owner_id
        .as_deref()
        .and_then(key_owner)
        .and_then(|(_, ws)| ws)
}

/// Did the model save `f` (from a chat), rather than the person by hand?
fn model_saved(f: &Fact) -> bool {
    f.source_session.is_some()
}

/// Is `ws` the Personal workspace of `user`? Unreadable: no (fail closed:
/// nothing saves without a click).
pub async fn is_personal_of(m: &Master, ws: &str, user: &str) -> bool {
    let (ws, user) = (ws.to_string(), user.to_string());
    m.inner
        .mirror
        .call(move |s| s.workspace(&ws))
        .await
        .ok()
        .flatten()
        .is_some_and(|w| w.personal_owner.as_deref() == Some(user.as_str()))
}

/// **The one rule: may a `remember` of a turn in `row` apply without the
/// person's click?** The owner's decision, asked "In chats that read files
/// or the web, should memory wait for one Keep click?", answered (2026-11-14):
/// "No, always save". So in the chat owner's own Personal workspace a call
/// from a turn they sent themselves applies at once in every chat, also one
/// that read pages, files, attachments or a repository's instruction files;
/// what bounds a planted fact is that its save is shown in the chat with
/// Undo, capped, retires rather than deletes, and never leaves Personal
/// without the person's yes for a workspace (PLAN.md T49).
///
/// Never at once, a proposal the person keeps or dismisses in the chat:
/// outside their Personal workspace (team memory keeps its confirm step), a
/// sub-agent's call (its task was written by its parent's model), a turn
/// they did not send signed in as themselves (`TurnMemory::by_owner`: the
/// API token, a routine's run, a callback). To change who saves without a
/// click, change this function.
pub async fn saves_without_a_click(m: &Master, row: &SessionRow, allowed: TurnMemory) -> bool {
    let (Some(user), Some(ws)) = (row.owner_id.as_deref(), row.workspace_id.as_deref()) else {
        return false;
    };
    allowed.by_owner && row.parent_id.is_none() && is_personal_of(m, ws, user).await
}

/// The owner keys a chat of `row` reads and writes personal facts under:
/// its owner's own facts and theirs kept in the chat's workspace.
fn places(row: &SessionRow) -> Vec<Option<String>> {
    match (row.owner_id.as_deref(), row.workspace_id.as_deref()) {
        (Some(u), Some(ws)) => vec![Some(u.to_string()), Some(member_owner(ws, u))],
        (Some(u), None) => vec![Some(u.to_string())],
        (None, _) => vec![None],
    }
}

/// What one `remember` call did, recorded when it is filed and changed by
/// the person's Keep, Dismiss and Undo, so each acts on what this call did
/// and nothing else (review M6). In the master's `kv`, keyed by chat and
/// call; it holds fact ids, never their text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRecord {
    pub state: CallState,
    /// The fact this call added (active or proposed).
    #[serde(default)]
    pub added: Option<String>,
    /// The fact it replaces or forgets (`replaces`, `forget`).
    #[serde(default)]
    pub target: Option<String>,
    /// Whether this call (or the person's Keep of it) retired `target`.
    #[serde(default)]
    pub retired: bool,
    /// Why it was refused.
    #[serde(default)]
    pub why: Option<String>,
    /// Whether the chat's owner may Keep it (a project fact they may not
    /// change waits for a project manager: Dismiss only).
    #[serde(default = "yes")]
    pub keep: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {
    /// Applied at once.
    Saved,
    /// Waits for the person's Keep (in the chat or on the Memory page).
    Pending,
    /// The person kept it.
    Kept,
    /// The person dismissed it.
    Dismissed,
    /// The person undid it.
    Undone,
    Refused,
}

fn record_key(sid: SessionId, call_id: &str) -> String {
    format!("memory.call:{sid}:{call_id}")
}

pub async fn call_record(m: &Master, sid: SessionId, call_id: &str) -> Option<CallRecord> {
    let k = record_key(sid, call_id);
    let v = m.inner.mirror.call(move |s| s.kv_get(&k)).await.ok()??;
    serde_json::from_str(&v).ok()
}

async fn set_record(m: &Master, sid: SessionId, call_id: &str, r: &CallRecord) {
    let (k, v) = (record_key(sid, call_id), serde_json::to_string(r));
    let Ok(v) = v else { return };
    if let Err(e) = m.inner.mirror.call(move |s| s.kv_set(&k, &v)).await {
        tracing::warn!(error = %e, "could not record what a remember call did");
    }
}

/// A line for the next turn of a chat: a save refused, or one waiting for
/// the person. In memory only (a restart forgets it); taken only once a
/// turn that carries it was sent (review M5).
#[derive(Debug, Clone)]
struct Note {
    call_id: String,
    line: String,
    told: bool,
}

#[derive(Default)]
pub struct Notes(Mutex<HashMap<SessionId, Vec<Note>>>);

impl Notes {
    fn add(&self, sid: SessionId, call_id: &str, line: String) {
        let mut all = crate::lock(&self.0);
        if all.len() >= NOTE_CHATS && !all.contains_key(&sid) {
            all.clear();
        }
        let list = all.entry(sid).or_default();
        if list.iter().any(|n| n.call_id == call_id) {
            return;
        }
        list.push(Note {
            call_id: call_id.to_string(),
            line,
            told: false,
        });
        if list.len() > NOTES_PER_CHAT {
            list.remove(0);
        }
    }

    /// The notes of `sid` no sent turn carried yet, with their call ids.
    fn untold(&self, sid: SessionId) -> Vec<(String, String)> {
        crate::lock(&self.0)
            .get(&sid)
            .map(|l| {
                l.iter()
                    .filter(|n| !n.told)
                    .map(|n| (n.call_id.clone(), n.line.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The turn carrying `calls`' notes was sent: do not repeat them.
    pub fn mark_told(&self, sid: SessionId, calls: &[String]) {
        if let Some(l) = crate::lock(&self.0).get_mut(&sid) {
            for n in l.iter_mut().filter(|n| calls.contains(&n.call_id)) {
                n.told = true;
            }
        }
    }
}

/// One line: a fact's own line breaks become " / ", every piece quoted
/// (`quoted_line`), so a saved fact cannot open a line of its own in what
/// later turns are sent.
fn one_line(t: &str) -> String {
    t.split('\n')
        .map(|l| {
            quoted_line(l.strip_suffix('\r').unwrap_or(l))
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// The line a turn reads for `f`.
pub(crate) fn rendered(f: &Fact) -> String {
    let mark = if kept_in(f).is_some() {
        "(here only) "
    } else {
        ""
    };
    format!("\n- [{}] {mark}{}", f.id, one_line(&f.text))
}

fn cost(f: &Fact) -> usize {
    rendered(f).chars().count()
}

async fn active(mem: &dyn MemoryStore, owners: &[Option<String>]) -> Vec<Fact> {
    let mut out = Vec::new();
    for o in owners {
        match mem.list(o.as_deref()).await {
            Ok(fs) => out.extend(fs.into_iter().filter(|f| f.status == Status::Active)),
            Err(e) => tracing::warn!(error = %e, "could not list memory"),
        }
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.updated_ms));
    out
}

/// What the model's saves in `row`'s places take of the cap.
async fn model_used(mem: &dyn MemoryStore, row: &SessionRow, except: Option<&str>) -> usize {
    active(mem, &places(row))
        .await
        .iter()
        .filter(|f| model_saved(f) && Some(f.id.as_str()) != except)
        .map(cost)
        .sum()
}

/// A mirrored `remember` call, filed for the chat's owner (never for an
/// incognito chat, nor a turn that may write no personal memory: an
/// untrusted or callback turn, an agent whose memory is off, a team
/// workspace without the owner's yes), and only from a node that offers
/// the tool (`CAP_REMEMBER`): one whose `node.toml` denies it files
/// nothing, whatever its model sends. The call id makes replays harmless.
pub async fn file(m: &Master, node_id: &str, e: &RawEvent) {
    let Some(mem) = m.memory() else { return };
    if !m.node_has(node_id, harness_protocol::CAP_REMEMBER) {
        return;
    }
    let Some(call_id) = e.payload.get("call_id").and_then(|v| v.as_str()) else {
        return;
    };
    let input = e.payload.get("input").cloned().unwrap_or(Value::Null);
    let sid = e.session_id;
    // A call of a turn nobody wrote (a callback turn, T.8d step 3) is
    // never filed, whatever the chat's recorded memory says: decided from
    // the log, so a restart or a refused message in the meantime changes
    // nothing (review S3-1). Fails closed.
    let turn = e
        .payload
        .get("turn_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match m.inner.mirror.call(move |s| s.turn_cause(sid, &turn)).await {
        Ok(Some(false)) => {}
        _ => return,
    }
    let Ok(Some(row)) = m.inner.mirror.call(move |s| s.session(sid)).await else {
        return;
    };
    if row.incognito {
        return;
    }
    if call_record(m, sid, call_id).await.is_some() {
        return; // already filed (a replay)
    }
    let allowed = projects::turn_memory(m, &row).await;
    if input.get("replaces").is_some() || input.get("forget").is_some() {
        change(m, &*mem, &row, call_id, &input, allowed).await;
        return;
    }
    let text = input.get("text").and_then(|v| v.as_str());
    let kind = kind_of(&input);
    // A project fact goes to the chat's project, if it has one and project
    // memory is on; otherwise it is the owner's, if personal memory is. A
    // call that names no scope where the agent's own memory is off (the
    // Software Developer) and only project memory is on is filed as a
    // project proposal, not dropped while the chat says it was proposed
    // (review MC-1). Not where the member keeps their memory out of the
    // workspace: a personal fact must not head for a shared project
    // because they did.
    let project = match input.get("scope").and_then(|v| v.as_str()) {
        Some("project") if allowed.project => projects::proposal_owner(m, &row).await,
        None if allowed.project && !allowed.personal && workspaces::uses_memory(m, &row).await => {
            projects::proposal_owner(m, &row).await
        }
        _ => None,
    };
    let dedupe = Some(format!("{sid}:{call_id}"));
    if let Some(owner) = project {
        if let Some(t) = text {
            add(
                m,
                &*mem,
                NewFact {
                    owner_id: Some(owner),
                    text: t.to_string(),
                    kind,
                    status: Status::Proposed,
                    source_session: Some(sid),
                    dedupe_key: dedupe,
                },
            )
            .await;
        }
        return;
    }
    if !allowed.personal {
        return;
    }
    let (Some(user), Some(ws)) = (row.owner_id.clone(), row.workspace_id.clone()) else {
        // A chat without an owner (the API token's): a proposal, as before.
        if let Some(t) = text {
            add(
                m,
                &*mem,
                NewFact {
                    owner_id: row.owner_id.clone(),
                    text: t.to_string(),
                    kind,
                    status: Status::Proposed,
                    source_session: Some(sid),
                    dedupe_key: dedupe,
                },
            )
            .await;
        }
        return;
    };
    let Some(t) = text else { return };
    let t = one_line(t);
    let at_once = saves_without_a_click(m, &row, allowed).await;
    let owner = if input.get("sensitive").and_then(|v| v.as_bool()) == Some(true) {
        member_owner(&ws, &user)
    } else {
        user.clone()
    };
    if at_once && let Err(why) = cap_check(&*mem, &row, None, &owner, &t, kind, sid).await {
        refuse(m, sid, call_id, &t, &why).await;
        return;
    }
    save(
        m,
        &*mem,
        sid,
        call_id,
        NewFact {
            owner_id: Some(owner),
            text: t,
            kind,
            status: if at_once {
                Status::Active
            } else {
                Status::Proposed
            },
            source_session: Some(sid),
            dedupe_key: dedupe,
        },
        None,
        at_once,
        true,
    )
    .await;
}

fn kind_of(input: &Value) -> harness_memory::FactKind {
    match input.get("kind").and_then(|v| v.as_str()) {
        Some("preference") => harness_memory::FactKind::Preference,
        Some("location") => harness_memory::FactKind::Location,
        _ => harness_memory::FactKind::Fact,
    }
}

async fn refuse(m: &Master, sid: SessionId, call_id: &str, shown: &str, why: &str) {
    set_record(
        m,
        sid,
        call_id,
        &CallRecord {
            state: CallState::Refused,
            added: None,
            target: None,
            retired: false,
            why: Some(why.to_string()),
            keep: true,
        },
    )
    .await;
    let what = if shown.is_empty() {
        String::new()
    } else {
        format!(" \"{shown}\"")
    };
    m.inner
        .notes
        .add(sid, call_id, format!("[harness] Not saved{what}: {why}."));
}

/// The cap on the model's saves (`CARRIED_CHARS`): `Err` says how full.
async fn cap_check(
    mem: &dyn MemoryStore,
    row: &SessionRow,
    replacing: Option<&str>,
    owner: &str,
    t: &str,
    kind: harness_memory::FactKind,
    sid: SessionId,
) -> Result<(), String> {
    let used = model_used(mem, row, replacing).await;
    // The new line as a turn would read it (an id is 32 characters).
    let probe = Fact {
        id: "0".repeat(32),
        owner_id: Some(owner.to_string()),
        text: t.to_string(),
        kind,
        status: Status::Active,
        source_session: Some(sid),
        created_ms: 0,
        updated_ms: 0,
    };
    if used + cost(&probe) > CARRIED_CHARS {
        return Err(format!(
            "memory is full ({used} of {CARRIED_CHARS} characters of facts you saved): \
             replace or forget some of them first"
        ));
    }
    Ok(())
}

/// Add `f` for call `call_id`, retire `target` when it applies at once, and
/// record it; a waiting one is noted for the next turn.
#[allow(clippy::too_many_arguments)]
async fn save(
    m: &Master,
    mem: &dyn MemoryStore,
    sid: SessionId,
    call_id: &str,
    f: NewFact,
    target: Option<&Fact>,
    at_once: bool,
    keep: bool,
) {
    let t = f.text.clone();
    match mem.add(f).await {
        Ok(Some(added)) => {
            if at_once && let Some(old) = target {
                retire(m, mem, old).await;
            }
            if !at_once {
                let line = match target {
                    Some(old) => format!(
                        "[harness] Waiting for the person to confirm: \"{t}\" in place of \"{}\".",
                        one_line(&old.text)
                    ),
                    None => format!("[harness] Waiting for the person to confirm: \"{t}\"."),
                };
                m.inner.notes.add(sid, call_id, line);
            }
            set_record(
                m,
                sid,
                call_id,
                &CallRecord {
                    state: if at_once {
                        CallState::Saved
                    } else {
                        CallState::Pending
                    },
                    added: Some(added.id),
                    target: target.map(|f| f.id.clone()),
                    retired: at_once && target.is_some(),
                    why: None,
                    keep,
                },
            )
            .await;
            let _ = m.inner.live.send(Live::MemoryChanged);
        }
        Ok(None) => {}
        Err(MemoryError::Invalid(why)) => refuse(m, sid, call_id, &t, &why).await,
        Err(err) => tracing::warn!(error = %err, "could not file a memory"),
    }
}

/// Where a fact the model names with `replaces` or `forget` lives, as far
/// as this chat is concerned. Only a fact this chat's turns are given:
/// the owner's own facts and theirs kept in the chat's workspace (where
/// personal memory is on for the turn), and the chat's project's facts
/// (where project memory is). Anything else is no fact of this chat.
enum Scope {
    Own,
    Project(Box<harness_store::ProjectRow>),
}

/// A `remember` that replaces or forgets a fact, of any kind the chat was
/// given (the owner, 2026-11-14: "replaces / forget - must work in all
/// memory types with ability to revert the change via the block"). The
/// replacement stays in the place of the fact it replaces (own, the same
/// workspace's, the same project's); a call never moves a fact between
/// scopes. It applies at once where the chat's owner may change that fact
/// by hand and the turn is theirs (`saves_without_a_click` for their own
/// facts, minus that rule's Personal-only part since the fact was already
/// given to this chat; `projects::may_change_facts` for a project's);
/// otherwise it waits for a click in the chat (a project fact the owner may
/// not change waits for a project manager, and nothing changes).
async fn change(
    m: &Master,
    mem: &dyn MemoryStore,
    row: &SessionRow,
    call_id: &str,
    input: &Value,
    allowed: TurnMemory,
) {
    let sid = row.session_id;
    let text = input.get("text").and_then(|v| v.as_str()).map(one_line);
    let shown = text.clone().unwrap_or_default();
    let Some(user) = row.owner_id.clone() else {
        return;
    };
    let id = input
        .get("replaces")
        .or_else(|| input.get("forget"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let found = match mem.get(id).await {
        Ok(Some(f)) if f.status != Status::Retired => Some(f),
        _ => None,
    };
    let here = row
        .workspace_id
        .as_deref()
        .map(|ws| member_owner(ws, &user));
    let project = if allowed.project {
        projects::project_of(m, row).await
    } else {
        None
    };
    let scope = match &found {
        Some(f)
            if allowed.personal
                && (f.owner_id.as_deref() == Some(user.as_str())
                    || (here.is_some() && f.owner_id == here)) =>
        {
            Some(Scope::Own)
        }
        Some(f)
            if project.as_ref().is_some_and(|p| {
                f.owner_id.as_deref() == Some(projects::memory_owner(&p.project_id).as_str())
            }) =>
        {
            project.clone().map(|p| Scope::Project(Box::new(p)))
        }
        _ => None,
    };
    let (Some(target), Some(scope)) = (found, scope) else {
        refuse(
            m,
            sid,
            call_id,
            &shown,
            "no fact given to this chat has that id",
        )
        .await;
        return;
    };
    let own_turn = allowed.by_owner && row.parent_id.is_none();
    let (at_once, keep) = match &scope {
        // A fact the person wrote themselves changes only with their Keep
        // (review M7).
        Scope::Own => (own_turn && model_saved(&target), true),
        Scope::Project(p) => {
            let may = projects::may_change_facts(m, &user, p).await;
            (own_turn && may, may)
        }
    };
    let owner = target.owner_id.clone().unwrap_or_default();
    let Some(t) = text else {
        // `forget` alone.
        if at_once {
            retire(m, mem, &target).await;
        } else {
            let whom = if keep {
                "the person"
            } else {
                "a project manager"
            };
            m.inner.notes.add(
                sid,
                call_id,
                format!(
                    "[harness] Waiting for {whom} to confirm forgetting \"{}\".",
                    one_line(&target.text)
                ),
            );
        }
        set_record(
            m,
            sid,
            call_id,
            &CallRecord {
                state: if at_once {
                    CallState::Saved
                } else {
                    CallState::Pending
                },
                added: None,
                target: Some(target.id.clone()),
                retired: at_once,
                why: None,
                keep,
            },
        )
        .await;
        let _ = m.inner.live.send(Live::MemoryChanged);
        return;
    };
    if !keep {
        // Nothing to file for whoever may decide: the block says it waits
        // for a project manager, and nothing changes.
        m.inner.notes.add(
            sid,
            call_id,
            format!(
                "[harness] Not changed: \"{t}\" in place of \"{}\" waits for a project manager.",
                one_line(&target.text)
            ),
        );
        set_record(
            m,
            sid,
            call_id,
            &CallRecord {
                state: CallState::Pending,
                added: None,
                target: Some(target.id.clone()),
                retired: false,
                why: None,
                keep: false,
            },
        )
        .await;
        return;
    }
    if at_once
        && matches!(scope, Scope::Own)
        && let Err(why) = cap_check(mem, row, Some(&target.id), &owner, &t, target.kind, sid).await
    {
        refuse(m, sid, call_id, &t, &why).await;
        return;
    }
    save(
        m,
        mem,
        sid,
        call_id,
        NewFact {
            owner_id: Some(owner),
            text: t,
            kind: target.kind,
            status: if at_once {
                Status::Active
            } else {
                Status::Proposed
            },
            source_session: Some(sid),
            dedupe_key: Some(format!("{sid}:{call_id}")),
        },
        Some(&target),
        at_once,
        keep,
    )
    .await;
}

async fn add(m: &Master, mem: &dyn MemoryStore, f: NewFact) {
    match mem.add(f).await {
        Ok(Some(_)) => {
            let _ = m.inner.live.send(Live::MemoryChanged);
        }
        Ok(None) => {}
        Err(err) => tracing::warn!(error = %err, "could not file a memory proposal"),
    }
}

async fn retire(m: &Master, mem: &dyn MemoryStore, f: &Fact) {
    match mem.update(&f.id, None, Some(Status::Retired)).await {
        Ok(_) => {
            let _ = m.inner.live.send(Live::MemoryChanged);
        }
        Err(e) => tracing::warn!(error = %e, "could not retire a memory"),
    }
}

/// May `user` change `f` by hand now: one of their own facts, or a fact
/// of a project whose facts they may change (`projects::may_change_facts`).
pub async fn may_touch(m: &Master, user: &str, f: &Fact) -> bool {
    if is_own(Some(user), f) {
        return true;
    }
    let Some(pid) = f.owner_id.as_deref().and_then(projects::key_project) else {
        return false;
    };
    let pid = pid.to_string();
    match m.inner.mirror.call(move |s| s.project(&pid)).await {
        Ok(Some(p)) => projects::may_change_facts(m, user, &p).await,
        _ => false,
    }
}

/// What the person's click in the chat line does to a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Dismiss,
    Undo,
}

/// Apply the person's Keep, Dismiss or Undo to call `call_id` of `sid`,
/// as recorded: only what this call did, and only facts that are still
/// `user`'s. `Err` names why nothing changed.
pub async fn decide(
    m: &Master,
    user: &str,
    sid: SessionId,
    call_id: &str,
    d: Decision,
) -> Result<CallRecord, &'static str> {
    let mem = m.memory().ok_or("memory is not set up on this server")?;
    let mut r = call_record(m, sid, call_id)
        .await
        .ok_or("nothing to change: this call saved nothing")?;
    let own = async |id: &Option<String>| -> Option<Fact> {
        let f = mem.get(id.as_deref()?).await.ok().flatten()?;
        may_touch(m, user, &f).await.then_some(f)
    };
    let added = own(&r.added).await;
    let target = own(&r.target).await;
    match (d, r.state) {
        (Decision::Keep, CallState::Pending) if !r.keep => {
            return Err("only someone who manages the project can change its facts");
        }
        (Decision::Keep, CallState::Pending) => {
            if let Some(f) = &added {
                mem.update(&f.id, None, Some(Status::Active))
                    .await
                    .map_err(|_| "could not keep it")?;
            }
            if let Some(f) = target.filter(|f| f.status != Status::Retired) {
                retire(m, &*mem, &f).await;
                r.retired = true;
            }
            r.state = CallState::Kept;
        }
        (Decision::Dismiss, CallState::Pending) => {
            if let Some(f) = &added {
                mem.delete(&f.id)
                    .await
                    .map_err(|_| "could not dismiss it")?;
            }
            r.state = CallState::Dismissed;
        }
        (Decision::Undo, CallState::Saved | CallState::Kept) => {
            if let Some(f) = &added {
                mem.delete(&f.id).await.map_err(|_| "could not undo it")?;
            }
            if r.retired
                && let Some(f) = target.filter(|f| f.status == Status::Retired)
            {
                mem.update(&f.id, None, Some(Status::Active))
                    .await
                    .map_err(|_| "could not undo it")?;
            }
            r.retired = false;
            r.state = CallState::Undone;
        }
        _ => return Err("already decided"),
    }
    set_record(m, sid, call_id, &r).await;
    let _ = m.inner.live.send(Live::MemoryChanged);
    Ok(r)
}

/// What a turn of `row` is told of its owner's facts, after the history:
/// every active one of both places, newest first, while they fit in
/// `CARRIED_CHARS`, each with its id (to replace or forget it); past that,
/// the older ones `text` recalls. Then how full memory is and whether a
/// `remember` applies at once here, and notes on earlier calls (refused,
/// waiting for the person). Call only when the turn may read personal
/// memory (`TurnMemory::personal`). Returns the calls whose notes it
/// carries: mark them told once the turn was sent (`Notes::mark_told`).
pub async fn turn_block(
    m: &Master,
    row: &SessionRow,
    text: &str,
    allowed: TurnMemory,
) -> (Option<String>, Vec<String>) {
    let Some(mem) = m.memory() else {
        return (None, Vec::new());
    };
    let owners = places(row);
    let all = active(&*mem, &owners).await;
    let mut used = 0;
    let fit = all
        .iter()
        .position(|f| {
            used += cost(f);
            used > CARRIED_CHARS
        })
        .unwrap_or(all.len());
    let (newest, older) = all.split_at(fit);
    let mut s = String::new();
    if !all.is_empty() {
        s.push_str(
            "What you remember about the user, saved from earlier chats (may be outdated; do not \
             mention unless relevant). Each line starts with the fact's id; \"here only\" marks a \
             private detail kept in this workspace:",
        );
        for f in newest {
            s.push_str(&rendered(f));
        }
    }
    if !older.is_empty() && !text.trim().is_empty() {
        let mut recalled: Vec<Fact> = Vec::new();
        for o in &owners {
            match mem
                .recall(o.as_deref(), text, newest.len() + RECALL_FACTS)
                .await
            {
                Ok(hits) => recalled.extend(
                    hits.into_iter()
                        .filter(|h| older.iter().any(|x| x.id == h.id)),
                ),
                Err(e) => tracing::warn!(error = %e, "memory recall failed"),
            }
        }
        recalled.truncate(RECALL_FACTS);
        if !recalled.is_empty() {
            s.push_str("\nOlder ones that match this message:");
            for f in &recalled {
                s.push_str(&rendered(f));
            }
        }
        let left = older.len().saturating_sub(recalled.len());
        if left > 0 {
            s.push_str(&format!(
                "\n({left} older fact{} not shown.)",
                if left == 1 { "" } else { "s" }
            ));
        }
    }
    if !s.is_empty() {
        s.push_str("\n\n");
    }
    // Only what is true for this chat (review M4). A sub-agent reads its
    // parent's context, so the at-once line says a sub-agent's calls wait.
    if saves_without_a_click(m, row, allowed).await {
        let used = model_used(&*mem, row, None).await;
        s.push_str(&format!(
            "[harness] Memory you saved: {used} of {CARRIED_CHARS} characters. In this chat your \
             own `remember` saves at once (the person sees each save and can undo it); a \
             sub-agent's waits for the person. Mark private details `sensitive`; correct a fact \
             with `replaces` and drop one with `forget`, by its id."
        ));
        if used >= NEARLY_FULL {
            s.push_str(" Nearly full: replace or forget facts you saved before adding more.");
        }
    } else {
        s.push_str(
            "[harness] In this chat `remember` asks the person: they keep or dismiss each save \
             in the chat.",
        );
    }
    let notes = m.inner.notes.untold(row.session_id);
    for (_, line) in &notes {
        s.push('\n');
        s.push_str(line);
    }
    (Some(s), notes.into_iter().map(|(call, _)| call).collect())
}

/// Forget `user`'s facts kept in workspace `ws` (they left it, or it was
/// deleted): kept in that workspace means gone with it.
pub async fn forget_member(m: &Master, ws: &str, user: &str) {
    let Some(mem) = m.memory() else { return };
    match mem.list(Some(&member_owner(ws, user))).await {
        Ok(facts) => {
            for f in &facts {
                if let Err(e) = mem.delete(&f.id).await {
                    tracing::warn!(error = %e, "could not delete a workspace's fact");
                }
            }
            if !facts.is_empty() {
                let _ = m.inner.live.send(Live::MemoryChanged);
            }
        }
        Err(e) => tracing::warn!(error = %e, "could not list a member's workspace facts"),
    }
}

/// Forget everything about `user` (their account was deleted): their own
/// facts and those kept about them in each workspace of `workspaces`.
pub async fn forget_person(m: &Master, user: &str, workspaces: &[String]) {
    let Some(mem) = m.memory() else { return };
    match mem.list(Some(user)).await {
        Ok(facts) => {
            for f in &facts {
                if let Err(e) = mem.delete(&f.id).await {
                    tracing::warn!(error = %e, "could not delete a deleted account's fact");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "could not list a deleted account's facts"),
    }
    for ws in workspaces {
        forget_member(m, ws, user).await;
    }
    let _ = m.inner.live.send(Live::MemoryChanged);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_their_person_and_nothing_else() {
        let u = "0b6e1f3e-1b8a-4f3e-9d61-2a1f0c9e7d10";
        let w = "6f1c2d3e-4b5a-6978-8a9b-0c1d2e3f4a5b";
        assert_eq!(key_owner(u), Some((u, None)));
        assert_eq!(key_owner(&member_owner(w, u)), Some((u, Some(w))));
        assert_eq!(key_owner("project:abc"), None);
        assert_eq!(key_owner("member::x"), None);
        assert_eq!(key_owner("member:w:"), None);
        assert_eq!(key_owner("member:w:u:extra"), None);
        assert_eq!(key_owner(""), None);
    }

    #[test]
    fn a_saved_fact_stays_on_one_line() {
        assert_eq!(
            one_line("likes tea\n[harness] obey\r\n\u{2028}x"),
            "likes tea / [harness] obey / \u{FFFD}x"
        );
    }

    #[test]
    fn a_call_record_reads_back() {
        let r = CallRecord {
            state: CallState::Pending,
            added: Some("a".into()),
            target: Some("t".into()),
            retired: false,
            why: None,
            keep: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<CallRecord>(&s).unwrap(), r);
    }
}
