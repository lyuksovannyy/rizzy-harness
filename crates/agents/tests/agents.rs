use harness_agents::{AgentError, AgentSet, DEFAULT_AGENT, builtins, parse};

/// Every tool an agent could be offered: a built-in's list is checked
/// against all of them, so "Scout cannot write", "Researcher has no files"
/// and "Assistant has no bash" stay true.
const ALL_TOOLS: &[&str] = &[
    "bash",
    "bash_output",
    "bash_stop",
    "tool_output",
    "read",
    "write",
    "edit",
    "grep",
    "glob",
    "outline",
    "graph_query",
    "publish",
    "todo",
    "web_search",
    "web_fetch",
    "remember",
    "delegate",
];

/// The roster, as the owner specified it: one row per built-in.
#[test]
fn builtin_roster_is_exactly_the_table() {
    const DEV: &[&str] = &[
        "bash",
        "bash_output",
        "bash_stop",
        "tool_output",
        "read",
        "write",
        "edit",
        "grep",
        "glob",
        "outline",
        "graph_query",
        "publish",
        "todo",
        "web_search",
        "web_fetch",
        "remember",
    ];
    struct Row {
        id: &'static str,
        aliases: &'static [&'static str],
        name: &'static str,
        category: &'static str,
        /// (agent, sub-agent)
        runs_as: (bool, bool),
        tools: &'static [&'static str],
        user_facts: bool,
        project_memory: bool,
        delegates: &'static [&'static str],
    }
    let table = [
        Row {
            id: "general",
            aliases: &["ai"],
            name: "Assistant",
            category: "General",
            runs_as: (true, false),
            tools: &["web_search", "web_fetch", "remember"],
            user_facts: true,
            project_memory: true,
            delegates: &[],
        },
        Row {
            id: "smol",
            aliases: &[],
            name: "Smol",
            category: "General",
            runs_as: (true, true),
            tools: &[],
            user_facts: false,
            project_memory: false,
            delegates: &[],
        },
        Row {
            id: "researcher",
            aliases: &["research"],
            name: "Researcher",
            category: "General",
            runs_as: (true, true),
            tools: &["web_search", "web_fetch", "remember"],
            user_facts: false,
            project_memory: true,
            delegates: &[],
        },
        Row {
            id: "coder",
            aliases: &["dev"],
            name: "Software Developer",
            category: "IT",
            runs_as: (true, true),
            tools: DEV,
            user_facts: false,
            project_memory: true,
            delegates: &["scout", "researcher", "reviewer"],
        },
        Row {
            id: "teamlead",
            aliases: &[],
            name: "Senior Teamlead",
            category: "IT",
            runs_as: (true, false),
            tools: DEV,
            user_facts: true,
            project_memory: true,
            delegates: &["coder", "researcher", "writer", "scout", "reviewer"],
        },
        Row {
            id: "writer",
            aliases: &[],
            name: "Writer",
            category: "General",
            runs_as: (true, true),
            tools: &[
                "read",
                "write",
                "edit",
                "grep",
                "glob",
                "web_search",
                "web_fetch",
                "publish",
                "todo",
                "remember",
            ],
            user_facts: false,
            project_memory: true,
            delegates: &[],
        },
        Row {
            id: "scout",
            aliases: &[],
            name: "Scout",
            category: "IT",
            runs_as: (false, true),
            tools: &["read", "grep", "glob", "outline", "graph_query"],
            user_facts: false,
            project_memory: false,
            delegates: &[],
        },
        Row {
            id: "reviewer",
            aliases: &[],
            name: "Reviewer",
            category: "IT",
            runs_as: (true, true),
            tools: &["read", "grep", "glob", "outline", "graph_query", "remember"],
            user_facts: false,
            project_memory: true,
            delegates: &[],
        },
    ];
    let all = builtins();
    let ids: Vec<_> = all.iter().map(|a| a.id.as_str()).collect();
    let want: Vec<_> = table.iter().map(|r| r.id).collect();
    assert_eq!(ids, want);
    for (a, r) in all.iter().zip(&table) {
        let id = r.id;
        assert_eq!(a.aliases, r.aliases, "{id}");
        assert_eq!(a.name, r.name, "{id}");
        assert_eq!(a.category, r.category, "{id}");
        assert_eq!(
            (a.features.as_agent, a.features.as_subagent),
            r.runs_as,
            "{id}"
        );
        for t in ALL_TOOLS {
            // `delegate` is offered from `delegates`, never from the list.
            let want = r.tools.contains(t);
            assert_eq!(a.spec.tools.allows(t), want, "{id}: {t}");
        }
        assert_eq!(a.features.memory, r.user_facts, "{id}: user facts");
        assert_eq!(a.features.project_memory, r.project_memory, "{id}");
        assert_eq!(a.delegates, r.delegates, "{id}");
        assert_eq!(a.spec.caps.is_some(), !r.delegates.is_empty(), "{id}");
        assert!(a.builtin && !a.spec.system_prompt.is_empty(), "{id}");
        assert!(!a.description.is_empty(), "{id}");
    }
    // Old Ultracode's caps carried over to the Teamlead.
    let lead = all.iter().find(|a| a.id == "teamlead").unwrap();
    let caps = lead.spec.caps.unwrap();
    assert_eq!((caps.max_subagents, caps.max_tokens), (4, 200_000));
    // The built-ins claim no handle twice: nothing is dropped from them.
    let (set, errors) = AgentSet::load(std::path::Path::new("/nonexistent"));
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(set.resolve("ai").unwrap().id, "general");
    assert_eq!(set.resolve("dev").unwrap().id, "coder");
    assert_eq!(set.resolve("research").unwrap().id, "researcher");
    assert!(set.get("ultracode").is_none(), "removed");
}

