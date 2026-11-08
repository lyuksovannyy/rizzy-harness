//! What a routine runs (`RoutineSpec`, stored as JSON in `routines.spec`),
//! and the checks a saved one passes. Requests are strict (an unknown field
//! is an error); stored specs are read leniently, so a field a later build
//! drops never makes a routine unreadable.

use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Master;
use crate::workspaces::Access;

/// The longest prompt template.
pub const MAX_PROMPT_TEMPLATE: usize = 16_000;
/// The longest run, in minutes.
pub const MAX_RUNTIME_MIN: u32 = 1440;
/// The longest routine name.
const MAX_NAME: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutineSpec {
    pub profile: Profile,
    /// Explicit, so it beats a leading @mention in the prompt.
    pub agent_id: String,
    /// `None`: the agent's default in the routine's workspace.
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Agentic only.
    pub node_id: Option<String>,
    pub cwd: Option<String>,
    /// `ask` or `ask_all`; nothing runs without a click (agentic).
    pub approval_mode: ApprovalMode,
    /// The template (`template.rs`).
    pub prompt: String,
    /// The project the run's chat lands in (one in the routine's workspace).
    pub project_id: Option<String>,
    pub catch_up: CatchUp,
    pub on_missing: OnMissing,
    pub notify: Notify,
    pub max_runtime_min: u32,
}

impl Default for RoutineSpec {
    fn default() -> Self {
        Self {
            profile: Profile::Chat,
            agent_id: String::new(),
            model: None,
            effort: None,
            node_id: None,
            cwd: None,
            approval_mode: ApprovalMode::Ask,
            prompt: String::new(),
            project_id: None,
            catch_up: CatchUp::default(),
            on_missing: OnMissing::default(),
            notify: Notify::default(),
            max_runtime_min: 120,
        }
    }
}

/// What happens to occurrences missed while the server was down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatchUp {
    /// Record them as one `missed` run.
    #[default]
    Skip,
    /// Run once, late.
    Once,
}

/// What a variable without a value does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnMissing {
    /// Renders as nothing, with a warning on the run.
    #[default]
    Empty,
    /// The run is recorded as `skipped`.
    Skip,
}

/// Which results raise a browser notification (attention rows always
/// appear).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    Always,
    #[default]
    Failures,
    Never,
}

impl RoutineSpec {
    /// A stored spec; `None` when it is not JSON at all.
    pub fn read(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    pub fn agentic(&self) -> bool {
        self.profile == Profile::Agentic
    }

    /// A spec from a request: unknown fields are refused, every value is
    /// checked on its own (what needs the workspace is `check`'s), and the
    /// result is normalized (blank optional strings become `None`).
    pub fn parse(v: Value) -> Result<Self, String> {
        let mut s: Self = serde_json::from_value(v.clone()).map_err(|e| format!("spec: {e}"))?;
        strict_keys("spec", &v, &s)?;
        let blank = |o: &mut Option<String>| {
            if o.as_deref().is_some_and(|x| x.trim().is_empty()) {
                *o = None;
            }
        };
        blank(&mut s.model);
        blank(&mut s.effort);
        blank(&mut s.node_id);
        blank(&mut s.cwd);
        blank(&mut s.project_id);
        s.agent_id = s.agent_id.trim().to_string();
        if s.agent_id.is_empty() {
            return Err("spec.agent_id: choose the agent that runs the routine".into());
        }
        if let Some(e) = &s.effort
            && !matches!(e.as_str(), "low" | "medium" | "high" | "max")
        {
            return Err(format!("spec.effort: unknown level {e:?}"));
        }
        if s.agentic() {
            if s.node_id.is_none() || s.cwd.is_none() {
                return Err("spec: an agentic routine needs a node_id and a cwd".into());
            }
            if s.approval_mode == ApprovalMode::Auto {
                return Err(
                    "spec.approval_mode: routines never approve on their own; use ask or ask_all"
                        .into(),
                );
            }
        } else if s.node_id.is_some() || s.cwd.is_some() {
            return Err("spec: node_id and cwd are for agentic routines only".into());
        } else {
            // Chats run on the chat runtime and approve nothing.
            s.approval_mode = ApprovalMode::Ask;
        }
        if s.cwd.as_ref().is_some_and(|c| c.len() > 4096) {
            return Err("spec.cwd is too long".into());
        }
        if s.prompt.trim().is_empty() {
            return Err("spec.prompt: write what the routine should ask".into());
        }
        if s.prompt.chars().count() > MAX_PROMPT_TEMPLATE {
            return Err(format!(
                "spec.prompt: at most {MAX_PROMPT_TEMPLATE} characters"
            ));
        }
        if !(1..=MAX_RUNTIME_MIN).contains(&s.max_runtime_min) {
            return Err(format!(
                "spec.max_runtime_min: between 1 and {MAX_RUNTIME_MIN}"
            ));
        }
        Ok(s)
    }
}

/// Refuse fields `T` does not have: every field of the parsed value is
/// serialized, so a key of `input` missing from it is unknown.
pub(crate) fn strict_keys<T: Serialize>(
    what: &str,
    input: &Value,
    parsed: &T,
) -> Result<(), String> {
    let known = serde_json::to_value(parsed).map_err(|e| e.to_string())?;
    let (Some(input), Some(known)) = (input.as_object(), known.as_object()) else {
        return Err(format!("{what} must be an object"));
    };
    match input.keys().find(|k| !known.contains_key(*k)) {
        Some(k) => Err(format!("{what}: unknown field {k:?}")),
        None => Ok(()),
    }
}

/// A routine name: trimmed, 1-100 characters, no control characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("give the routine a name".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("a routine name has at most {MAX_NAME} characters"));
    }
    if name.chars().any(char::is_control) {
        return Err("a routine name cannot hold control characters".into());
    }
    Ok(name.to_string())
}

