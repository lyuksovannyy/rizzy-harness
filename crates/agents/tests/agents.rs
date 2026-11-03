use harness_agents::{AgentError, AgentSet, DEFAULT_AGENT, builtins, parse};

#[test]
fn builtins_parse_and_scout_is_read_only() {
    let all = builtins();
    let ids: Vec<_> = all.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["general", "coder", "writer", "scout", "ultracode"]);
    let ultra = all.iter().find(|a| a.id == "ultracode").unwrap();
    assert_eq!(ultra.delegates, ["coder", "scout", "writer"]);
    let caps = ultra.spec.caps.unwrap();
    assert_eq!((caps.max_subagents, caps.max_tokens), (4, 200_000));
    assert!(
        all.iter()
            .filter(|a| a.id != "ultracode")
            .all(|a| a.delegates.is_empty() && a.spec.caps.is_none())
    );
    let scout = all.iter().find(|a| a.id == "scout").unwrap();
    for t in ["bash", "write", "edit"] {
        assert!(!scout.spec.tools.allows(t), "scout must not get {t}");
    }
    assert!(scout.spec.tools.allows("read"));
    assert!(
        all.iter()
            .all(|a| a.builtin && !a.spec.system_prompt.is_empty())
    );
}

#[test]
fn parse_fields_and_hash_changes_with_prompt() {
    let a = parse(
        "reviewer",
        "---\nname: Reviewer\ndescription: Reviews code.\ntools: [read, grep]\nmodel: some-model\neffort: high\nbudget: standard\n---\nReview carefully.\n",
        false,
    )
    .unwrap();
    assert_eq!(a.name, "Reviewer");
    assert_eq!(a.spec.model.as_deref(), Some("some-model"));
    assert_eq!(a.spec.effort.as_deref(), Some("high"));
    assert_eq!(a.spec.system_prompt, "Review carefully.");
    let b = parse(
        "reviewer",
        "---\nname: Reviewer\ntools: [read, grep]\nmodel: some-model\neffort: high\n---\nReview very carefully.\n",
        false,
    )
    .unwrap();
    assert_ne!(a.spec.prompt_hash, b.spec.prompt_hash);
    assert_eq!(a.spec.agent_ref().agent_id, "reviewer");
}

#[test]
fn rejects_bad_definitions() {
    assert!(matches!(
        parse("x", "no frontmatter", false),
        Err(AgentError::NoFrontmatter(_))
    ));
    assert!(matches!(
        parse("x", "---\nname: X\ntools: all\nbogus: 1\n---\nhi\n", false),
        Err(AgentError::Frontmatter { .. })
    ));
    assert!(matches!(
        parse(
            "x",
            "---\nname: X\ntools: all\neffort: ultra\n---\nhi\n",
            false
        ),
        Err(AgentError::BadEffort { .. })
    ));
    assert!(matches!(
        parse("x", "---\nname: X\ntools: all\n---\n   \n", false),
        Err(AgentError::EmptyPrompt(_))
    ));
    assert!(matches!(
        parse("Bad Id", "---\nname: X\ntools: all\n---\nhi\n", false),
        Err(AgentError::BadId(_))
    ));
}

#[test]
fn user_dir_overrides_and_extends_and_skips_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("coder.md"),
        "---\nname: My Coder\ntools: [read]\n---\nCustom.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("translator.md"),
        "---\nname: Translator\ntools: []\n---\nTranslate.\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("broken.md"), "nope").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
    let (set, errors) = AgentSet::load(dir.path());
    assert_eq!(errors.len(), 1, "{errors:?}");
    let coder = set.get("coder").unwrap();
    assert_eq!(coder.name, "My Coder");
    assert!(!coder.builtin);
    assert!(set.get("translator").is_some());
    assert_eq!(set.default_id(), DEFAULT_AGENT);
    assert_eq!(set.list().len(), 6);

    let (empty, errs) = AgentSet::load(&dir.path().join("missing"));
    assert!(errs.is_empty());
    assert_eq!(empty.list().len(), 5);
}