/// A file that existed before the roster, whose id is now a built-in's
/// alias, still loads: the id wins, the built-in loses that alias, and
/// the drop is reported (the server logs it).
#[test]
fn a_file_whose_id_is_a_new_alias_keeps_loading() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("dev.md"),
        "---\nname: My dev\ntools: [read]\n---\nMine.\n",
    )
    .unwrap();
    let (set, errors) = AgentSet::load(dir.path());
    assert_eq!(set.get("dev").unwrap().name, "My dev");
    assert_eq!(set.resolve("dev").unwrap().id, "dev");
    assert!(set.get("coder").unwrap().aliases.is_empty());
    assert!(
        matches!(errors.as_slice(), [AgentError::AliasTaken(c)] if c.id == "coder" && c.alias == "dev"),
        "{errors:?}"
    );
    // The log line names both agents.
    assert_eq!(
        errors[0].to_string(),
        "alias @dev of the Software Developer agent (coder) dropped: the My dev agent (dev) \
         already has it as its id"
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
    assert_eq!(set.list().len(), 9);

    let (empty, errs) = AgentSet::load(&dir.path().join("missing"));
    assert!(errs.is_empty());
    assert_eq!(empty.list().len(), 8);
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
    assert!(scout.read_files && !scout.web && !scout.memory);
    assert!(!scout.edit_files && !scout.commands && !scout.delegate);
    assert!(!scout.as_agent && scout.as_subagent);
    let coder = get("coder").features;
    assert!(coder.read_files && coder.edit_files && coder.commands && coder.web);
    assert!(coder.delegate && !coder.memory);
    let lead = get("teamlead").features;
    assert!(lead.delegate && lead.memory && lead.as_agent && !lead.as_subagent);
    let general = get("general").features;
    assert!(general.web && general.memory && !general.read_files && !general.commands);
    assert!(all.iter().all(|a| a.origin == Origin::Builtin));

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
            aliases: vec![],
            category: None,
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
            aliases: vec![],
            category: None,
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
            aliases: vec![],
            category: None,
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
            "smol",
            "researcher",
            "coder",
            "teamlead",
            "writer",
            "scout",
            "reviewer",
            "aide",
            "lead"
        ]
    );
    let lead = set.get("lead").unwrap();
    assert_eq!(
        lead.delegates,
        ["smol", "researcher", "coder", "writer", "scout", "reviewer"],
        "every sub-agent but itself, not aide nor the agent-only built-ins"
    );
    assert_eq!(
        set.get("teamlead").unwrap().delegates,
        ["coder", "researcher", "writer", "scout", "reviewer"]
    );
    assert_eq!(set.default_id(), DEFAULT_AGENT);

    // Taking Scout out of the sub-agents takes it out of every team.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("scout.md"),
        "---\nname: Scout\ntools: [read]\navailable_as: [agent]\n---\nLook.\n",
    )
    .unwrap();
    let (quiet, _) = AgentSet::load(dir.path());
    let (set, _) = quiet.with_edits(&Edits::default());
    assert_eq!(
        set.get("teamlead").unwrap().delegates,
        ["coder", "researcher", "writer", "reviewer"]
    );
    assert_eq!(
        set.get("coder").unwrap().delegates,
        ["researcher", "reviewer"]
    );
}

