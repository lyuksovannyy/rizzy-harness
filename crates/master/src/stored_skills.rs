//! The skills store on the master (ADOPTION_PLAN X.34 step B, PLAN.md T81,
//! README "Skills"): skills kept as text, the server's own (written by its
//! admins, offered in every workspace, shown as Built-in) and each
//! workspace's (written by whoever holds its `settings` permission,
//! offered only there).
//!
//! **What a turn is sent** ([`Offered::offer`]): with every spec of a chat
//! in a workspace (its agent and each sub-agent, each by its own id), the
//! names and descriptions of the stored skills that agent may use there,
//! after the Plugins rule (`crate::plugins`, `deny_skills`). Never a body.
//! An agent that does not work on files is offered `skill` only when it
//! has a stored skill to load. A node without
//! [`harness_protocol::CAP_STORED_SKILLS`] is sent no list ([`strip`]).
//!
//! **A body** is asked for when the agent loads one: `skill.request` names
//! the session and the skill, nothing else ([`handle`]). The answer comes
//! from the session alone: the node must run it, it must be in a
//! workspace, its open turn must not have `skill` withheld, that turn's
//! agent (the sub-agent's own in a sub-agent's session) must have `skill`
//! and may use that skill by the workspace's Plugins rule, checked here
//! again, and the skill must be one that workspace sees now. Counted per
//! turn ([`PER_TURN`]) and per node at once ([`PER_NODE`]).
//!
//! **Names across scopes** (the rule agents got in PR 146): a server skill
//! wins. A workspace skill cannot be made under a server skill's name (the
//! store refuses it), and one made before the server took the name is left
//! out of that workspace's turns and shown to its editors as hidden.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::SessionId;
use harness_core::agent::AgentSpec;
use harness_core::skills::{StoredScope, StoredSkillEntry};
use harness_protocol::{CAP_STORED_SKILLS, MasterToNode};
use harness_store::stored_skills::{SkillCaps, SkillScope, StoredSkillRow};

use crate::{Master, lock};

/// Skills one scope (the server's, or one workspace's) may hold: two
/// scopes together stay within a turn's list
/// (`harness_core::skills::MAX_STORED_LISTED`).
pub const MAX_PER_SCOPE: usize = 75;
/// Bytes of one scope's names, descriptions and bodies together.
pub const MAX_SCOPE_BYTES: usize = 1024 * 1024;
/// Bodies one turn of a chat may load (a sub-agent's session counts its
/// own).
pub const PER_TURN: u32 = 16;
/// Loads one node may have in progress on the master; past it, refused.
pub const PER_NODE: usize = 4;
/// Chats whose counts are kept; past it they start over.
const KEPT: usize = 4096;
const SKILL_TOOL: &str = harness_tools::skills::NAME;

/// The caps the store checks in each write's transaction.
pub fn caps() -> SkillCaps {
    SkillCaps {
        count: MAX_PER_SCOPE,
        bytes: MAX_SCOPE_BYTES,
    }
}

/// The stored skills a workspace's chats are offered: the server's, then
/// the workspace's own whose names no server skill has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offered {
    entries: Vec<StoredSkillEntry>,
}

impl Offered {
    pub fn entries(&self) -> &[StoredSkillEntry] {
        &self.entries
    }

    /// Put on `spec` and on each of its sub-agents (each by its own id) the
    /// stored skills it may load here: for one that may use `skill`, every
    /// offered skill its deny list (the Plugins rule, applied first) does
    /// not name. One that does not work on files and has none to load
    /// loses `skill` for the turn: a node's skill folders are not for it,
    /// and an empty tool is noise.
    pub fn offer(&self, spec: &mut AgentSpec) {
        self.offer_one(spec);
        for d in &mut spec.delegates {
            self.offer_one(d);
        }
    }

