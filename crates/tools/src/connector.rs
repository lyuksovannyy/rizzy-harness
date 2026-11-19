//! A workspace's custom connectors as MCP tools of a turn (ADOPTION_PLAN
//! X.35 step 2, PLAN.md T85): the master's list
//! (`AgentSpec::connector_tools`) offered beside the chat's own node.toml
//! servers, under the same per-chat caps ([`crate::mcp::MAX_TOOLS`] tools
//! and [`crate::mcp::MAX_TOOL_TEXT`] bytes for both together, node servers
//! first), and a call filed with the master ([`ConnectorSource`]). The node
//! starts nothing and reads nothing for one: a call is a request over the
//! link, and only its text comes back.
//!
//! A connector tool is an MCP tool for every other rule: its name is
//! `mcp__<connector>__<tool>` (a node server's tool keeps a name both
//! would have: the connector's is left out and counted), its risk is
//! [`ToolRisk::Mutating`] (asked in `ask`, refused in plan mode, whatever
//! the server says about it), and its result is tool output like any
//! other (capped, scrubbed, quoted by the loop).

use std::collections::HashSet;
use std::sync::Arc;

use harness_core::connectors::{ConnectorSource, ConnectorToolEntry, MAX_LISTED};
use harness_core::policy::ToolRisk;
use serde_json::Value;

use crate::mcp::{MAX_TOOL_TEXT, MAX_TOOLS};
use crate::{Tool, ToolCtx, ToolOutcome};

/// One connector tool, as offered to the model.
pub struct ConnectorTool {
    entry: ConnectorToolEntry,
    schema: Value,
    source: Arc<dyn ConnectorSource>,
}

impl ConnectorTool {
    /// The connector and the server's own name of the tool (the approval
    /// card shows them).
    pub fn origin(&self) -> (&str, &str) {
        (&self.entry.connector, &self.entry.tool)
    }
}

#[async_trait::async_trait]
impl Tool for ConnectorTool {
    fn name(&self) -> &str {
        &self.entry.name
    }

    fn description(&self) -> &str {
        &self.entry.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    /// Always, as for every MCP tool.
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    /// The workspace's connector: never a node server of the same name.
    fn approval_origin(&self) -> Option<String> {
        Some(format!("connector:{}", self.entry.connector))
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        if !input.is_object() {
            return ToolOutcome::err("the arguments of an MCP tool are a JSON object");
        }
        let call = self.source.call(&self.entry.name, input);
        let res = tokio::select! {
            r = call => r,
            _ = ctx.cancel.cancelled() => return ToolOutcome::err("cancelled"),
        };
        match res {
            Ok(a) if a.is_error => ToolOutcome::err(a.text),
            Ok(a) => ToolOutcome::ok(a.text),
            Err(e) => ToolOutcome::err(format!("connector `{}`: {e}", self.entry.connector)),
        }
    }
}

/// What a turn gets of the master's list.
pub struct Offered {
    pub tools: Vec<Arc<dyn Tool>>,
    pub notes: Vec<String>,
}

/// The bytes a tool counts against [`MAX_TOOL_TEXT`]: its name, its
/// description and its schema as JSON (the node's MCP rule).
pub fn weight(t: &dyn Tool) -> usize {
    t.name().len() + t.description().len() + t.schema().to_string().len()
}

/// The entries of `list` this turn may be offered, after `already` (the
/// chat's node MCP tools offered this turn, which win every name and come
/// first under the caps): only well-formed ones, only those `allowed`
/// (the agent's own rule and node.toml's `denied_tools`), at most what is
/// left of the per-chat caps. What is left out is named in a note.
pub fn offer(
    list: &[ConnectorToolEntry],
    already: &[Arc<dyn Tool>],
    allowed: impl Fn(&ConnectorToolEntry) -> bool,
    source: Arc<dyn ConnectorSource>,
) -> Offered {
    let mut taken: HashSet<String> = already.iter().map(|t| t.name().to_string()).collect();
    let mut text: usize = already.iter().map(|t| weight(t.as_ref())).sum();
    let mut count = already.len();
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    let mut notes = Vec::new();
    let (mut bad, mut twice, mut cut) = (0usize, Vec::new(), Vec::new());
    if list.len() > MAX_LISTED {
        bad += list.len() - MAX_LISTED;
    }
    for e in list.iter().take(MAX_LISTED) {
        if !e.well_formed() {
            bad += 1;
            continue;
        }
        if !allowed(e) {
            continue;
        }
        let Ok(schema) = serde_json::from_str::<Value>(&e.schema) else {
            bad += 1;
            continue;
        };
        if taken.contains(&e.name) {
            twice.push(harness_core::event::shown_name(&e.tool));
            continue;
        }
        let size = e.name.len() + e.description.len() + schema.to_string().len();
        if count >= MAX_TOOLS || text + size > MAX_TOOL_TEXT {
            cut.push(harness_core::event::shown_name(&e.tool));
            continue;
        }
        text += size;
        count += 1;
        taken.insert(e.name.clone());
        tools.push(Arc::new(ConnectorTool {
            entry: e.clone(),
            schema,
            source: source.clone(),
        }));
    }
    let list_of = |v: &[String]| shorten(&v.join(", "), 300);
    if bad > 0 {
        notes.push(format!(
            "{bad} connector tool(s) from the server were not taken: they are not in the form \
             this node takes"
        ));
    }
    if !twice.is_empty() {
        notes.push(format!(
            "{} connector tool(s) are not offered: an MCP server of this node has a tool of the \
             same name in this chat ({})",
            twice.len(),
            list_of(&twice)
        ));
    }
    if !cut.is_empty() {
        notes.push(format!(
            "{} connector tool(s) are not offered in this chat ({}): a chat carries at most \
             {MAX_TOOLS} MCP tools and {MAX_TOOL_TEXT} bytes of their descriptions and schemas, \
             this node's servers first",
            cut.len(),
            list_of(&cut)
        ));
    }
    Offered { tools, notes }
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::connectors::ConnectorAnswer;

    struct Echo;

    #[async_trait::async_trait]
    impl ConnectorSource for Echo {
        async fn call(&self, tool: &str, a: Value) -> Result<ConnectorAnswer, String> {
            Ok(ConnectorAnswer {
                text: format!("{tool} {a}"),
                is_error: a.get("fail").is_some(),
            })
        }
    }

    fn entry(c: &str, t: &str, desc: usize) -> ConnectorToolEntry {
        ConnectorToolEntry {
            connector: c.into(),
            name: crate::mcp::public_name(c, t),
            tool: t.into(),
            description: format!("(MCP server `{c}`) {}", "d".repeat(desc)),
            schema: r#"{"type":"object"}"#.into(),
        }
    }

    struct Named(String);

    #[async_trait::async_trait]
    impl Tool for Named {
        fn name(&self) -> &str {
            &self.0
        }
        fn description(&self) -> &str {
            ""
        }
        fn schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        fn risk(&self) -> ToolRisk {
            ToolRisk::Mutating
        }
        async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
            ToolOutcome::ok("")
        }
    }

