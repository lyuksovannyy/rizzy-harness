//! The node owner's skill folders in the agent shell's file view (review
//! S2 of pull request 95, PLAN.md T46): a session whose agent has the
//! `skill` tool (its turn carries a catalogue) may read and run what is in
//! `[skills] roots`, never write there; a session without it does not see
//! them at all; a root that is a link is bound as its real folder.
//!
//! Through the real `rizzy-harness` binary, the real `bwrap` and the real
//! bash tool. They skip where bubblewrap or Landlock does not work, and fail
//! there with `HARNESS_REQUIRE_BWRAP=1`.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::PathBuf;
use std::sync::Arc;

use harness_core::skills::{Catalogue, SkillOrigin};
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::skills::{self, Root};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    ws: PathBuf,
    /// `home/.claude/skills`, a link to `home/dotfiles/skills`.
    root: PathBuf,
    /// The real folder behind it.
    real: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let data = base.join("data");
    let ws = base.join("home/proj");
    let real = base.join("home/dotfiles/skills");
    let root = base.join("home/.claude/skills");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(real.join("hello/scripts")).unwrap();
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &root).unwrap();
    std::fs::write(
        real.join("hello/SKILL.md"),
        "---\nname: hello\ndescription: Say hello\n---\nRun scripts/hi.sh.\n",
    )
    .unwrap();
    std::fs::write(real.join("hello/scripts/hi.sh"), "echo SKILL-SCRIPT-RAN\n").unwrap();
    Fixture {
        _dir: dir,
        data,
        ws,
        root,
        real,
    }
}

impl Fixture {
    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap();
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain"))
            .ok()
            .map(Arc::new)
    }

    /// The bash tool's context; `skills`: the turn's catalogue, as the node
    /// sets it for an agent that has the `skill` tool.
    fn ctx(&self, sb: Arc<Sandbox>, skills: Option<Catalogue>) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        ctx.skills = skills.map(Arc::new);
        ctx
    }

    fn catalogue(&self) -> Catalogue {
        skills::discover(
            &[Root {
                path: self.root.clone(),
                origin: SkillOrigin::Node,
                bound: None,
            }],
            std::slice::from_ref(&self.data),
        )
        .catalogue
    }
}

/// With the tool: the script under the root runs (by the real folder's
/// path, which the `skill` tool names), nothing there can be written; the
/// rest of the home folder stays out of view.
#[tokio::test]
async fn an_agent_with_the_tool_runs_a_skill_script_and_cannot_write_there() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let c = f.catalogue();
    assert_eq!(
        c.read,
        std::slice::from_ref(&f.real),
        "the link's real folder"
    );
    assert_eq!(c.skills[0].dir, f.real.join("hello"));
    let ctx = f.ctx(sb, Some(c));
    let script = f.real.join("hello/scripts/hi.sh");
    let r = Bash
        .run(
            &ctx,
            json!({ "command": format!("bash {}", script.display()) }),
        )
        .await;
    assert!(r.output.contains("SKILL-SCRIPT-RAN"), "{}", r.output);
    let w = Bash
        .run(
            &ctx,
            json!({ "command": format!(
                "echo x > {0}/hello/planted; echo y >> {0}/hello/scripts/hi.sh; ls {1}",
                f.real.display(),
                f.data.display()
            ) }),
        )
        .await;
    assert!(!f.real.join("hello/planted").exists(), "{}", w.output);
    assert_eq!(
        std::fs::read_to_string(&script).unwrap(),
        "echo SKILL-SCRIPT-RAN\n",
        "{}",
        w.output
    );
}

/// Without the tool (no catalogue in the turn): the root is not in the
/// view, so the script cannot even be read.
#[tokio::test]
async fn an_agent_without_the_tool_gets_no_such_bind() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let ctx = f.ctx(sb, None);
    let script = f.real.join("hello/scripts/hi.sh");
    let r = Bash
        .run(
            &ctx,
            json!({ "command": format!("cat {}; echo done", script.display()) }),
        )
        .await;
    assert!(r.output.contains("done"), "{}", r.output);
    assert!(!r.output.contains("SKILL-SCRIPT-RAN"), "{}", r.output);
}