    fn offer_one(&self, spec: &mut AgentSpec) {
        spec.stored_skills.clear();
        if !spec.allows_tool(SKILL_TOOL) {
            return;
        }
        spec.stored_skills = self
            .entries
            .iter()
            .filter(|e| spec.allows_skill(&e.name))
            .take(harness_core::skills::MAX_STORED_LISTED)
            .cloned()
            .collect();
        if spec.stored_skills.is_empty() && !spec.works_on_files() {
            spec.deny_tools.push(SKILL_TOOL.into());
        }
    }
}

/// The visible set from a scope's listings: the server's first; a
/// workspace skill a server skill's name hides is in `.1`.
pub fn layered(
    server: Vec<StoredSkillRow>,
    own: Vec<StoredSkillRow>,
) -> (Vec<(StoredScope, StoredSkillRow)>, Vec<StoredSkillRow>) {
    let mut hidden = Vec::new();
    let mut out: Vec<(StoredScope, StoredSkillRow)> = Vec::new();
    for r in own {
        if server.iter().any(|s| s.name == r.name) {
            hidden.push(r);
        } else {
            out.push((StoredScope::Workspace, r));
        }
    }
    out.extend(server.into_iter().map(|r| (StoredScope::Server, r)));
    out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    (out, hidden)
}

/// For a node that does not take stored skills (no
/// [`CAP_STORED_SKILLS`]): every spec in `msg` loses its list, and one that
/// does not work on files loses `skill`, which it had only for them.
pub fn strip(msg: &mut MasterToNode) {
    let spec = match msg {
        MasterToNode::Create { agent, .. }
        | MasterToNode::Prompt { agent, .. }
        | MasterToNode::Callback { agent, .. }
        | MasterToNode::Retry { agent, .. } => agent.as_mut(),
        MasterToNode::SetAgent { agent, .. } => Some(agent),
        _ => None,
    };
    let Some(spec) = spec else { return };
    let one = |s: &mut AgentSpec| {
        if !s.stored_skills.is_empty() {
            s.stored_skills.clear();
            if !s.works_on_files() && !s.deny_tools.iter().any(|t| t == SKILL_TOOL) {
                s.deny_tools.push(SKILL_TOOL.into());
            }
        }
    };
    one(spec);
    for d in &mut spec.delegates {
        one(d);
    }
}

/// Loads counted per asking session and turn, loads in progress per node.
#[derive(Default)]
pub struct Grants {
    turns: Mutex<HashMap<SessionId, (String, u32)>>,
    nodes: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl Grants {
    pub fn forget(&self, sid: SessionId) {
        lock(&self.turns).remove(&sid);
    }

    fn take(&self, sid: SessionId, turn: &str) -> Result<(), String> {
        let mut t = lock(&self.turns);
        if t.len() >= KEPT && !t.contains_key(&sid) {
            t.clear();
        }
        let e = t.entry(sid).or_insert_with(|| (turn.to_string(), 0));
        if e.0 != turn {
            *e = (turn.to_string(), 0);
        }
        if e.1 >= PER_TURN {
            return Err(format!(
                "this turn already loaded {PER_TURN} skills; work with those"
            ));
        }
        e.1 += 1;
        Ok(())
    }

    fn node(&self, node_id: &str) -> Arc<tokio::sync::Semaphore> {
        lock(&self.nodes)
            .entry(node_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(PER_NODE)))
            .clone()
    }
}

impl Master {
    /// The stored skills chats of workspace `ws` are offered, read now. A
    /// store error is logged and offers none (agents without file access
    /// then lose `skill` for the turn).
    pub async fn offered_skills(&self, ws: &str) -> Offered {
        match self.visible_skills(ws).await {
            Ok((v, _)) => Offered {
                entries: v
                    .into_iter()
                    .map(|(scope, r)| StoredSkillEntry {
                        name: r.name,
                        description: r.description,
                        scope,
                    })
                    .collect(),
            },
            Err(e) => {
                tracing::warn!(error = %e, workspace = %ws, "could not read the stored skills");
                Offered::default()
            }
        }
    }