/// What in `spec` does not exist in workspace `a` (an agent, a model, a
/// node or a project): checked at save. The same things are checked again
/// at every fire, where they block the routine (`runner::why_blocked`).
pub async fn check(m: &Master, a: &Access, spec: &RoutineSpec) -> Result<(), String> {
    if let Some(why) = missing(m, a.id(), spec).await {
        return Err(why);
    }
    if let Some(pid) = spec.project_id.clone() {
        let project = m
            .inner
            .mirror
            .call(move |s| s.project(&pid))
            .await
            .map_err(|e| e.to_string())?;
        if project.is_none_or(|p| p.workspace_id != a.id()) {
            return Err("spec.project_id: no such project in this workspace".into());
        }
    }
    Ok(())
}

/// The first thing `spec` names that workspace `ws` does not have now.
pub(crate) async fn missing(m: &Master, ws: &str, spec: &RoutineSpec) -> Option<String> {
    let r = m.resolver_for(Some(ws)).await;
    if r.agents().get(&spec.agent_id).is_none() {
        return Some(format!("the agent {:?} no longer exists", spec.agent_id));
    }
    if let Some(model) = &spec.model
        && r.find_model(model).is_none()
    {
        return Some(format!("the model {model:?} is not available"));
    }
    if let Some(node) = &spec.node_id {
        return node_problem(m, node).await;
    }
    None
}

/// Why agentic runs cannot go to `node`: it is not a worker node, or it was
/// revoked or never joined. An offline node is fine (runs retry).
async fn node_problem(m: &Master, node: &str) -> Option<String> {
    if let Some(n) = m.nodes().into_iter().find(|n| n.node_id == node) {
        return (n.profile != Profile::Agentic)
            .then(|| format!("{node} does not run agentic sessions"));
    }
    let registered = m
        .inner
        .mirror
        .call(|s| s.registered_nodes())
        .await
        .unwrap_or_default();
    match registered.into_iter().find(|n| n.node_id == node) {
        Some(n) if n.revoked_ms.is_none() => None,
        Some(_) => Some(format!("the node {node} was revoked")),
        None => Some(format!("there is no node {node}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat() -> Value {
        json!({"profile": "chat", "agent_id": "general", "prompt": "Summarise the news"})
    }

    #[test]
    fn defaults_and_normalizing() {
        let s = RoutineSpec::parse(chat()).unwrap();
        assert_eq!(s.catch_up, CatchUp::Skip);
        assert_eq!(s.on_missing, OnMissing::Empty);
        assert_eq!(s.notify, Notify::Failures);
        assert_eq!(s.max_runtime_min, 120);
        assert_eq!(s.model, None);
        let mut v = chat();
        v["model"] = json!("  ");
        v["node_id"] = json!("");
        assert_eq!(RoutineSpec::parse(v).unwrap().model, None);
    }

    #[test]
    fn requests_are_strict() {
        let mut v = chat();
        v["trusted_tools"] = json!(["bash"]);
        let e = RoutineSpec::parse(v).unwrap_err();
        assert!(e.contains("trusted_tools"), "{e}");
        let mut v = chat();
        v["notify"] = json!("sometimes");
        assert!(RoutineSpec::parse(v).is_err());
        assert!(RoutineSpec::parse(json!({"profile": "chat", "prompt": "x"})).is_err());
        assert!(
            RoutineSpec::parse(json!({"profile": "chat", "agent_id": "g", "prompt": " "})).is_err()
        );
        let mut v = chat();
        v["prompt"] = json!("x".repeat(MAX_PROMPT_TEMPLATE + 1));
        assert!(RoutineSpec::parse(v).is_err());
        let mut v = chat();
        v["max_runtime_min"] = json!(0);
        assert!(RoutineSpec::parse(v).is_err());
        let mut v = chat();
        v["node_id"] = json!("local");
        assert!(RoutineSpec::parse(v).is_err(), "chats run on no node");
    }

    #[test]
    fn agentic_needs_a_place_and_never_auto() {
        let v = json!({"profile": "agentic", "agent_id": "coder", "prompt": "review",
                       "node_id": "local", "cwd": "/srv/app"});
        let s = RoutineSpec::parse(v.clone()).unwrap();
        assert_eq!(s.approval_mode, ApprovalMode::Ask);
        let mut all = v.clone();
        all["approval_mode"] = json!("ask_all");
        assert_eq!(
            RoutineSpec::parse(all).unwrap().approval_mode,
            ApprovalMode::AskAll
        );
        let mut auto = v.clone();
        auto["approval_mode"] = json!("auto");
        assert!(RoutineSpec::parse(auto).unwrap_err().contains("ask"));
        let mut nowhere = v;
        nowhere["cwd"] = Value::Null;
        assert!(RoutineSpec::parse(nowhere).is_err());
    }

    #[test]
    fn stored_specs_are_read_leniently() {
        let s = RoutineSpec::read(r#"{"agent_id":"general","prompt":"hi","dropped":1}"#).unwrap();
        assert_eq!(s.agent_id, "general");
        assert_eq!(s.profile, Profile::Chat);
        assert!(RoutineSpec::read("not json").is_none());
    }

    #[test]
    fn names() {
        assert_eq!(clean_name("  Morning news ").unwrap(), "Morning news");
        assert!(clean_name(" ").is_err());
        assert!(clean_name("a\u{7}b").is_err());
        assert!(clean_name(&"n".repeat(101)).is_err());
    }
}
