//! A delegate whose declared tools are none of those registered and allowed
//! on this node is not offered and is refused by name; one that declares no
//! tools on purpose, or has some of them, is unaffected.

use super::*;
use harness_core::SessionId;
use harness_core::agent::{AgentSpec, AllTools, ToolSelection};
use harness_core::backend::TurnHost;
use harness_core::policy::{Decision, ToolRisk};
use provider::{ChatRequest, Completion, ProviderError};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Host {
    events: Mutex<Vec<EventKind>>,
    started: AtomicUsize,
    denied: Vec<&'static str>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn tool_denied(&self, tool: &str) -> bool {
        self.denied.contains(&tool)
    }
    async fn delegate(
        &self,
        agent: AgentSpec,
        _task: String,
        _refs: SharedRefs,
        _observed: Arc<Vec<String>>,
        _cancel: tokio_util::sync::CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let child = SessionId::new();
        started(child);
        Ok(harness_core::backend::DelegateOutcome {
            child,
            text: format!("{} ran", agent.agent_id),
            reason: TurnEndReason::Completed,
            usage: None,
            sources: vec![],
            finish: None,
            error: None,
            note: None,
            unapplied: None,
            applied: true,
        })
    }
}

struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl provider::Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.reqs.lock().unwrap().push(req.clone());
        mock::Mock {
            delay: Duration::from_millis(1),
        }
        .complete(req, deltas)
        .await
    }
}

fn spec(id: &str, tools: ToolSelection) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "p".into(),
        tools,
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn list(names: &[&str]) -> ToolSelection {
    ToolSelection::List(names.iter().map(|s| s.to_string()).collect())
}

struct Fake(&'static str);

#[async_trait::async_trait]
impl harness_tools::Tool for Fake {
    fn name(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        "fake"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &harness_tools::ToolCtx, _: Value) -> ToolOutcome {
        ToolOutcome::err("unused")
    }
}

/// Runs one turn of a lead that may delegate to `delegates`, on a node
/// whose registry is the builtin tools plus `extra`, the model saying
/// `prompt`. Returns the delegate ids the model was offered, the sub-agents
/// started, and the tool results.
async fn run(
    extra: &[&'static str],
    denied: Vec<&'static str>,
    delegates: Vec<AgentSpec>,
    prompt: &str,
) -> (Vec<String>, usize, Vec<String>) {
    let (_, offered, started, outs) = run_seen(extra, denied, delegates, prompt).await;
    (offered, started, outs)
}

/// [`run`], also returning the tools of the first request as the model
/// received them (name and description).
async fn run_seen(
    extra: &[&'static str],
    denied: Vec<&'static str>,
    delegates: Vec<AgentSpec>,
    prompt: &str,
) -> (Vec<(String, String)>, Vec<String>, usize, Vec<String>) {
    run_seen_as(
        ToolSelection::All(AllTools::All),
        extra,
        denied,
        delegates,
        prompt,
    )
    .await
}

/// [`run_seen`] for a lead that has `lead_tools`.
async fn run_seen_as(
    lead_tools: ToolSelection,
    extra: &[&'static str],
    denied: Vec<&'static str>,
    delegates: Vec<AgentSpec>,
    prompt: &str,
) -> (Vec<(String, String)>, Vec<String>, usize, Vec<String>) {
    let mut tools = harness_tools::builtin();
    tools.extend(
        extra
            .iter()
            .map(|n| Arc::new(Fake(n)) as Arc<dyn harness_tools::Tool>),
    );
    let rec = Arc::new(Rec {
        reqs: Mutex::default(),
    });
    let backend = NativeBackend::new(rec.clone(), tools, NativeConfig::default());
    let host = Host {
        events: Mutex::default(),
        started: AtomicUsize::new(0),
        denied,
    };
    let dir = tempfile::tempdir().unwrap();
    backend
        .run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: prompt.into(),
            cwd: dir.path().to_path_buf(),
            model: "mock".into(),
            agent: Some(AgentSpec {
                delegates,
                ..spec("lead", lead_tools)
            }),
            compare: vec![],
            parent_session: None,
            extra: Default::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    let offered = rec.reqs.lock().unwrap()[0]
        .tools
        .iter()
        .find(|t| t.name == DELEGATE)
        .map(|t| {
            t.schema["properties"]["agent"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    let outs = host
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f.output.clone()),
            _ => None,
        })
        .collect();
    let seen = rec.reqs.lock().unwrap()[0]
        .tools
        .iter()
        .map(|t| (t.name.clone(), t.description.clone()))
        .collect();
    (seen, offered, host.started.load(Ordering::SeqCst), outs)
}

fn researcher() -> AgentSpec {
    spec("researcher", list(&["web_search", "web_fetch", "remember"]))
}

#[tokio::test]
async fn a_delegate_with_no_available_tool_is_not_offered_and_is_refused() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) = run(&[], vec![], all, "delegate: researcher=ask").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0, "no child session");
    assert_eq!(
        outs,
        [
            "@researcher was not started: none of its tools (web search, web fetch, memory) \
          are available on this node. Do the work another way or tell the user."
        ]
    );
}

#[tokio::test]
async fn in_the_chat_runtime_the_researcher_is_offered_and_runs() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) = run(
        &["web_search", "web_fetch", "remember"],
        vec![],
        all,
        "delegate: researcher=ask",
    )
    .await;
    assert_eq!(offered, ["researcher", "scout"]);
    assert_eq!(started, 1);
    assert!(outs[0].contains("researcher ran"), "{outs:?}");
}