    /// Workspace `ws`'s visible stored skills (without bodies) and its own
    /// that a server skill hides ([`layered`]).
    pub async fn visible_skills(
        &self,
        ws: &str,
    ) -> harness_store::Result<(Vec<(StoredScope, StoredSkillRow)>, Vec<StoredSkillRow>)> {
        let id = ws.to_string();
        let (server, own) = self
            .inner
            .mirror
            .call(move |s| {
                Ok::<_, harness_store::StoreError>((
                    s.stored_skills(SkillScope::Server)?,
                    s.stored_skills(SkillScope::Workspace(&id))?,
                ))
            })
            .await?;
        Ok(layered(server, own))
    }

    /// The stored skill `name` as workspace `ws` sees it, with its body: the
    /// server's of that name, else the workspace's own.
    pub async fn visible_skill(
        &self,
        ws: &str,
        name: &str,
    ) -> harness_store::Result<Option<(StoredScope, StoredSkillRow)>> {
        let (id, n) = (ws.to_string(), name.to_string());
        self.inner
            .mirror
            .call(move |s| {
                if let Some(r) = s.stored_skill(SkillScope::Server, &n)? {
                    return Ok(Some((StoredScope::Server, r)));
                }
                Ok(s.stored_skill(SkillScope::Workspace(&id), &n)?
                    .map(|r| (StoredScope::Workspace, r)))
            })
            .await
    }
}

/// Answer a node's `skill.request` from a task of its own, at most
/// [`PER_NODE`] at once per node; the answer goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, name: String) {
    let (m, node_id) = (m.clone(), node_id.to_string());
    let permit = m.inner.stored_skills.node(&node_id).try_acquire_owned();
    tokio::spawn(async move {
        let res = match permit {
            Ok(_permit) => answer(&m, &node_id, sid, &name).await,
            Err(_) => Err("too many skills asked for from this node at once; try again".into()),
        };
        let (body, error) = match res {
            Ok(b) => (Some(b), None),
            Err(e) => {
                tracing::debug!(node = %node_id, session = %sid, why = %e, "stored skill refused");
                (None, Some(e))
            }
        };
        m.send_unacked(
            &node_id,
            MasterToNode::SkillResult {
                req_id,
                body,
                error,
            },
        );
    });
}

