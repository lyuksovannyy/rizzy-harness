//! The skills a chat can ask for with `/name`, for the composer's menu
//! (X.65, PLAN.md T46), and the pieces a turn's discovery shares with it:
//! whether the agent has the `skill` tool here, and which folders are
//! searched. The menu is built by the same discovery, the same
//! `withhold` and `merge_stored` and the same caps as the list the model
//! gets at the start of a turn, so it can neither show a skill the turn
//! would not offer nor hide one it would. It reads each `SKILL.md`'s front
//! matter and nothing else, appends no event and starts no turn.

use std::path::Path;

use harness_core::agent::AgentSpec;
use harness_core::event::Profile;
use harness_protocol::MenuSkill;
use harness_tools::skills;

use super::Session;
use crate::instructions::{self as ins, Bounded};

impl Session {
    /// Whether `agent` (none: the node's default) may use the `skill` tool
    /// in this chat: its own tool list, node.toml's `denied_tools`, and a
    /// backend that has the tool.
    pub(super) fn skill_on(&self, agent: Option<&AgentSpec>) -> bool {
        agent.is_none_or(|a| a.allows_tool(skills::NAME))
            && !self.node.cfg.policy.denies(skills::NAME)
            && self.node.cfg.backend.has_tool(skills::NAME)
    }

    /// The folders a discovery searches for `agent` in `dir` (the chat's
    /// checked folder) under `granted`: node.toml's `[skills] roots`, then
    /// (where `repository` is on) the repository's. Only for an agent that
    /// works on files or runs commands: a skill folder is files (X.34
    /// step B); one without has the stored skills, which are text. The
    /// caller has checked [`Self::skill_on`].
    pub(super) fn skill_roots(
        &self,
        agent: Option<&AgentSpec>,
        dir: &Path,
        granted: &Path,
    ) -> Vec<skills::Root> {
        let policy = &self.node.cfg.policy;
        let mut roots = Vec::new();
        if agent.is_none_or(|a| a.works_on_files()) {
            // Checked when node.toml was loaded; a policy built in code
            // with a bad entry reads none of them.
            roots.extend(
                policy
                    .skills
                    .roots()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|path| skills::Root {
                        path,
                        origin: harness_core::skills::SkillOrigin::Node,
                        bound: None,
                    }),
            );
            if policy.skills.repository {
                roots.extend(skills::repo_roots(dir, granted));
            }
        }
        roots
    }

    /// What the `/` menu lists for this chat when its agent is `agent`
    /// (resolved by the master as for a prompt): the turn's catalogue as
    /// [`harness_tools::skills::menu`] filters it. Empty for an agent
    /// without the `skill` tool, a node that denies it, a folder no longer
    /// allowed, a discovery that did not finish in time. A plain chat (no
    /// folder) lists the master's stored skills only, as its turns do.
    pub(crate) async fn skills_menu(&self, agent: &AgentSpec) -> Vec<MenuSkill> {
        let policy = &self.node.cfg.policy;
        let agent = Some(agent);
        if !self.skill_on(agent) {
            return Vec::new();
        }
        let stored: &[harness_core::skills::StoredSkillEntry] =
            match agent.filter(|_| policy.skills.from_master) {
                Some(a) => &a.stored_skills,
                None => &[],
            };
        let denied = agent.map(|a| a.deny_skills.as_slice()).unwrap_or_default();
        let mut found = skills::Found::default();
        if self.profile == Profile::Agentic {
            let Ok(dir) = self.checked_folder() else {
                return Vec::new();
            };
            let granted = match self.owned() {
                Some(o) => o.top.clone(),
                None => match policy.root_of(&dir) {
                    Ok(r) => r,
                    Err(_) => return Vec::new(),
                },
            };
            let roots = self.skill_roots(agent, &dir, &granted);
            if !roots.is_empty() {
                let protected = policy.protected_dirs.clone();
                match ins::bounded(
                    move || skills::discover(&roots, &protected),
                    &tokio_util::sync::CancellationToken::new(),
                    ins::READ_TIMEOUT,
                )
                .await
                {
                    Bounded::Done(f) => found = f,
                    _ => return Vec::new(),
                }
            }
        }
        skills::withhold(&mut found.catalogue, denied);
        skills::merge_stored(&mut found.catalogue, stored, denied);
        skills::menu(&found.catalogue)
            .into_iter()
            .map(|(name, description)| MenuSkill { name, description })
            .collect()
    }
}
