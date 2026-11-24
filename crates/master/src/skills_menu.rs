//! The skills a chat can ask for with `/name`, for the composer's `/` menu
//! (ADOPTION_PLAN X.65, PLAN.md T46), asked of the node that owns the chat.
//! The master resolves the chat's own agent as it does for a prompt (so the
//! node applies the agent's deny lists and its stored skills exactly as a
//! turn would), relays one `session.skills_menu` and hands the one
//! `session.skills_menu_result` that answers it, from that node only, to the
//! waiting request. It reads no folder and runs no program; it keeps
//! nothing: no cache, no log line, nothing in its store. What a node sends
//! is text from a repository someone else controls, so it is bounded again
//! here before it goes on to a page. The visibility check is the caller's
//! (`http/skills_menu.rs`), the same as opening the chat.

use std::collections::HashMap;
use std::time::Duration;

use harness_protocol::{
    CAP_SKILLS_MENU, MAX_MENU_DESCRIPTION_CHARS, MAX_MENU_SKILLS, MasterToNode, MenuSkill,
};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: over the node's own bound on the
/// work (5 s), under the route's limit (`limits.rs`).
const WAIT: Duration = Duration::from_secs(12);
/// Requests waiting at once, all nodes together, and per account.
const MAX_WAITING: usize = 32;
const PER_USER: usize = 2;

enum Answer {
    Skills(Vec<MenuSkill>),
    Busy,
}

/// Requests waiting for nodes' answers, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there is nothing to list, for the route to say or to say nothing of.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuError {
    /// The node is not connected (or went while the request waited).
    Away,
    /// The node is older than this server.
    TooOld,
    /// This server, or the node, answers other requests already.
    Busy,
    /// No answer in time.
    Timeout,
    /// The chat's agent cannot be resolved now.
    NoAgent,
    Unknown,
}

impl From<crate::CommandError> for MenuError {
    fn from(_: crate::CommandError) -> Self {
        MenuError::Unknown
    }
}

/// Removes a request's entry however its wait ends.
struct Entry {
    m: Master,
    req_id: String,
}

impl Drop for Entry {
    fn drop(&mut self) {
        lock(&self.m.inner.skills_menus.0).remove(&self.req_id);
    }
}

/// The skills of `row`'s chat the `/` menu lists, from the node that owns
/// it; `user` is who asks (the API token counts as one).
pub async fn ask(
    m: &Master,
    user: &str,
    row: &harness_store::SessionRow,
) -> Result<Vec<MenuSkill>, MenuError> {
    let sid = row.session_id;
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(MenuError::Away);
    }
    if !m.node_has(&node, CAP_SKILLS_MENU) {
        return Err(MenuError::TooOld);
    }
    // The chat's own agent, resolved as a prompt resolves it.
    let r = m.resolver_for(row.workspace_id.as_deref()).await;
    let session_model = m.session_model(row).await;
    // An agent that is gone (or became sub-agent-only) lists nothing: the
    // prompt itself says why, the menu keeps to the built-in commands.
    let chosen = match r.session_agent(row.agent_id.as_deref()) {
        Ok(Some(id)) => id,
        Ok(None) => match r.default_for(crate::profile_of(row)) {
            Ok(id) => id.to_string(),
            Err(_) => return Err(MenuError::NoAgent),
        },
        Err(_) => return Err(MenuError::NoAgent),
    };
    let agent = r
        .agent_spec_with(Some(&chosen), session_model.as_deref(), None)
        .map_err(|_| MenuError::NoAgent)?;
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.skills_menus.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(MenuError::Busy);
        }
        w.insert(
            req_id.clone(),
            Wait {
                node: node.clone(),
                user: user.to_string(),
                tx,
            },
        );
    }
    let sent = m.send_raw(
        &node,
        MasterToNode::SkillsMenu {
            req_id: req_id.clone(),
            session_id: sid,
            agent,
        },
    );
    if !sent {
        lock(&m.inner.skills_menus.0).remove(&req_id);
        return Err(MenuError::Away);
    }
    // The wait is a task of its own, so a page that gives up does not free
    // the place while the node is still working (as `git_state`).
    let entry = Entry {
        m: m.clone(),
        req_id,
    };
    let wait = tokio::spawn(async move {
        let _entry = entry;
        tokio::time::timeout(WAIT, rx).await
    });
    match wait.await {
        Ok(Ok(Ok(Answer::Skills(s)))) => Ok(bounded(s)),
        Ok(Ok(Ok(Answer::Busy))) => Err(MenuError::Busy),
        Ok(Ok(Err(_))) => Err(MenuError::Away),
        Ok(Err(_)) => Err(MenuError::Timeout),
        Err(_) => Err(MenuError::Unknown),
    }
}