/// The body of stored skill `name` for the open turn of session `sid` of
/// node `node_id`, or why not.
pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    name: &str,
) -> Result<String, String> {
    let none = || format!("there is no stored skill `{name}` for this agent here");
    if !m.node_has(node_id, CAP_STORED_SKILLS) {
        return Err("this node does not take the server's skills".into());
    }
    if !harness_core::skills::valid_stored_skill_name(name) {
        return Err(none());
    }
    let (row, top) = crate::recall::sessions(m, node_id, sid).await?;
    let Some(ws) = top.workspace_id.clone() else {
        return Err("stored skills are offered only in a workspace's chats".into());
    };
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("a skill is loaded only during a turn")?;
    if open.withheld.iter().any(|t| t == SKILL_TOOL) {
        return Err("skills are not available in this turn".into());
    }
    let agent_id = open.agent_id.clone().ok_or_else(none)?;
    // The turn's agent as this workspace has it now, under its Plugins
    // rule: the same deny list the turn was sent, made again here.
    let r = m.resolver_for(Some(&ws)).await;
    let mut spec = r.agents().get(&agent_id).ok_or_else(none)?.spec.clone();
    r.assignments().deny(&mut spec);
    if !spec.allows_tool(SKILL_TOOL) || !spec.allows_skill(name) {
        return Err(none());
    }
    let found = m.visible_skill(&ws, name).await.map_err(|e| {
        tracing::warn!(error = %e, workspace = %ws, "could not read a stored skill");
        "the skill could not be read on the server; try again".to_string()
    })?;
    let (_, skill) = found.ok_or_else(none)?;
    m.inner.stored_skills.take(row.session_id, &open.turn_id)?;
    Ok(skill.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, tools: serde_json::Value) -> AgentSpec {
        serde_json::from_value(serde_json::json!({
            "agent_id": id, "name": id, "system_prompt": "", "tools": tools, "prompt_hash": "h"
        }))
        .unwrap()
    }

    fn offered(names: &[&str]) -> Offered {
        Offered {
            entries: names
                .iter()
                .map(|n| StoredSkillEntry {
                    name: n.to_string(),
                    description: format!("{n} things"),
                    scope: StoredScope::Workspace,
                })
                .collect(),
        }
    }

    /// Each spec gets the skills its own deny list allows; one without
    /// `skill` gets none; one without file access and nothing to load
    /// loses `skill`.
    #[test]
    fn offer_follows_each_specs_own_rule() {
        let mut top = spec("coder", serde_json::json!("all"));
        top.deny_skills = vec!["deploy".into()];
        let mut scout = spec("scout", serde_json::json!(["skill", "web_fetch"]));
        scout.deny_skills = vec!["deploy".into(), "notes".into()];
        top.delegates = vec![scout, spec("plain", serde_json::json!(["web_fetch"]))];
        offered(&["deploy", "notes"]).offer(&mut top);
        let names = |s: &AgentSpec| -> Vec<String> {
            s.stored_skills.iter().map(|e| e.name.clone()).collect()
        };
        assert_eq!(names(&top), ["notes"]);
        assert!(top.allows_tool("skill"));
        assert!(names(&top.delegates[0]).is_empty());
        assert!(
            !top.delegates[0].allows_tool("skill"),
            "no files and nothing to load: no tool"
        );
        assert!(names(&top.delegates[1]).is_empty());
        // Nothing offered: an agent with files keeps its tool.
        let mut c = spec("coder", serde_json::json!("all"));
        Offered::default().offer(&mut c);
        assert!(c.allows_tool("skill") && c.stored_skills.is_empty());
    }

    /// A node without the capability gets no list, and an agent that had
    /// `skill` only for the list loses it there.
    #[test]
    fn strip_leaves_no_list_and_no_empty_tool() {
        let mut a = spec("general", serde_json::json!(["skill", "web_fetch"]));
        offered(&["notes"]).offer(&mut a);
        let mut c = spec("coder", serde_json::json!("all"));
        offered(&["notes"]).offer(&mut c);
        a.delegates = vec![c];
        let mut msg = MasterToNode::SetAgent {
            req_id: "r".into(),
            session_id: SessionId::new(),
            agent: a,
        };
        strip(&mut msg);
        let MasterToNode::SetAgent { agent, .. } = msg else {
            unreachable!()
        };
        assert!(agent.stored_skills.is_empty() && !agent.allows_tool("skill"));
        let c = &agent.delegates[0];
        assert!(c.stored_skills.is_empty() && c.allows_tool("skill"));
    }

    /// A server skill wins its name; the workspace's of that name is hidden.
    #[test]
    fn a_server_skill_hides_a_workspace_skill_of_its_name() {
        let row = |ws: Option<&str>, n: &str| StoredSkillRow {
            workspace_id: ws.map(str::to_string),
            name: n.into(),
            description: "d".into(),
            body: String::new(),
            updated_ms: 0,
        };
        let (v, hidden) = layered(
            vec![row(None, "notes")],
            vec![row(Some("w"), "notes"), row(Some("w"), "lint")],
        );
        let got: Vec<(StoredScope, &str)> = v.iter().map(|(s, r)| (*s, r.name.as_str())).collect();
        assert_eq!(
            got,
            [
                (StoredScope::Workspace, "lint"),
                (StoredScope::Server, "notes")
            ]
        );
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].workspace_id.as_deref(), Some("w"));
    }
}