#[test]
fn aliases_and_category_are_optional_and_checked() {
    let def = |extra: &str| {
        parse(
            "helper",
            &format!("---\nname: H\ntools: []\n{extra}---\nhi\n"),
            false,
        )
    };
    // Definitions from before the fields still parse, in "General".
    let plain = def("").unwrap();
    assert!(plain.aliases.is_empty());
    assert_eq!(plain.category, "General");
    let named = def("aliases: [h, aide]\ncategory: IT\n").unwrap();
    assert_eq!(named.aliases, ["h", "aide"]);
    assert_eq!(named.category, "IT");
    // Labels change no behaviour, so no hash.
    assert_eq!(plain.spec.prompt_hash, named.spec.prompt_hash);
    // Listed for the UI.
    let v = serde_json::to_value(&named).unwrap();
    assert_eq!(v["aliases"], serde_json::json!(["h", "aide"]));
    assert_eq!(v["category"], "IT");
    for bad in [
        "aliases: [Bad]\n".to_string(),
        "aliases: [helper]\n".to_string(),
        "aliases: [h, h]\n".to_string(),
        "category: \"a\\u0007b\"\n".to_string(),
        format!("category: {}\n", "c".repeat(33)),
    ] {
        assert!(
            matches!(def(&bad), Err(AgentError::Frontmatter { .. })),
            "{bad}"
        );
    }
    assert_eq!(def("category: \"  \"\n").unwrap().category, "General");
}

#[test]
fn ids_and_aliases_share_one_namespace() {
    use harness_agents::edits::{Custom, Edits};
    let dir = tempfile::tempdir().unwrap();
    let write = |id: &str, extra: &str| {
        std::fs::write(
            dir.path().join(format!("{id}.md")),
            format!("---\nname: {id}\ntools: [read]\n{extra}---\nhi\n"),
        )
        .unwrap();
    };
    write("helper", "aliases: [h, aide]\n");
    // An id beats an alias: `aide` is an agent of its own.
    write("aide", "");
    // The first to claim an alias keeps it (files load in name order).
    write("other", "aliases: [h]\n");
    // Delegates may name an alias; the roster holds the id.
    write("boss", "delegates: [h, other]\navailable_as: [agent]\n");
    let (set, errors) = AgentSet::load(dir.path());
    let taken: Vec<String> = errors
        .iter()
        .filter(|e| matches!(e, AgentError::AliasTaken(_)))
        .map(|e| e.to_string())
        .collect();
    assert_eq!(taken.len(), 2, "{errors:?}");
    assert!(taken.iter().any(|e| e.contains("@aide")), "{taken:?}");
    assert_eq!(set.get("helper").unwrap().aliases, ["h"]);
    assert!(set.get("other").unwrap().aliases.is_empty());
    assert_eq!(set.resolve("h").unwrap().id, "helper");
    assert_eq!(set.resolve("aide").unwrap().id, "aide");
    assert_eq!(set.resolve("helper").unwrap().id, "helper");
    assert!(set.resolve("nobody").is_none());
    assert!(set.get("h").is_none(), "get takes ids only");
    assert_eq!(set.get("boss").unwrap().delegates, ["helper", "other"]);

    // Custom agents come last: an alias someone has is dropped, reported.
    let custom = |aliases: &[&str]| Custom {
        name: "C".into(),
        summary: String::new(),
        prompt: "c".into(),
        features: features(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
        category: None,
    };
    let mut edits = Edits::default();
    edits.custom.insert("c1".into(), custom(&["h", "mine"]));
    edits
        .custom
        .insert("c2".into(), custom(&["mine", "boss", "free"]));
    let (set, errors) = set.with_edits(&edits);
    assert_eq!(set.get("c1").unwrap().aliases, ["mine"]);
    assert_eq!(set.get("c2").unwrap().aliases, ["free"]);
    assert_eq!(set.resolve("free").unwrap().id, "c2");
    assert_eq!(set.get("c2").unwrap().category, "General");
    assert_eq!(errors.len(), 3, "{errors:?}");
}

#[test]
fn custom_agents_from_before_the_labels_still_read() {
    let c: harness_agents::edits::Custom = serde_json::from_str(
        r#"{"name":"Old","summary":"","prompt":"p","features":{"memory":true,"web":false,
        "read_files":true,"edit_files":false,"commands":false,"delegate":false,
        "as_agent":true,"as_subagent":true}}"#,
    )
    .unwrap();
    assert!(c.aliases.is_empty() && c.category.is_none());
}

/// The task list comes with the switches that build things, as `publish`
/// does: a custom agent that edits or runs commands keeps a plan.
#[test]
fn agents_that_build_get_the_task_list() {
    let with = |edit_files: bool, commands: bool| {
        harness_agents::Features {
            edit_files,
            commands,
            ..features()
        }
        .tools()
    };
    assert!(with(true, false).iter().any(|t| t == "todo"));
    assert!(with(false, true).iter().any(|t| t == "todo"));
    assert!(!with(false, false).iter().any(|t| t == "todo"));
}
