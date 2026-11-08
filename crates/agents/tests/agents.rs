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
fn ui_level_parses_defaults_to_rich_and_changes_the_hash() {
    use harness_core::agent::UiLevel;
    let with = |ui: &str| {
        parse(
            "x",
            &format!("---\nname: X\ntools: all\n{ui}---\nhi\n"),
            false,
        )
    };
    let rich = with("").unwrap();
    assert_eq!(rich.spec.ui, UiLevel::Rich);
    let display = with("ui: display\n").unwrap();
    assert_eq!(display.spec.ui, UiLevel::Display);
    assert_eq!(with("ui: off\n").unwrap().spec.ui, UiLevel::Off);
    assert_eq!(
        with("ui: rich\n").unwrap().spec.prompt_hash,
        rich.spec.prompt_hash
    );
    assert_ne!(display.spec.prompt_hash, rich.spec.prompt_hash);
    assert!(matches!(
        with("ui: fancy\n"),
        Err(AgentError::Frontmatter { .. })
    ));
    assert!(builtins().iter().all(|a| a.spec.ui == UiLevel::Rich));
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

fn features() -> harness_agents::Features {
    harness_agents::Features {
        memory: true,
        project_memory: true,
        web: true,
        read_files: true,
        edit_files: false,
        commands: false,
        delegate: false,
        as_agent: true,
        as_subagent: true,
    }
}

#[test]
fn features_are_read_off_the_definitions() {
    use harness_agents::Origin;
    let all = builtins();
    let get = |id: &str| all.iter().find(|a| a.id == id).unwrap();
    let scout = get("scout").features;
    assert!(scout.read_files && scout.web && scout.memory);
    assert!(!scout.edit_files && !scout.commands && !scout.delegate);
    let coder = get("coder").features;
    assert!(coder.read_files && coder.edit_files && coder.commands && !coder.web);
    let ultra = get("ultracode").features;
    assert!(ultra.delegate && ultra.as_agent && !ultra.as_subagent);
    assert!(
        all.iter()
            .all(|a| a.origin == Origin::Builtin && a.features.as_agent)
    );

    let file = parse(
        "quiet",
        "---\nname: Q\ntools: all\nmemory: false\navailable_as: [subagent]\n---\nhi\n",
        false,
    )
    .unwrap();
    assert_eq!(file.origin, Origin::File);
    assert!(!file.features.memory && !file.features.as_agent && file.features.as_subagent);
    let with_memory = parse(
        "quiet",
        "---\nname: Q\ntools: all\navailable_as: [subagent]\n---\nhi\n",
        false,
    )
    .unwrap();
    assert_ne!(file.spec.prompt_hash, with_memory.spec.prompt_hash);

    // Project memory: on unless the file says otherwise; saying "on" changes
    // nothing, so agents from before the switch keep their hash.
    let defined = |extra: &str| {
        parse(
            "notes",
            &format!("---\nname: N\ntools: [read, remember]\n{extra}---\nhi\n"),
            false,
        )
        .unwrap()
    };
    let (plain, on, off) = (
        defined(""),
        defined("project_memory: true\n"),
        defined("project_memory: false\n"),
    );
    assert!(plain.features.project_memory && !off.features.project_memory);
    assert_eq!(plain.spec.prompt_hash, on.spec.prompt_hash);
    assert_ne!(plain.spec.prompt_hash, off.spec.prompt_hash);
    assert!(matches!(
        parse(
            "x",
            "---\nname: X\ntools: all\navailable_as: []\n---\nhi\n",
            false
        ),
        Err(AgentError::Unavailable(_))
    ));
}

#[test]
fn custom_agents_get_tools_and_budget_from_their_features() {
    use harness_core::agent::{Budget, ToolSelection};
    let a = harness_agents::custom(
        "reviewer",
        "Reviewer",
        "Reviews.",
        "Review it.",
        features(),
        vec!["coder".into()],
    )
    .unwrap();
    let ToolSelection::List(tools) = &a.spec.tools else {
        panic!("a list")
    };
    assert_eq!(
        tools,
        &[
            "remember",
            "web_search",
            "web_fetch",
            "read",
            "grep",
            "glob",
            "outline",
            "graph_query"
        ]
    );
    assert_eq!(a.spec.budget, Budget::Lean);
    assert!(a.delegates.is_empty(), "delegate is off");
    assert_eq!(a.features, features());

    let busy = harness_agents::Features {
        commands: true,
        delegate: true,
        ..features()
    };
    let b =
        harness_agents::custom("lead", "Lead", "", "Lead.", busy, vec!["coder".into()]).unwrap();
    assert_eq!(b.spec.budget, Budget::Standard);
    assert_eq!(b.delegates, ["coder"]);
    assert!(b.spec.caps.is_some());

    // `remember` comes with either memory; the master offers it per turn.
    let tools = |f: harness_agents::Features| {
        let a = harness_agents::custom("m", "M", "", "m", f, vec![]).unwrap();
        a.spec.tools.allows("remember")
    };
    let project_only = harness_agents::Features {
        memory: false,
        ..features()
    };
    assert!(tools(project_only));
    assert!(!tools(harness_agents::Features {
        project_memory: false,
        ..project_only
    }));

    let none = harness_agents::Features {
        as_agent: false,
        as_subagent: false,
        ..features()
    };
    assert!(matches!(
        harness_agents::custom("x", "X", "", "x", none, vec![]),
        Err(AgentError::Unavailable(_))
    ));
    assert!(matches!(
        harness_agents::custom("x", "X", "", "  ", features(), vec![]),
        Err(AgentError::EmptyPrompt(_))
    ));
}

#[test]
fn edits_add_summaries_instructions_and_custom_agents() {
    use harness_agents::edits::{ADDITIONAL, Custom, Edits, Overlay};
    let base = AgentSet::from_agents(builtins(), DEFAULT_AGENT);
    let coder = base.get("coder").unwrap().clone();
    let mut edits = Edits::default();
    edits.overlays.insert(
        "coder".into(),
        Overlay {
            summary: "Our engineer.".into(),
            instructions: "Use pnpm.".into(),
        },
    );
    edits.overlays.insert(
        "writer".into(),
        Overlay {
            summary: "  ".into(),
            instructions: String::new(),
        },
    );
    edits.custom.insert(
        "lead".into(),
        Custom {
            name: "Lead".into(),
            summary: "Plans.".into(),
            prompt: "Plan.".into(),
            features: harness_agents::Features {
                delegate: true,
                ..features()
            },
        },
    );
    edits.custom.insert(
        "aide".into(),
        Custom {
            name: "Aide".into(),
            summary: String::new(),
            prompt: "Help.".into(),
            features: harness_agents::Features {
                as_subagent: false,
                ..features()
            },
        },
    );
    // A custom agent cannot take a built-in's id.
    edits.custom.insert(
        "scout".into(),
        Custom {
            name: "S".into(),
            summary: String::new(),
            prompt: "s".into(),
            features: features(),
        },
    );
    let (set, errors) = base.with_edits(&edits);
    assert!(
        matches!(errors.as_slice(), [AgentError::Shadowed(id)] if id == "scout"),
        "{errors:?}"
    );

    let edited = set.get("coder").unwrap();
    assert_eq!(edited.description, "Our engineer.");
    assert_eq!(
        edited.spec.system_prompt,
        format!("{}\n\n{ADDITIONAL}\n\nUse pnpm.", coder.spec.system_prompt)
    );
    assert_ne!(edited.spec.prompt_hash, coder.spec.prompt_hash);
    let writer = set.get("writer").unwrap();
    assert_eq!(
        writer,
        base.get("writer").unwrap(),
        "blank fields change nothing"
    );

    let ids: Vec<&str> = set.list().iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "general",
            "coder",
            "writer",
            "scout",
            "ultracode",
            "aide",
            "lead"
        ]
    );
    let lead = set.get("lead").unwrap();
    assert_eq!(
        lead.delegates,
        ["general", "coder", "writer", "scout"],
        "every sub-agent but itself, not aide"
    );
    assert_eq!(
        set.get("ultracode").unwrap().delegates,
        ["coder", "scout", "writer"]
    );
    assert_eq!(set.default_id(), DEFAULT_AGENT);

    // Taking Scout out of the sub-agents takes it out of Ultracode's team.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("scout.md"),
        "---\nname: Scout\ntools: [read]\navailable_as: [agent]\n---\nLook.\n",
    )
    .unwrap();
    let (quiet, _) = AgentSet::load(dir.path());
    let (set, _) = quiet.with_edits(&Edits::default());
    assert_eq!(set.get("ultracode").unwrap().delegates, ["coder", "writer"]);
}
