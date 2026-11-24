//! The branch and the changed-file count of a chat's folder for its header
//! (X.64, PLAN.md T19). The reading is `git::repo_state`: files of the
//! repository read by our own code, the count asked of the node's shadow;
//! no git process runs in the person's repository. This is the session's
//! side: which chats are asked, and what a node that restricts `read` shows.
//! Nothing is appended to the log: it is a view.

use harness_core::event::Profile;
use harness_protocol::GitState;

use super::Session;

impl Session {
    /// The state of the repository this chat's folder is in, `None` for
    /// everything the header shows nothing for: a chat that is not a coding
    /// chat, a sub-agent's own folder (the node made it; it is no
    /// repository of the person's), a node that denies the `read` tool (the
    /// branch is text of the folder: what the agent may not read, the header
    /// does not tell), a folder no longer allowed, no repository the node
    /// reads. With `checkpoints_keep = 0` no checkpoint store is made for
    /// this, so the count is absent and the branch is shown alone.
    pub(crate) async fn git_state(&self) -> Option<GitState> {
        let policy = &self.node.cfg.policy;
        if self.profile != Profile::Agentic || self.owned().is_some() || policy.denies("read") {
            return None;
        }
        let dir = self.checked_folder().ok()?;
        let store = if policy.checkpoints_keep == 0 {
            None
        } else {
            self.node.cfg.checkpoints_dir.as_deref()
        };
        crate::git::repo_state(&dir, store, &self.bounds(&dir)).await
    }
}