    /// The node's own servers win a name and go first under the caps; the
    /// caps hold for both together; a malformed entry is never offered;
    /// what is left out is told.
    #[test]
    fn node_servers_first_under_one_set_of_caps() {
        let node: Vec<Arc<dyn Tool>> = (0..38)
            .map(|i| Arc::new(Named(format!("mcp__srv__t{i}"))) as Arc<dyn Tool>)
            .chain([Arc::new(Named("mcp__linear__same".into())) as Arc<dyn Tool>])
            .collect();
        let mut bad = entry("linear", "x", 1);
        bad.name = "bash".into();
        let list = vec![
            entry("linear", "same", 1),
            entry("linear", "one", 1),
            entry("linear", "two", 1),
            bad,
        ];
        let o = offer(&list, &node, |_| true, Arc::new(Echo));
        let names: Vec<&str> = o.tools.iter().map(|t| t.name()).collect();
        assert_eq!(names, ["mcp__linear__one"], "39 + 1 = MAX_TOOLS");
        let notes = o.notes.join("\n");
        assert!(
            notes.contains("1 connector tool(s) from the server were not taken"),
            "{notes}"
        );
        assert!(notes.contains("same name in this chat (same)"), "{notes}");
        assert!(notes.contains("not offered in this chat (two)"), "{notes}");
        // The text cap counts node tools too.
        let big: Vec<ConnectorToolEntry> = (0..20)
            .map(|i| entry("a", &format!("t{i}"), 4_000))
            .collect();
        let o = offer(&big, &[], |_| true, Arc::new(Echo));
        assert_eq!(o.tools.len(), 15, "64,000 bytes hold 15 of about 4,060");
        // Over MAX_LISTED: the rest is not taken.
        let many: Vec<ConnectorToolEntry> = (0..MAX_LISTED + 5)
            .map(|i| entry("a", &format!("t{i}"), 1))
            .collect();
        let o = offer(&many, &[], |_| true, Arc::new(Echo));
        assert_eq!(o.tools.len(), MAX_LISTED);
        assert!(
            o.notes[0].starts_with("5 connector tool(s)"),
            "{:?}",
            o.notes
        );
        // Not allowed: left out silently (the agent's own rule).
        let o = offer(&list[1..2], &[], |_| false, Arc::new(Echo));
        assert!(o.tools.is_empty() && o.notes.is_empty());
    }

    #[tokio::test]
    async fn a_call_is_an_mcp_call_of_the_master() {
        let o = offer(&[entry("linear", "one", 1)], &[], |_| true, Arc::new(Echo));
        let t = &o.tools[0];
        assert_eq!(t.risk(), ToolRisk::Mutating);
        let ctx = ToolCtx::new(std::env::temp_dir());
        let ok = t.run(&ctx, serde_json::json!({"a": 1})).await;
        assert!(
            ok.ok && ok.output == r#"mcp__linear__one {"a":1}"#,
            "{}",
            ok.output
        );
        let err = t.run(&ctx, serde_json::json!({"fail": 1})).await;
        assert!(!err.ok);
        let not_object = t.run(&ctx, serde_json::json!([1])).await;
        assert!(!not_object.ok && not_object.output.contains("JSON object"));
    }
}
