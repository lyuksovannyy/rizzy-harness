//! MCP tools through the real loop (ADOPTION_PLAN X.3): the host's servers
//! (`TurnHost::mcp`, from node.toml alone) are offered to the model beside
//! the built-in tools; a call goes through the approval path as a mutating
//! call whatever the server claims, and its result comes back as tool
//! output; an agent that may use no MCP tool starts no server; a name no
//! server of the host offers is an unknown tool, whatever the model says.
//! The server is `tools/tests/support/fake_mcp.py`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ToolCallRef};
use harness_core::policy::{Decision, ToolRisk};
use harness_tools::mcp::{ServerConfig, SessionMcp};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

struct Host {
    events: Mutex<Vec<EventKind>>,
    asked: Mutex<Vec<(String, ToolRisk)>>,
    mcp: Option<Arc<SessionMcp>>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, tool: &str, _: &Value, risk: ToolRisk) -> Decision {
        self.asked.lock().unwrap().push((tool.into(), risk));
        Decision::Allow
    }
    fn mcp(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.mcp
            .clone()
            .map(|m| m as Arc<dyn std::any::Any + Send + Sync>)
    }
}

impl Host {
    fn results(&self) -> Vec<(bool, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some((f.ok, f.output.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Calls `calls` first, answers after a tool result; keeps every request.
struct Script {
    calls: Vec<(&'static str, Value)>,
    seen: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let tool_calls = if matches!(req.messages.last(), Some(ChatMsg::Tool { .. })) {
            vec![]
        } else {
            self.calls
                .iter()
                .map(|(tool, input)| ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: (*tool).into(),
                    input: input.clone(),
                })
                .collect()
        };
        Ok(Completion {
            text: "done".into(),
            tool_calls,
            usage: None,
            stop: Default::default(),
        })
    }
}

fn servers(started: &Path) -> Arc<SessionMcp> {
    let mut m = BTreeMap::new();
    m.insert(
        "fake".to_string(),
        ServerConfig::new("python3", &[FAKE, "--started", started.to_str().unwrap()]),
    );
    SessionMcp::new(&m).unwrap()
}

fn turn<'a>(
    host: &'a Host,
    cwd: &Path,
    agent: Option<harness_core::agent::AgentSpec>,
) -> TurnCtx<'a> {
    TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "go".into(),
        cwd: cwd.to_path_buf(),
        model: "x".into(),
        agent,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

fn agent(tools: &[&str]) -> harness_core::agent::AgentSpec {
    serde_json::from_value(json!({
        "agent_id": "a", "name": "A", "system_prompt": "", "prompt_hash": "h",
        "tools": tools,
    }))
    .unwrap()
}

#[tokio::test]
async fn mcp_tools_reach_the_model_and_a_call_asks_and_returns_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let host = Host {
        events: Mutex::default(),
        asked: Mutex::default(),
        mcp: Some(servers(&started)),
    };
    let script = Arc::new(Script {
        calls: vec![("mcp__fake__echo", json!({"text": "from the server"}))],
        seen: Mutex::default(),
    });
    let backend = NativeBackend::new(
        script.clone(),
        harness_tools::builtin(),
        NativeConfig::default(),
    );
    backend
        .run_turn(turn(&host, dir.path(), Some(agent(&["read", "mcp"]))))
        .await;
    let seen = script.seen.lock().unwrap();
    let names: Vec<&str> = seen[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"read"), "{names:?}");
    assert!(names.contains(&"mcp__fake__echo"), "{names:?}");
    assert!(!names.contains(&"bash"), "{names:?}");
    let echo = seen[0]
        .tools
        .iter()
        .find(|t| t.name == "mcp__fake__echo")
        .unwrap();
    assert!(echo.description.contains("Repeat the text."));
    // Asked as a mutating call, although the server says `readOnlyHint`.
    assert_eq!(
        *host.asked.lock().unwrap(),
        [("mcp__fake__echo".to_string(), ToolRisk::Mutating)]
    );
    assert_eq!(host.results(), [(true, "from the server".to_string())]);
    // The model gets it as a tool result, like any other.
    let last = seen.last().unwrap();
    assert!(
        last.messages
            .iter()
            .any(|m| matches!(m, ChatMsg::Tool { content, .. } if content == "from the server")),
        "{:?}",
        last.messages
    );
}

#[tokio::test]
async fn an_agent_without_mcp_starts_no_server_and_unknown_names_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let host = Host {
        events: Mutex::default(),
        asked: Mutex::default(),
        mcp: Some(servers(&started)),
    };
    let script = Arc::new(Script {
        calls: vec![("mcp__fake__echo", json!({"text": "x"}))],
        seen: Mutex::default(),
    });
    let backend = NativeBackend::new(
        script.clone(),
        harness_tools::builtin(),
        NativeConfig::default(),
    );
    backend
        .run_turn(turn(&host, dir.path(), Some(agent(&["read"]))))
        .await;
    assert!(
        !started.exists(),
        "a server started for an agent without mcp"
    );
    let names: Vec<String> = script.seen.lock().unwrap()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert!(!names.iter().any(|n| n.starts_with("mcp__")), "{names:?}");
    let r = host.results();
    assert!(!r[0].0 && r[0].1.contains("unknown tool"), "{r:?}");
    assert!(host.asked.lock().unwrap().is_empty());

    // A server node.toml does not name cannot be reached by naming it.
    let script = Arc::new(Script {
        calls: vec![("mcp__evil__run", json!({"command": "id"}))],
        seen: Mutex::default(),
    });
    let host2 = Host {
        events: Mutex::default(),
        asked: Mutex::default(),
        mcp: Some(servers(&started)),
    };
    let backend = NativeBackend::new(script, harness_tools::builtin(), NativeConfig::default());
    backend.run_turn(turn(&host2, dir.path(), None)).await;
    let r = host2.results();
    assert!(
        !r[0].0 && r[0].1.contains("unknown tool `mcp__evil__run`"),
        "{r:?}"
    );
    assert!(host2.asked.lock().unwrap().is_empty());
}
