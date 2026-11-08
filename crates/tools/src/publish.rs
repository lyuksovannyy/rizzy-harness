//! `publish`: hand a file or folder the agent built to the user, through
//! Artifacts (docs/artifacts.md). The node does the work (confinement, the
//! staged copy, the event); this tool only checks its arguments.

use harness_core::artifacts::{self, PublishRequest};
use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

pub struct Publish;

/// An optional string argument, `None` when absent, null or empty.
fn opt_arg(input: &Value, key: &str) -> Result<Option<String>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

/// The request the arguments make, or why they make none.
fn request(ctx: &ToolCtx, input: &Value) -> Result<PublishRequest, String> {
    let path = str_arg(input, "path")?.trim().to_string();
    if path.is_empty() {
        return Err("`path` is empty".into());
    }
    let name = opt_arg(input, "name")?
        .map(|n| artifacts::check_name(&n))
        .transpose()?;
    let notes = opt_arg(input, "notes")?
        .map(|n| artifacts::check_notes(&n))
        .transpose()?;
    let kind_hint = opt_arg(input, "kind_hint")?
        .map(|k| harness_core::text::clean_line(&k, 32))
        .filter(|k| !k.is_empty());
    Ok(PublishRequest {
        call_id: ctx.call_id.clone(),
        path,
        name,
        notes,
        kind_hint,
        cancel: ctx.cancel.clone(),
    })
}

#[async_trait::async_trait]
impl Tool for Publish {
    fn name(&self) -> &'static str {
        "publish"
    }

    /// Frozen text (sidebar areas §7.2): agents publish because of it.
    fn description(&self) -> &'static str {
        "Publish a file or folder you built so the user can download it from Artifacts. \
         When you finish building something the user asked for (a binary, package, app \
         bundle, archive or rendered media), publish the final output once. Do not publish \
         intermediate files, secrets, or files the user did not ask for."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The file or folder, relative to the working folder"},
                "name": {"type": "string", "description": "The artifact's name (default: the file or folder name); versions of one name are kept together"},
                "notes": {"type": "string", "description": "One line about this version"},
                "kind_hint": {"type": "string", "description": "What it is, e.g. binary, package, archive, document"}
            },
            "required": ["path"]
        })
    }

    /// It sends a file off the machine: asked about in `ask` mode.
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let Some(sink) = &ctx.artifacts else {
            return ToolOutcome::err("publishing is not available in this session");
        };
        let req = match request(ctx, &input) {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(e),
        };
        match sink.publish(req).await {
            Ok(p) => ToolOutcome::ok(format!(
                "Published `{}` ({}, sha256 {}…); uploading to the server, the user will find it in Artifacts.",
                p.name,
                artifacts::human_size(p.size),
                &p.sha256[..p.sha256.len().min(4)]
            )),
            Err(e) => ToolOutcome::err(format!("Could not publish: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use harness_core::artifacts::{ArtifactSink, Published};

    use super::*;

    #[derive(Default)]
    struct Rec(Mutex<Vec<PublishRequest>>);

    #[async_trait::async_trait]
    impl ArtifactSink for Rec {
        async fn publish(&self, req: PublishRequest) -> Result<Published, String> {
            let name = req.name.clone().unwrap_or_else(|| "app".into());
            self.0.lock().unwrap().push(req);
            Ok(Published {
                name,
                size: 12_900_000,
                sha256: "3f2a".repeat(16),
                packed: false,
            })
        }
    }

    #[tokio::test]
    async fn publishes_through_the_sink_and_checks_arguments() {
        let rec = Arc::new(Rec::default());
        let ctx = ToolCtx {
            artifacts: Some(rec.clone()),
            call_id: "c1".into(),
            ..ToolCtx::new("/")
        };
        let out = Publish
            .run(
                &ctx,
                json!({"path": "target/release/myapp", "name": "myapp"}),
            )
            .await;
        assert!(out.ok, "{}", out.output);
        assert_eq!(
            out.output,
            "Published `myapp` (12.3 MB, sha256 3f2a…); uploading to the server, the user will find it in Artifacts."
        );
        assert_eq!(rec.0.lock().unwrap()[0].call_id, "c1");
        for bad in [
            json!({"path": "x", "name": "a/b"}),
            json!({"path": "x", "notes": "two\nlines"}),
            json!({"path": "  "}),
            json!({"name": "no path"}),
            json!({"path": "x", "name": 3}),
        ] {
            assert!(!Publish.run(&ctx, bad.clone()).await.ok, "{bad}");
        }
        assert_eq!(rec.0.lock().unwrap().len(), 1);
        assert_eq!(Publish.risk(), ToolRisk::Mutating);
    }

    #[tokio::test]
    async fn off_without_a_sink() {
        let out = Publish.run(&ToolCtx::new("/"), json!({"path": "x"})).await;
        assert!(!out.ok);
        assert!(out.output.contains("not available"));
    }
}