/// What a node sent, held to the shape the menu shows: a name that is a
/// skill's name (1 to 64 of letters, digits, `.`, `_`, `-`) and not a
/// built-in command's, once; a description of at most
/// [`MAX_MENU_DESCRIPTION_CHARS`] visible characters on one line; at most
/// [`MAX_MENU_SKILLS`] entries.
fn bounded(list: Vec<MenuSkill>) -> Vec<MenuSkill> {
    let mut out: Vec<MenuSkill> = Vec::new();
    for s in list {
        if out.len() == MAX_MENU_SKILLS {
            break;
        }
        if !harness_core::skills::valid_skill_name(&s.name)
            || harness_tools::skills::RESERVED.contains(&s.name.as_str())
            || out.iter().any(|o| o.name == s.name)
        {
            continue;
        }
        let mut d = String::new();
        let mut n = 0;
        for c in s.description.chars() {
            if harness_core::text::is_hidden(c) {
                continue;
            }
            let c = if c.is_control() || c.is_whitespace() {
                ' '
            } else {
                c
            };
            if c == ' ' && (d.is_empty() || d.ends_with(' ')) {
                continue;
            }
            if n == MAX_MENU_DESCRIPTION_CHARS {
                break;
            }
            d.push(c);
            n += 1;
        }
        out.push(MenuSkill {
            name: s.name,
            description: d.trim_end().to_string(),
        });
    }
    out
}

/// A `session.skills_menu_result` from `node_id`: to the request it
/// answers, if that request went to this node; dropped otherwise.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, skills: Vec<MenuSkill>, busy: bool) {
    let tx = {
        let mut w = lock(&m.inner.skills_menus.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "skills menu answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(if busy {
            Answer::Busy
        } else {
            Answer::Skills(skills)
        });
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.skills_menus.0).retain(|_, x| x.node != node_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str) -> MenuSkill {
        MenuSkill {
            name: name.into(),
            description: description.into(),
        }
    }

    /// A node that is wrong or hostile cannot hand the page more than the
    /// shape it shows.
    #[test]
    fn what_a_node_sends_is_held_to_the_shape() {
        let mut list = vec![
            skill("ok", "fine"),
            skill("ok", "again"),
            skill("a b", "space"),
            skill("a/b", "slash"),
            skill("x\u{202e}y", "bidi"),
            skill("<script>", "markup"),
            skill("compact", "shadow"),
            skill("", "empty"),
            skill(&"n".repeat(65), "long"),
            skill(
                "desc",
                &format!("a\u{1b}[0m\n\tb\u{200b}c {}", "é".repeat(500)),
            ),
        ];
        list.extend((0..400).map(|i| skill(&format!("s{i}"), "d")));
        let out = bounded(list);
        assert_eq!(out.len(), MAX_MENU_SKILLS);
        assert_eq!(out[0], skill("ok", "fine"));
        assert_eq!(out[1].name, "desc");
        assert!(out[1].description.starts_with("a [0m b"), "{:?}", out[1]);
        assert!(out[1].description.contains("bc "), "{:?}", out[1]);
        assert_eq!(
            out[1].description.chars().count(),
            MAX_MENU_DESCRIPTION_CHARS
        );
        assert!(out.iter().all(|s| s.name != "compact" && s.name != "a b"));
    }
}
