//! Skills (ADOPTION_PLAN X.2): discovery of `SKILL.md` folders in the
//! repository and in the node owner's roots, the capped catalogue, and the
//! `skill` tool that loads one body (and the files beside it) by name.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::policy::ToolRisk;
use harness_core::skills::{Catalogue, SkillOrigin};
use harness_tools::skills::{self, LEAD, Root, SkillTool};
use harness_tools::{Tool, ToolCtx};
use serde_json::json;

struct Tree {
    _d: tempfile::TempDir,
    base: PathBuf,
    /// The granted folder.
    ws: PathBuf,
    /// A repository inside it.
    repo: PathBuf,
    /// A root the node's owner names.
    home: PathBuf,
}

fn tree() -> Tree {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let ws = base.join("ws");
    let repo = ws.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let home = base.join("home/.agents/skills");
    std::fs::create_dir_all(&home).unwrap();
    Tree {
        _d: d,
        base,
        ws,
        repo,
        home,
    }
}

fn put(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill_md(name: &str, description: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n{body}")
}

fn node_root(p: &Path) -> Root {
    Root {
        path: p.to_path_buf(),
        origin: SkillOrigin::Node,
        bound: None,
    }
}

/// The node's roots first, then the repository's, nearest folder first.
fn roots(t: &Tree, cwd: &Path) -> Vec<Root> {
    let mut r = vec![node_root(&t.home)];
    r.extend(skills::repo_roots(cwd, &t.ws));
    r
}

fn names(c: &Catalogue) -> Vec<&str> {
    c.skills.iter().map(|s| s.name.as_str()).collect()
}

fn ctx_with(ws: &Path, c: Catalogue) -> ToolCtx {
    ToolCtx {
        skills: Some(Arc::new(c)),
        ..ToolCtx::new(ws)
    }
}

/// Skills are found in `.agents/skills` and `.claude/skills` of every
/// folder from the repository's top down to the session folder and in the
/// node's roots, with the folder name standing in for a missing `name`
/// and the first paragraph of text for a missing `description`; a folder above the
/// repository, below the session folder, a hidden folder and a folder with
/// no `SKILL.md` add nothing.
#[test]
fn skills_are_found_in_the_repository_and_the_node_roots() {
    let t = tree();
    let cwd = t.repo.join("crates/foo");
    std::fs::create_dir_all(&cwd).unwrap();
    put(
        &t.repo.join(".agents/skills/release/SKILL.md"),
        "---\nname: release\ndescription: >\n  Cut a release:\n  tag and notes.\n---\nSteps.\n",
    );
    put(
        &t.repo.join(".claude/skills/pdf/SKILL.md"),
        "# PDF\n\nFill PDF forms\nwith pdftk.\n\nMore.\n",
    );
    put(
        &cwd.join(".agents/skills/foo-tests/SKILL.md"),
        skill_md("foo-tests", "Run foo's tests", "cargo test -p foo"),
    );
    put(
        &t.home.join("commit/SKILL.md"),
        skill_md("commit", "Write a commit message", "Imperative title."),
    );
    put(
        &t.ws.join(".agents/skills/above/SKILL.md"),
        skill_md("above", "above the repository", ""),
    );
    put(
        &cwd.join("deeper/.agents/skills/below/SKILL.md"),
        skill_md("below", "below the session", ""),
    );
    put(
        &t.repo.join(".agents/skills/.hidden/SKILL.md"),
        skill_md("hidden", "hidden", ""),
    );
    std::fs::create_dir_all(t.repo.join(".agents/skills/empty")).unwrap();
    let found = skills::discover(&roots(&t, &cwd), &[]);
    let c = &found.catalogue;
    assert_eq!(names(c), ["commit", "foo-tests", "pdf", "release"]);
    let get = |n: &str| c.skills.iter().find(|s| s.name == n).unwrap();
    assert_eq!(get("release").description, "Cut a release: tag and notes.");
    // A heading is not a description: the first paragraph of text is.
    assert_eq!(get("pdf").description, "Fill PDF forms with pdftk.");
    assert_eq!(get("commit").origin, SkillOrigin::Node);
    assert_eq!(get("release").origin, SkillOrigin::Repository);
    assert_eq!(get("foo-tests").dir, cwd.join(".agents/skills/foo-tests"));
    // No `SKILL.md` is not a problem; nothing else went wrong.
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

/// A name the node's roots already give is not taken from the repository
/// (a repository cannot stand in for the owner's own skill), and within
/// the repository the nearest folder wins; the log is told.
#[test]
fn the_node_root_wins_a_name_and_the_nearest_folder_next() {
    let t = tree();
    let cwd = t.repo.join("sub");
    put(
        &t.home.join("deploy/SKILL.md"),
        skill_md("deploy", "the owner's deploy", ""),
    );
    put(
        &t.repo.join(".agents/skills/deploy/SKILL.md"),
        skill_md("deploy", "the repository's deploy", ""),
    );
    put(
        &t.repo.join(".agents/skills/lint/SKILL.md"),
        skill_md("lint", "top lint", ""),
    );
    put(
        &cwd.join(".claude/skills/lint/SKILL.md"),
        skill_md("lint", "near lint", ""),
    );
    let found = skills::discover(&roots(&t, &cwd), &[]);
    let c = &found.catalogue;
    assert_eq!(names(c), ["deploy", "lint"]);
    assert_eq!(c.skills[0].description, "the owner's deploy");
    assert_eq!(c.skills[1].description, "near lint");
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
    assert!(found.problems.iter().all(|p| p.contains("already")));
}

/// Abuse: a skill folder or `SKILL.md` that is a link out of the
/// repository, a FIFO, a skill with no description, and a protected folder
/// are not read; each is named in the problems and in `refused`; nothing
/// waits. Bad front matter is read line by line and a name with a
/// direction override gives way to the folder's.
#[test]
fn links_out_of_the_root_and_bad_files_are_not_read() {
    let t = tree();
    put(
        &t.base.join("secret/SKILL.md"),
        skill_md("stolen", "secret", "key"),
    );
    put(
        &t.base.join("loose.md"),
        skill_md("loose", "outside", "key"),
    );
    let skills_dir = t.repo.join(".agents/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    // A skill folder that is a link out of the repository.
    std::os::unix::fs::symlink(t.base.join("secret"), skills_dir.join("stolen")).unwrap();
    // A SKILL.md that is a link out of its folder.
    std::fs::create_dir_all(skills_dir.join("loose")).unwrap();
    std::os::unix::fs::symlink(t.base.join("loose.md"), skills_dir.join("loose/SKILL.md")).unwrap();
    // A FIFO is never waited on.
    std::fs::create_dir_all(skills_dir.join("fifo")).unwrap();
    nix::unistd::mkfifo(
        &skills_dir.join("fifo/SKILL.md"),
        nix::sys::stat::Mode::S_IRWXU,
    )
    .unwrap();
    // Front matter that does not parse, a name that is not a name, none.
    put(
        &skills_dir.join("bad/SKILL.md"),
        "---\ndescription: [unclosed\n---\nx",
    );
    put(
        &skills_dir.join("evil/SKILL.md"),
        "---\nname: \"a\\u202eb\"\ndescription: d\n---\n",
    );
    put(
        &skills_dir.join("nodesc/SKILL.md"),
        "---\nname: nodesc\n---\n",
    );
    // A protected folder inside the repository (the node's data directory).
    let data = t.repo.join("data");
    put(
        &data.join("keys/SKILL.md"),
        skill_md("keys", "keys", "secret"),
    );
    std::os::unix::fs::symlink(data.join("keys"), skills_dir.join("keys")).unwrap();
    // A good one, the control.
    put(
        &skills_dir.join("ok/SKILL.md"),
        skill_md("ok", "fine", "body"),
    );
    let at = std::time::Instant::now();
    let found = skills::discover(
        &skills::repo_roots(&t.repo, &t.ws),
        std::slice::from_ref(&data),
    );
    assert!(at.elapsed() < std::time::Duration::from_secs(2));
    // Front matter that is not YAML is read line by line (review S1), and
    // a name that is not one gives way to the folder's (review S4): the
    // direction override never reaches the list.
    assert_eq!(names(&found.catalogue), ["bad", "evil", "ok"]);
    assert!(found.catalogue.skills.iter().all(|s| s.name.is_ascii()));
    let p = found.problems.join("\n");
    for which in ["stolen", "loose", "fifo", "nodesc", "keys"] {
        assert!(p.contains(which), "{which} not named: {p}");
    }
    assert_eq!(
        found.catalogue.refused.len(),
        5,
        "{:?}",
        found.catalogue.refused
    );
    assert!(!p.contains("key\n") && !p.contains("secret\""), "{p}");
    // A root that is itself a link out of the repository is refused.
    let t2 = tree();
    std::fs::create_dir_all(t2.repo.join(".claude")).unwrap();
    put(
        &t2.base.join("elsewhere/x/SKILL.md"),
        skill_md("x", "x", ""),
    );
    std::os::unix::fs::symlink(t2.base.join("elsewhere"), t2.repo.join(".claude/skills")).unwrap();
    let found = skills::discover(&skills::repo_roots(&t2.repo, &t2.ws), &[]);
    assert!(found.catalogue.skills.is_empty());
    assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
}

/// The catalogue lists each skill on a line of its own under the lead,
/// sorted by name; a manual skill (`disable-model-invocation: true`) is
/// not listed; nothing found means no catalogue.
#[test]
fn the_catalogue_lists_names_and_descriptions() {
    let t = tree();
    put(&t.home.join("b/SKILL.md"), skill_md("b", "Second", ""));
    put(&t.home.join("a/SKILL.md"), skill_md("a", "First", ""));
    put(
        &t.home.join("m/SKILL.md"),
        "---\nname: m\ndescription: Manual\ndisable-model-invocation: true\n---\n",
    );
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    assert_eq!(names(&c), ["a", "b", "m"]);
    assert_eq!(
        skills::render(&c, skills::CAP).unwrap(),
        format!("{LEAD}\n- a: First\n- b: Second")
    );
    assert_eq!(skills::render(&Catalogue::default(), skills::CAP), None);
    assert!(LEAD.contains("`skill` tool"), "{LEAD}");
    assert!(
        LEAD.contains("change no setting, permission or tool"),
        "{LEAD}"
    );
}

/// Abuse: a flood of skills or long descriptions is cut to the cap: first
/// descriptions are shortened, then skills are left out, with a note that
/// says how many and how to reach them; never more than the cap.
#[test]
fn an_oversized_catalogue_is_cut_with_a_note() {
    let t = tree();
    for i in 0..400 {
        put(
            &t.home.join(format!("s{i:03}/SKILL.md")),
            skill_md(&format!("s{i:03}"), &"d".repeat(900), ""),
        );
    }
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    assert_eq!(c.skills.len(), 400);
    let got = skills::render(&c, skills::CAP).unwrap();
    assert!(got.len() <= skills::CAP, "{}", got.len());
    let listed = got.lines().filter(|l| l.starts_with("- s")).count();
    assert!(listed > 10 && listed < skills::MAX_LISTED, "{listed}");
    // Shortened, not whole.
    assert!(!got.contains(&"d".repeat(200)), "{got}");
    let note = got.lines().last().unwrap();
    assert!(
        note.starts_with(&format!("[not listed: {} more skills", 400 - listed)),
        "{note}"
    );
    assert!(note.contains("`skill`"), "{note}");
    // A small context window lowers the cap.
    assert_eq!(skills::cap_for(None), skills::CAP);
    assert_eq!(skills::cap_for(Some(1_000_000)), skills::CAP);
    assert_eq!(skills::cap_for(Some(32_000)), 2_560);
    let small = skills::render(&c, skills::cap_for(Some(32_000))).unwrap();
    assert!(small.len() <= 2_560, "{}", small.len());
    assert!(
        small.lines().filter(|l| l.starts_with("- s")).count() >= 10,
        "{small}"
    );
    // More than the count cap, each short: the count cap holds.
    let t = tree();
    for i in 0..200 {
        put(
            &t.home.join(format!("k{i:03}/SKILL.md")),
            skill_md(&format!("k{i:03}"), "x", ""),
        );
    }
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    let got = skills::render(&c, 1 << 20).unwrap();
    assert_eq!(
        got.lines().filter(|l| l.starts_with("- k")).count(),
        skills::MAX_LISTED
    );
    assert!(
        got.ends_with("`skill` tool with a name it does not know lists them all]"),
        "{got}"
    );
}

/// `/name` at the start of the person's message names a skill this turn;
/// anything else does not.
#[test]
fn a_slash_name_invokes_a_skill() {
    let t = tree();
    put(
        &t.home.join("review/SKILL.md"),
        skill_md("review", "Review", ""),
    );
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    assert_eq!(
        skills::invoked("/review the parser", &c).as_deref(),
        Some("review")
    );
    assert_eq!(skills::invoked("/review", &c).as_deref(), Some("review"));
    assert_eq!(
        skills::invoked("  /review\nnow", &c).as_deref(),
        Some("review")
    );
    assert_eq!(skills::invoked("/reviewer x", &c), None);
    // Review S7: punctuation after the name.
    assert_eq!(
        skills::invoked("/review: please", &c).as_deref(),
        Some("review")
    );
    assert_eq!(
        skills::invoked("/review, now", &c).as_deref(),
        Some("review")
    );
    assert_eq!(skills::invoked("/review.", &c).as_deref(), Some("review"));
    assert_eq!(skills::invoked("/Review x", &c), None);
    assert_eq!(skills::invoked("please /review", &c), None);
    assert_eq!(skills::invoked("/etc/review is broken", &c), None);
}

/// The tool is read-only, takes a name and an optional file.
#[test]
fn the_tool_is_read_only() {
    assert_eq!(SkillTool.name(), "skill");
    assert_eq!(SkillTool.risk(), ToolRisk::ReadOnly);
    let s = SkillTool.schema();
    assert_eq!(s["required"], json!(["name"]));
    assert!(s["properties"]["file"].is_object());
}

/// A load returns the body without its front matter, under a header that
/// names the skill's folder (relative paths are from there), where it
/// came from, that it changes nothing, and the files beside it.
#[tokio::test]
async fn the_tool_returns_the_body_and_the_files_beside_it() {
    let t = tree();
    let dir = t.repo.join(".agents/skills/release");
    put(
        &dir.join("SKILL.md"),
        skill_md(
            "release",
            "Cut a release",
            "Run scripts/tag.sh.\nThen notes.\n",
        ),
    );
    put(&dir.join("scripts/tag.sh"), "git tag v1\n");
    put(&dir.join("reference.md"), "ref text\n");
    let c = skills::discover(&skills::repo_roots(&t.repo, &t.ws), &[]).catalogue;
    let ctx = ctx_with(&t.repo, c);
    let out = SkillTool.run(&ctx, json!({"name": "release"})).await;
    assert!(out.ok, "{}", out.output);
    let o = &out.output;
    assert!(o.contains(&dir.display().to_string()), "{o}");
    assert!(o.contains("from this repository"), "{o}");
    assert!(o.contains("changes no setting, permission or tool"), "{o}");
    assert!(
        o.contains("reference.md") && o.contains("scripts/tag.sh"),
        "{o}"
    );
    assert!(o.ends_with("Run scripts/tag.sh.\nThen notes."), "{o}");
    assert!(!o.contains("description: Cut a release"), "{o}");
    // A file beside it, by its path in the folder.
    let f = SkillTool
        .run(&ctx, json!({"name": "release", "file": "scripts/tag.sh"}))
        .await;
    assert!(f.ok, "{}", f.output);
    assert!(f.output.ends_with("git tag v1"), "{}", f.output);
}

/// Abuse: a file outside the skill's folder, by `..`, by an absolute path
/// or by a link, is not read; an unknown name, or a manual skill the
/// person did not invoke, is refused with the names there are; without
/// skills the tool says so.
#[tokio::test]
async fn the_tool_reads_nothing_outside_the_skill() {
    let t = tree();
    let dir = t.home.join("ops");
    put(&dir.join("SKILL.md"), skill_md("ops", "Ops", "body"));
    put(&t.base.join("home/.ssh/id_ed25519"), "PRIVATE KEY");
    std::os::unix::fs::symlink(t.base.join("home/.ssh/id_ed25519"), dir.join("key")).unwrap();
    put(
        &t.home.join("manual/SKILL.md"),
        "---\ndescription: by hand\ndisable-model-invocation: true\n---\nmanual body",
    );
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    let ctx = ctx_with(&t.ws, c.clone());
    for file in [
        "../../.ssh/id_ed25519".to_string(),
        t.base.join("home/.ssh/id_ed25519").display().to_string(),
        "key".to_string(),
    ] {
        let out = SkillTool
            .run(&ctx, json!({"name": "ops", "file": file}))
            .await;
        assert!(!out.ok, "{file}: {}", out.output);
        assert!(!out.output.contains("PRIVATE KEY"), "{}", out.output);
    }
    let out = SkillTool.run(&ctx, json!({"name": "nope"})).await;
    assert!(!out.ok);
    assert!(out.output.contains("ops"), "{}", out.output);
    assert!(!out.output.contains("manual"), "{}", out.output);
    let out = SkillTool.run(&ctx, json!({"name": "manual"})).await;
    assert!(!out.ok, "{}", out.output);
    // Invoked by the person this turn: it loads.
    let invoked = Catalogue {
        invoked: Some("manual".into()),
        ..c
    };
    let out = SkillTool
        .run(&ctx_with(&t.ws, invoked), json!({"name": "manual"}))
        .await;
    assert!(out.ok, "{}", out.output);
    assert!(out.output.ends_with("manual body"), "{}", out.output);
    let none = SkillTool
        .run(&ToolCtx::new(&t.ws), json!({"name": "ops"}))
        .await;
    assert!(!none.ok);
    assert!(none.output.contains("No skills"), "{}", none.output);
    // A SKILL.md swapped for a link out after discovery is refused.
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    std::fs::remove_file(dir.join("SKILL.md")).unwrap();
    std::os::unix::fs::symlink(t.base.join("home/.ssh/id_ed25519"), dir.join("SKILL.md")).unwrap();
    let out = SkillTool
        .run(&ctx_with(&t.ws, c), json!({"name": "ops"}))
        .await;
    assert!(!out.ok, "{}", out.output);
    assert!(!out.output.contains("PRIVATE KEY"), "{}", out.output);
}

/// Abuse: an oversized body is cut with a note that says where.
#[tokio::test]
async fn an_oversized_body_is_cut_with_a_note() {
    let t = tree();
    let line = "y".repeat(99) + "\n";
    put(
        &t.home.join("big/SKILL.md"),
        skill_md("big", "Big", &line.repeat(1000)),
    );
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    let out = SkillTool
        .run(&ctx_with(&t.ws, c), json!({"name": "big"}))
        .await;
    assert!(out.ok, "{}", out.output);
    let lines: Vec<&str> = out.output.lines().filter(|l| l.starts_with('y')).collect();
    // The lines and the line breaks between them.
    let body = lines.iter().map(|l| l.len()).sum::<usize>() + lines.len() - 1;
    assert!(body <= skills::BODY_CAP, "{body}");
    assert!(body > skills::BODY_CAP - 100, "{body}");
    assert!(
        out.output.ends_with(&format!(
            "[cut: SKILL.md is longer than {} KiB; the text above is its first part]",
            skills::BODY_CAP / 1024
        )),
        "{}",
        &out.output[out.output.len() - 200..]
    );
}

/// Review S1, from the README verbatim: an unquoted `": "` in a value is
/// not YAML, and is what other harnesses' skills look like. The keys are
/// read line by line then, a plain value's indented lines joined.
#[test]
fn front_matter_that_is_not_yaml_is_read_line_by_line() {
    let t = tree();
    put(
        &t.home.join("release/SKILL.md"),
        "---\nname: release\ndescription: Cut a release of this project: bump the version, tag, write the notes.\n---\n1. Run `scripts/bump.sh <version>` from this folder.\n2. ...\n",
    );
    put(
        &t.home.join("triage/SKILL.md"),
        "---\nname: triage\ndescription: Use when: a bug report\n  comes in: sort it.\nallowed-tools: Bash(gh:*)\ndisable-model-invocation: true\n---\nbody\n",
    );
    put(
        &t.home.join("quoted/SKILL.md"),
        "---\nname: 'quoted'\ndescription: \"Use when: quoted\" : odd\n---\nbody\n",
    );
    let found = skills::discover(&[node_root(&t.home)], &[]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let c = &found.catalogue;
    assert_eq!(names(c), ["quoted", "release", "triage"]);
    assert_eq!(
        c.skills[1].description,
        "Cut a release of this project: bump the version, tag, write the notes."
    );
    assert_eq!(
        c.skills[2].description,
        "Use when: a bug report comes in: sort it."
    );
    assert!(c.skills[2].manual);
    assert!(c.refused.is_empty());
}

/// Review S1: a skill that is still refused is named with the catalogue
/// (folder names only; the reasons stay in the node's log).
#[test]
fn a_refused_skill_is_named_with_the_catalogue() {
    let t = tree();
    put(&t.home.join("ok/SKILL.md"), skill_md("ok", "fine", ""));
    put(&t.home.join("empty-desc/SKILL.md"), "---\nname: x\n---\n");
    put(
        &t.home.join("open/SKILL.md"),
        "---\nname: open\ndescription: d\n",
    );
    let found = skills::discover(&[node_root(&t.home)], &[]);
    assert_eq!(found.catalogue.refused, ["empty-desc", "open"]);
    let got = skills::render(&found.catalogue, skills::CAP).unwrap();
    assert!(
        got.ends_with(
            "\n[2 skill folder(s) could not be read: empty-desc, open; the node's log says why]"
        ),
        "{got}"
    );
    // Only refused ones: still said.
    let only = Catalogue {
        refused: vec!["x".into()],
        ..Default::default()
    };
    assert!(
        skills::render(&only, skills::CAP)
            .unwrap()
            .contains("could not be read: x;")
    );
}

/// The front matter shapes seen in real skills (review probe): a BOM and
/// CRLF, `license`, `allowed-tools` and nested `metadata` ignored, a name
/// that differs from the folder's, a folded description, a number as a
/// name; review S4: a given name that is not one (`My Skill`) gives way to
/// the folder's name.
#[test]
fn real_front_matter_shapes_are_read() {
    let t = tree();
    let h = &t.home;
    put(
        &h.join("pdf-folder/SKILL.md"),
        "\u{FEFF}---\r\nname: pdf\r\ndescription: \"Comprehensive PDF toolkit. Use when: filling forms\"\r\nlicense: Proprietary. LICENSE.txt has complete terms\r\nallowed-tools: Bash(python:*), Read\r\nmetadata:\r\n  version: 1.0\r\n---\r\n# PDF\r\n",
    );
    put(
        &h.join("multi/SKILL.md"),
        "---\nname: multi\ndescription: >\n  Line one\n  line two.\n---\nbody\n",
    );
    put(
        &h.join("my-skill/SKILL.md"),
        "---\nname: My Skill\ndescription: does things\n---\nbody\n",
    );
    put(
        &h.join("2fa/SKILL.md"),
        "---\nname: 2024\ndescription: year\n---\nbody\n",
    );
    let found = skills::discover(&[node_root(h)], &[]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let c = &found.catalogue;
    assert_eq!(names(c), ["2024", "multi", "my-skill", "pdf"]);
    let d = |n: &str| {
        c.skills
            .iter()
            .find(|s| s.name == n)
            .unwrap()
            .description
            .clone()
    };
    assert_eq!(
        d("pdf"),
        "Comprehensive PDF toolkit. Use when: filling forms"
    );
    assert_eq!(d("multi"), "Line one line two.");
    assert_eq!(d("my-skill"), "does things");
}

/// Review probe: a CRLF body with a BOM loads clean, a file beside it by
/// its path, and `./x/../SKILL.md` is refused.
#[tokio::test]
async fn a_crlf_body_loads_and_a_dotdot_file_is_refused() {
    let t = tree();
    put(
        &t.home.join("pdf-folder/SKILL.md"),
        "\u{FEFF}---\r\nname: pdf\r\ndescription: PDF toolkit\r\n---\r\n# PDF\r\nSee references/forms.md\r\n",
    );
    put(&t.home.join("pdf-folder/references/forms.md"), "forms text");
    let c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    let ctx = ctx_with(&t.ws, c);
    let out = SkillTool.run(&ctx, json!({"name": "pdf"})).await;
    assert!(out.ok, "{}", out.output);
    assert!(
        out.output.ends_with("\n\n# PDF\nSee references/forms.md"),
        "{}",
        out.output
    );
    assert!(!out.output.contains('\r'), "{}", out.output);
    assert!(
        out.output.contains("references/forms.md (read one"),
        "{}",
        out.output
    );
    let f = SkillTool
        .run(&ctx, json!({"name": "pdf", "file": "references/forms.md"}))
        .await;
    assert!(f.ok && f.output.ends_with("forms text"), "{}", f.output);
    let up = SkillTool
        .run(
            &ctx,
            json!({"name": "pdf", "file": "./references/../SKILL.md"}),
        )
        .await;
    assert!(!up.ok, "{}", up.output);
}

/// Review S6 (probe): with a small window the catalogue still lists about
/// ten skills (the cap never goes below the lead, the note and about ten
/// short lines), and the invoked line is inside the same bound; 1,000
/// skills are found well within the turn's time limit.
#[test]
fn a_small_cap_still_lists_skills_and_counts_the_invoked_line() {
    let t = tree();
    let long = "x".repeat(1024);
    for i in 0..1000 {
        let n = format!("s{i:04}-{}", "n".repeat(50));
        put(
            &t.home.join(&n).join("SKILL.md"),
            format!("---\nname: {n}\ndescription: {long}\n---\nb\n"),
        );
    }
    let at = std::time::Instant::now();
    let mut c = skills::discover(&[node_root(&t.home)], &[]).catalogue;
    assert!(
        at.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        at.elapsed()
    );
    assert_eq!(c.skills.len(), 1000);
    let floor = |r: &str| r.lines().filter(|l| l.starts_with("- s")).count();
    for cap in [
        skills::cap_for(Some(8192)),
        skills::cap_for(Some(4096)),
        100,
    ] {
        c.invoked = None;
        let plain = skills::render(&c, cap).unwrap();
        assert!(floor(&plain) >= 9, "{cap}: {plain}");
        c.invoked = Some(c.skills[0].name.clone());
        let with = skills::render(&c, cap).unwrap();
        assert!(
            with.ends_with("Load it with the `skill` tool first.]"),
            "{with}"
        );
        // The floor's bound holds with the invoked line in it.
        let bound = skills::LEAD.len() + 2_400 + 300;
        assert!(with.len() <= bound, "{cap}: {} > {bound}", with.len());
    }
    // At the full cap the invoked line is inside it.
    c.invoked = Some(c.skills[0].name.clone());
    assert!(skills::render(&c, skills::CAP).unwrap().len() <= skills::CAP);
}