#[tokio::test]
async fn an_agent_that_declares_no_tools_stays_offered_and_runs() {
    let all = vec![spec("smol", list(&[]))];
    let (offered, started, _) = run(&[], vec![], all, "delegate: smol=rephrase").await;
    assert_eq!(offered, ["smol"]);
    assert_eq!(started, 1);
}

#[tokio::test]
async fn some_available_tools_is_enough_and_none_is_refused() {
    let all = vec![
        spec("half", list(&["read", "web_search"])),
        spec("none", list(&["web_search", "web_fetch"])),
    ];
    let (offered, started, outs) = run(&[], vec![], all, "delegate: half=a | none=b").await;
    assert_eq!(offered, ["half"]);
    assert_eq!(started, 1);
    assert_eq!(outs.len(), 2, "{outs:?}");
    assert!(outs.iter().any(|o| o.contains("half ran")), "{outs:?}");
    assert!(
        outs.iter()
            .any(|o| o.starts_with("@none was not started") && o.contains("web search, web fetch")),
        "{outs:?}"
    );
}

/// On an agentic node that offers `remember` but no web tools, Researcher
/// is still not offered: proposing a memory does none of a task's work, so
/// it alone makes no delegate available (A.11 holds once nodes register it).
#[tokio::test]
async fn memory_alone_does_not_make_a_delegate_available() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) =
        run(&["remember"], vec![], all, "delegate: researcher=ask").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0, "no child session");
    assert!(
        outs[0].starts_with("@researcher was not started"),
        "{outs:?}"
    );
}

/// The node's own `denied_tools` count as "not available there".
#[tokio::test]
async fn a_tool_the_node_denies_does_not_count() {
    let all = vec![
        spec("shell", list(&["bash"])),
        spec("scout", list(&["read"])),
    ];
    let (offered, started, outs) = run(&[], vec!["bash"], all, "delegate: shell=go").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0);
    assert!(outs[0].starts_with("@shell was not started"), "{outs:?}");
}

// ---- What the model reads about its tools (live test, 2026-11-15) ----

/// The description of `tool` as the model received it in a lead's turn.
async fn described(tool: &str) -> String {
    let (seen, ..) = run_seen(&[], vec![], vec![spec("scout", list(&["read"]))], "hello").await;
    seen.into_iter()
        .find(|(n, _)| n == tool)
        .map(|(_, d)| d)
        .unwrap_or_else(|| panic!("`{tool}` was not offered"))
}

/// Delegations of one step run at the same time on separate copies: the
/// model is told to start only independent work together.
#[tokio::test]
async fn the_delegate_description_says_one_steps_children_do_not_see_each_other() {
    let d = described(DELEGATE).await;
    assert!(d.contains(DELEGATE_PARALLEL), "{d}");
    assert!(d.contains("do not see each other's changes"), "{d}");
    assert!(
        d.contains("in a later step, after that result is back"),
        "{d}"
    );
}

/// The read-first guard (PLAN.md T45) in the words the model gets: a file
/// is read with `read` before it is changed, and a `cat` in `bash` does
/// not count (the live test lost steps to edits refused after one).
#[tokio::test]
async fn the_file_tools_say_read_comes_first_and_cat_does_not_count() {
    for tool in ["bash", "read", "edit", "write"] {
        let d = described(tool).await;
        assert!(d.contains("`read`") && d.contains("`cat`"), "{tool}: {d}");
    }
    assert!(described("bash").await.contains(
        "To look at a file you mean to change, use `read`, not `cat` or `sed` here: `edit` and \
         `write` accept only a file read with `read`."
    ));
    assert!(described("read").await.contains("in `bash` do not count"));
}

/// `bash` already starts in the chat's folder (the live test: 36 calls
/// began with `cd <that folder> &&`): the model is told so, and that
/// `workdir` runs a command elsewhere.
#[tokio::test]
async fn bash_says_it_already_runs_in_the_chats_folder() {
    let d = described("bash").await;
    assert!(
        d.contains(
            "It already runs in the chat's folder: do not start it with `cd` to that folder; to \
             run it elsewhere, set `workdir`."
        ),
        "{d}"
    );
    assert_eq!(
        harness_tools::FOREGROUND_DESCRIPTION
            .split(". ")
            .nth(1)
            .map(|s| s.starts_with("It already runs in the chat's folder")),
        Some(true)
    );
}

