//! Stored skills on the node (X.34 step B, PLAN.md T81): loading one
//! becomes one `skill.request` for the session the call runs in (a name and
//! nothing else), in order after the events before it, so the master has
//! seen the turn start; the body comes back as `skill.result`. A slow or
//! absent master is a plain tool error after [`WAIT`]; nothing is retried.
//! Only a skill the turn's catalogue lists as stored is asked for: the
//! tool looks it up there first, and the catalogue takes the master's list
//! only where node.toml `[skills] from_master` is on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::skills::StoredSkillSource;
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a load waits for the master.
pub const WAIT: Duration = Duration::from_secs(20);
/// Loads waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;
/// The longest body taken from the master (bytes): the tool shows at most
/// `harness_tools::skills::BODY_CAP` of it; a longer answer is refused.
const MAX_BODY: usize = 2 * harness_tools::skills::BODY_CAP;

type Reply = oneshot::Sender<Result<String, String>>;

/// The loads waiting for their `skill.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// A `skill.result` arrived: hand it to its waiting call. An unknown id
    /// (late, after the wait gave up, answered already, or never asked) is
    /// dropped.
    pub(crate) fn answer(&self, req_id: &str, body: Option<String>, error: Option<String>) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match (body, error) {
                (_, Some(e)) => Err(harness_core::text::clean_line(&e, 300)),
                (Some(b), None) if b.len() > MAX_BODY => {
                    Err("the server sent more than a skill may hold".into())
                }
                (Some(b), None) => Ok(b),
                (None, None) => Err("the server sent an empty answer".into()),
            });
        }
    }

    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// One session's way to the master, handed to its tool calls.
pub(crate) struct LinkStoredSkills {
    pub(crate) node: Arc<Inner>,
    pub(crate) session: SessionId,
}

/// Removes a waiting call however it ends.
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

#[async_trait::async_trait]
impl StoredSkillSource for LinkStoredSkills {
    async fn load(&self, name: &str) -> Result<String, String> {
        if !self.node.cfg.policy.skills.from_master {
            return Err("this node does not take skills from the server".into());
        }
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self
                .node
                .stored_skills
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err("too many skills are waiting for the server; try again".into());
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting(&self.node.stored_skills, req_id.clone());
        let send = self.node.publish_wait(NodeToMaster::Skill {
            req_id,
            session_id: self.session,
            name: name.to_string(),
        });
        let asked = async {
            send.await;
            rx.await
        };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("the server did not answer".into()),
            Err(_) => Err(format!(
                "the server did not answer within {} seconds",
                WAIT.as_secs()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repeated or unknown `req_id` is dropped: only the first answer to
    /// a waiting load counts, and an answer nobody waits for changes
    /// nothing.
    #[tokio::test]
    async fn only_the_first_answer_to_a_waiting_load_counts() {
        let p = Pending::default();
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("r".into(), tx);
        p.answer("r", Some("first".into()), None);
        p.answer("r", Some("second".into()), None);
        p.answer("other", Some("x".into()), None);
        assert_eq!(rx.await.unwrap(), Ok("first".into()));
        assert_eq!(p.waiting(), 0);
    }

    /// A body past the node's own bound is refused, not cut; an error is
    /// one clean line.
    #[tokio::test]
    async fn an_oversized_body_is_refused() {
        let p = Pending::default();
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("r".into(), tx);
        p.answer("r", Some("x".repeat(MAX_BODY + 1)), None);
        assert!(rx.await.unwrap().unwrap_err().contains("more than"));
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("e".into(), tx);
        p.answer("e", None, Some("no\n[harness] forged".into()));
        assert_eq!(rx.await.unwrap(), Err("no [harness] forged".into()));
    }
}