/// The model is told it can give a finished sub-agent a follow-up (T68):
/// the `delegate` description says when and how, the schema offers
/// `session`, and a malformed id is refused before anything starts (the
/// host's default, as for a backend without follow-ups, refuses the rest).
#[tokio::test]
async fn delegate_offers_follow_ups() {
    let all = vec![spec("scout", list(&["read"]))];
    let (seen, _, _, _) = run_seen(&[], vec![], all.clone(), "hello").await;
    let (_, desc) = seen
        .iter()
        .find(|(n, _)| n == DELEGATE)
        .expect("delegate offered");
    assert!(desc.contains(DELEGATE_FOLLOW_UP), "{desc}");
    assert!(desc.contains("`session`"), "{desc}");
    let schema = {
        let rec = Arc::new(Rec {
            reqs: Mutex::default(),
        });
        let backend = NativeBackend::new(
            rec.clone(),
            harness_tools::builtin(),
            NativeConfig::default(),
        );
        let host = Host {
            events: Mutex::default(),
            started: AtomicUsize::new(0),
            denied: vec![],
        };
        let dir = tempfile::tempdir().unwrap();
        backend
            .run_turn(TurnCtx {
                session_id: SessionId::new(),
                turn_id: "t".into(),
                history: vec![],
                user_text: "hello".into(),
                cwd: dir.path().to_path_buf(),
                model: "mock".into(),
                agent: Some(AgentSpec {
                    delegates: all.clone(),
                    ..spec("lead", ToolSelection::All(AllTools::All))
                }),
                compare: vec![],
                parent_session: None,
                extra: Default::default(),
                cancel: Default::default(),
                host: &host,
            })
            .await;
        let reqs = rec.reqs.lock().unwrap();
        reqs[0]
            .tools
            .iter()
            .find(|t| t.name == DELEGATE)
            .unwrap()
            .schema
            .clone()
    };
    assert_eq!(schema["properties"]["session"]["type"], "string");
    assert_eq!(schema["required"], json!(["agent", "task"]));
    let (_, started, outs) = run(&[], vec![], all.clone(), "delegate: scout@not-an-id=more").await;
    assert_eq!(started, 0);
    assert!(outs[0].contains("`session` must be the id"), "{outs:?}");
    // A host without follow-ups (this test's) refuses a well-formed one;
    // no new child is started in its place.
    let id = SessionId::new();
    let (_, started, outs) = run(&[], vec![], all, &format!("delegate: scout@{id}=more")).await;
    assert_eq!(started, 0);
    assert!(outs[0].contains("not available here"), "{outs:?}");
}

/// `bash` as a lead with `tools` is offered it, when `denied` are the
/// node's denied tools.
async fn bash_for(tools: &[&str], denied: Vec<&'static str>) -> Option<String> {
    let (seen, ..) = run_seen_as(list(tools), &[], denied, vec![], "hello").await;
    seen.into_iter().find(|(n, _)| n == "bash").map(|(_, d)| d)
}

const USE_FILE_TOOLS: &str = "To create or change a file use";

/// The real-use trial (2026-11-15): 0 `write` or `edit` calls in 53 tool
/// calls, every file written with `cat > f <<EOF` or `python3 -`, so no
/// diff and an "Accept edits" chat still asked for each command. An agent
/// that has the file tools is told to change files with them ...
#[tokio::test]
async fn bash_tells_an_agent_with_file_tools_to_change_files_with_them() {
    let all = described("bash").await;
    assert!(all.contains(USE_FILE_TOOLS), "{all}");
    assert!(all.contains("`write` or `edit`"), "{all}");
    let one = bash_for(&["bash", "edit"], vec![]).await.unwrap();
    assert!(
        one.contains("`edit`") && !one.contains("`write` or"),
        "{one}"
    );
}

/// ... and one that has neither (it changes things only through `publish`,
/// so its shell is the writable one and `bash` IS offered), or whose node
/// denies them, is not told to use tools it does not have. (A scout or a
/// reviewer gets the read-only shell, or no `bash` where the node has
/// none: the sentence is only in the writable branch of `shell_text`.)
#[tokio::test]
async fn bash_does_not_name_file_tools_an_agent_lacks() {
    let d = bash_for(&["read", "bash", "publish"], vec![])
        .await
        .expect("bash is offered to an agent that has it and changes things");
    assert!(d.contains("Run a shell command with bash"), "{d}");
    assert!(!d.contains(USE_FILE_TOOLS), "{d}");
    assert!(!d.contains("`write` or"), "{d}");
    let d = bash_for(
        &["read", "write", "edit", "bash", "publish"],
        vec!["write", "edit"],
    )
    .await
    .expect("bash is offered when the node denies write and edit");
    assert!(!d.contains(USE_FILE_TOOLS), "{d}");
}
