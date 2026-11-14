//! The single `harness` binary.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod listen;
mod lock;
mod net;
mod stop;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use harness_agent_native::{NativeBackend, NativeConfig, Provider};
use harness_core::event::Profile;
use harness_core::{BRAND, CONFIG_FILE};
use harness_master::{Master, auth};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};

use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;

#[derive(Parser)]
#[command(name = BRAND, version, about = "Self-hosted coding-agent harness")]
struct Cli {
    /// Data directory (default: ~/.local/share/rizzy-harness).
    #[arg(long, global = true, env = "HARNESS_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the master with an embedded node and the web UI.
    Serve {
        /// Address to listen on. Non-loopback requires --insecure-bind until TLS lands.
        #[arg(long, env = "HARNESS_BIND", default_value = "127.0.0.1:7777")]
        bind: SocketAddr,
        /// Allow a non-loopback address that is not a WireGuard interface
        /// (only on a network you trust). WireGuard addresses need no flag.
        #[arg(
            long,
            env = "HARNESS_INSECURE_BIND",
            value_parser = clap::builder::BoolishValueParser::new()
        )]
        insecure_bind: bool,
        /// Serve the UI over HTTPS with a self-signed certificate (trust
        /// `keys/ui-cert.pem` once). Needed off-localhost for notifications
        /// and the microphone: browsers only allow them on secure pages.
        #[arg(
            long,
            env = "HARNESS_TLS",
            value_parser = clap::builder::BoolishValueParser::new()
        )]
        tls: bool,
        /// Extra `host:port` values accepted in Host/Origin headers.
        #[arg(
            long = "allowed-host",
            env = "HARNESS_ALLOWED_HOSTS",
            value_delimiter = ','
        )]
        allowed_hosts: Vec<String>,
        /// Accept remote worker nodes here (TLS with a pinned self-signed
        /// certificate, Ed25519 node keys). Off unless given.
        #[arg(long, env = "HARNESS_NODE_LISTEN")]
        node_listen: Option<SocketAddr>,
        /// Run no agentic node here: chats only, agentic sessions go to
        /// remote nodes. Used by the container image (nodes need a real host).
        #[arg(
            long,
            env = "HARNESS_NO_LOCAL_NODE",
            value_parser = clap::builder::BoolishValueParser::new()
        )]
        no_local_node: bool,
    },
    /// Print the API token (for scripts; the UI uses username + password).
    Token,
    /// Print the one-time code that claims a fresh install.
    ClaimCode,
    /// Check the environment.
    Doctor,
    /// Node commands.
    Node {
        #[command(subcommand)]
        cmd: NodeCmd,
    },
    /// Optional graphify knowledge graph for agents (third-party program).
    Graphify {
        #[command(subcommand)]
        cmd: GraphifyCmd,
    },
    /// Run a command in a Landlock sandbox: apply the rules to this process,
    /// then exec the command. The node starts bash and graphify this way
    /// (node.toml `[sandbox]`).
    SandboxExec {
        /// A file or folder the command may read and change, with what the
        /// node expects there: `<dev>:<ino>:<label>` (its identity; any other
        /// directory there refuses the command) or `system` (a folder no
        /// session can write).
        #[arg(long, num_args = 2, value_names = ["EXPECT", "PATH"], action = clap::ArgAction::Append)]
        rw: Vec<std::ffi::OsString>,
        /// A file or folder the command may read and execute, the same way.
        #[arg(long, num_args = 2, value_names = ["EXPECT", "PATH"], action = clap::ArgAction::Append)]
        ro: Vec<std::ffi::OsString>,
        /// Deny TCP connect and bind (needs Landlock ABI 4).
        #[arg(long)]
        no_network: bool,
        /// Run with whatever the kernel enforces instead of refusing.
        #[arg(long)]
        best_effort: bool,
        /// Print what is enforced on stderr first.
        #[arg(long)]
        status: bool,
        /// The node's marker (T.8e): its temp root, so its init's command
        /// line names the node that started it. Not used here.
        #[arg(long, hide = true)]
        mark: Option<std::ffi::OsString>,
        /// The session of the marker. Not used here.
        #[arg(long, hide = true)]
        mark_session: Option<std::ffi::OsString>,
        /// The start's self-test (T.8c, review R-3): checks this process
        /// runs after it confined itself, in place of a command. A file that
        /// must be readable.
        #[arg(long, hide = true, action = clap::ArgAction::Append)]
        probe_read: Vec<std::ffi::OsString>,
        /// A file that must not be readable.
        #[arg(long, hide = true, action = clap::ArgAction::Append)]
        probe_unreadable: Vec<std::ffi::OsString>,
        /// A path that must not exist at all (the file view).
        #[arg(long, hide = true, action = clap::ArgAction::Append)]
        probe_absent: Vec<std::ffi::OsString>,
        /// This process must be pid 2 of a PID namespace of its own.
        #[arg(long, hide = true)]
        probe_pid_ns: bool,
        /// `/proc` must be that namespace's own.
        #[arg(long, hide = true)]
        probe_own_proc: bool,
        /// The network namespace must be another than this one (the node's).
        #[arg(long, hide = true)]
        probe_net_not: Option<std::ffi::OsString>,
        /// The command and its arguments (none for a probe).
        #[arg(last = true, value_name = "CMD")]
        command: Vec<std::ffi::OsString>,
    },
    /// Read a PDF on stdin, write its text on stdout, confined (no files, no
    /// network, limited memory). The server starts it for PDF attachments.
    #[command(hide = true)]
    PdfText,
}

#[derive(Subcommand)]
enum GraphifyCmd {
    /// Install the pinned graphify version for this user (uv or pipx).
    /// Then allow it with `graphify = true` in node.toml and restart.
    Install,
    /// Show whether graphify is installed and allowed here.
    Status,
}

#[derive(Subcommand)]
enum NodeCmd {
    /// Register this machine with a master (once). Get the command from the
    /// master's Settings → Nodes → Add node.
    Join {
        /// `wss://host:port` of the master's node listener.
        master_url: String,
        /// The single-use join token. Prefer the environment variable: an
        /// argument is visible to other users in the process list.
        #[arg(long, env = "HARNESS_JOIN_TOKEN", hide_env_values = true)]
        token: String,
        /// The master certificate fingerprint shown with the token.
        #[arg(long)]
        fingerprint: String,
    },
    /// Run as a worker node for the master joined before.
    Run,
}

/// `<data>/remote.toml`: where this node's master is.
#[derive(serde::Serialize, serde::Deserialize)]
struct RemoteToml {
    url: String,
    fingerprint: String,
    node_id: String,
}

/// Every database of the data directory, read only, before any is opened
/// (fix plan 4.3, review R43-3): one written by a newer build stops the
/// start before anything is created, migrated or swept, so no file is
/// migrated before a later one turns out to be newer.
fn check_schemas(dir: &Path) -> anyhow::Result<()> {
    use harness_store::migrate::check_file;
    for name in ["master.db", "chat.db", "node.db"] {
        check_file(&dir.join(name), &harness_store::DB_SCHEMA)?;
    }
    check_file(&dir.join("memory.db"), &harness_memory::DB_SCHEMA)?;
    Ok(())
}

fn data_dir(cli: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let d = match cli {
        Some(d) => d.clone(),
        None => dirs::data_local_dir()
            .context("no data directory; pass --data-dir")?
            .join(BRAND),
    };
    // Made 0700 from the start (never visible to others under the umask
    // for a moment); its parents as usual. An existing one must be this
    // user's, and is made 0700; a chmod that fails stops the start (Gate 3
    // review N-3).
    if let Some(parent) = d.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut mk = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut mk, 0o700);
    match mk.create(&d) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("creating {}", d.display())),
    }
    harness_master::secrets::check_data_dir(&d)?;
    // Absolute and real, once, here: everything below (the sandbox's temp
    // folder and protected folder among it) runs in other folders, where a
    // relative path would name something else (R.1, S-4).
    d.canonicalize()
        .with_context(|| format!("resolving {}", d.display()))
}

fn hostname() -> String {
    nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into())
}

#[allow(clippy::too_many_arguments)]
async fn serve(
    dir: &Path,
    bind: SocketAddr,
    insecure_bind: bool,
    tls: bool,
    extra_hosts: Vec<String>,
    node_listen: Option<SocketAddr>,
    no_local_node: bool,
    dotenv_names: &[String],
) -> anyhow::Result<()> {
    match net::exposure(bind, insecure_bind, net::in_container())? {
        net::Exposure::Loopback => {}
        net::Exposure::WireGuard(iface) => {
            tracing::info!(%bind, %iface, "serving on a WireGuard interface (peers only)");
            if !tls {
                tracing::warn!(
                    "without --tls, browsers treat this page as insecure: notifications and \
                     the microphone will not work outside localhost"
                );
            }
        }
        net::Exposure::Container => tracing::info!(
            %bind,
            "in a container: reachable only where the port is published (publish it on \
             127.0.0.1 or the WireGuard address, never 0.0.0.0)"
        ),
        net::Exposure::Insecure => tracing::warn!(
            %bind,
            "listening on an address that is not WireGuard (--insecure-bind){}. Use only on a \
             network you trust.",
            if tls { "" } else { " and WITHOUT TLS: passwords and cookies travel in clear text" }
        ),
    }
    // First, before anything starts that runs commands (security review
    // S2-1): every loopback address accepted as this server's own is held
    // by it, and a start where another program holds one ends here: no
    // database, token or key in the data directory has been opened (the
    // directory's lock was taken and is released as the process ends).
    let ui = listen::bind(bind).await?;
    let env = harness_master::env::EnvSettings::from_env();
    if !env.locked().is_empty() {
        tracing::info!(
            settings = ?env.locked().iter().map(|l| l.var).collect::<Vec<_>>(),
            "settings fixed by the environment (.env)"
        );
    }
    // Every file holding a secret, before any is read (AUDIT L7): the API
    // token and the claim code are checked where they are read.
    harness_master::secrets::check_private(&dir.join("secrets.toml"))?;
    harness_master::secrets::check_keys(dir)?;
    let cfg = Config::load_with_env(&dir.join(CONFIG_FILE), &env)?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let secrets = harness_core::SecretSet::default();
    for v in harness_master::env::secret_values() {
        secrets.add(v);
    }
    // The API token is the machine owner's root key: scrubbed from every
    // event and tool output like a provider key. Loaded before any runtime
    // that scrubs starts.
    let token = auth::load_or_create_token(&api_token_path(dir))?;
    secrets.add(token.clone());
    let (runtime, provider_env) = ProviderRuntime::start(cfg.clone(), dir, secrets.clone())?;
    let child_env = tool_env(&policy, provider_env, dotenv_names)?;
    let runtime = Arc::new(runtime.with_env(env));
    // Both runtimes share one swappable provider: settings can change it live.
    let provider: Arc<dyn Provider> = runtime.provider.clone();
    tracing::info!(
        provider = provider.name(),
        model = cfg.effective_default().unwrap_or_default(),
        "provider"
    );

    let mirror = Store::open(&dir.join("master.db"), Durability::Normal)?;
    // Accounts older than workspaces get their Personal one here, so every
    // user-owned chat has a workspace from the first request on.
    let added = mirror.call(|s| s.ensure_personal_workspaces()).await?;
    if added > 0 {
        tracing::info!(added, "created missing Personal workspaces and memberships");
    }
    let node = if no_local_node {
        tracing::info!("no local node: agentic sessions run on remote nodes");
        None
    } else {
        Some(
            agentic_node(
                dir,
                &cfg,
                policy,
                provider.clone(),
                child_env,
                &secrets,
                "local",
                false,
            )
            .await?,
        )
    };

    // The master's own chat runtime: same session code, no workspace and no
    // file or shell tools; only the web tools from `[web]`.
    let (mut web_tools, web_key) = harness_master::providers::web_tools(&cfg, dir)?;
    let (memory, builtin_memory) = harness_master::providers::memory(&cfg, dir, &secrets)?;
    if memory.is_some() {
        web_tools.push(Arc::new(harness_memory::Remember));
    }
    if let Some(k) = web_key {
        secrets.add(k);
    }
    tracing::info!(
        tools = ?web_tools.iter().map(|t| t.name()).collect::<Vec<_>>(),
        "chat tools"
    );
    let chat_backend = NativeBackend::new(
        provider,
        web_tools,
        NativeConfig {
            secrets: secrets.clone(),
            summarize_after_tokens: cfg.chat.summarize_after_tokens,
            // The generic summary, not the coding one.
            coding_summary: false,
            ..Default::default()
        },
    );
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: hostname(),
        policy: NodePolicy::default(),
        store: Store::open(&dir.join("chat.db"), Durability::Full)?,
        backend: Arc::new(chat_backend),
        default_model: fallback_model(&cfg),
        secrets: secrets.clone(),
        profile: Profile::Chat,
        incognito_ttl: Some(cfg.chat.incognito_ttl()),
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await?;

    // The bound address (port 0 resolved) and the loopback addresses held.
    let addr = ui.addr;
    let mut auth = auth::Auth::new(
        token,
        listen::allowed_hosts(addr, &ui.own, &extra_hosts),
        tls,
    );
    let unclaimed = mirror.call(|s| s.user_count()).await? == 0;
    if unclaimed {
        let path = dir.join("claim-code");
        let code = auth::load_or_create_secret(&path, auth::new_claim_code)?;
        auth = auth.with_claim(auth::Claim { code, path });
    } else {
        // A leftover code from an interrupted claim must not stay usable.
        let _ = std::fs::remove_file(dir.join("claim-code"));
    }
    let master = Master::new(mirror, auth);
    // Before the first node link and the node listener: a replayed publish
    // must find the blob store.
    harness_master::storage::open(&master, dir)?;
    master.set_provider_runtime(runtime);
    // PDF attachments are read by this binary's `pdf-text`, never in here.
    match std::env::current_exe().and_then(|e| e.canonicalize()) {
        Ok(exe) => master.set_pdf_reader(harness_master::pdf::PdfReader::new(exe)),
        Err(e) => tracing::warn!("PDF attachments are off: cannot find this binary: {e}"),
    }
    master.start_incognito_purge(cfg.chat.incognito_ttl());
    master.start_maintenance();
    // Each provider's model listing: at start when older than a day, then daily.
    master.start_model_refresh(harness_master::model_cache::REFRESH_EVERY);
    if let Some(mem) = memory {
        tracing::info!(backend = mem.name(), "memory");
        master.set_memory(mem);
    }
    if let Some(b) = builtin_memory {
        master.set_builtin_memory(b);
    }
    if let Some(t) = harness_master::providers::transcriber(&cfg, dir, &secrets)? {
        master.set_transcriber(t);
    }
    let (agents, agent_errors) = harness_agents::AgentSet::load(&dir.join("agents"));
    for e in &agent_errors {
        match e {
            harness_agents::AgentError::AliasTaken(_) => {
                tracing::warn!(error = %e, "dropping an agent alias")
            }
            _ => tracing::warn!(error = %e, "skipping agent definition"),
        }
    }
    master.set_agents(agents);
    master.load_agent_edits().await?;
    if let Some((node, graphify)) = &node {
        master
            .add_link(node.attach().await, true)
            .await
            .map_err(anyhow::Error::msg)?;
        // The built-in node shares this machine with the master, so the
        // settings page can show its graphify status (read-only).
        master.set_graphify(graphify.clone(), dir.join("node.toml"));
    }
    master
        .add_link(chat.attach().await, true)
        .await
        .map_err(anyhow::Error::msg)?;
    // After both links, so the chats routines start have somewhere to run.
    master.start_routines();
    master.start_callbacks();

    if let Some(addr) = node_listen {
        let cert = Arc::new(harness_link::MasterCert::load_or_create(dir)?);
        let l = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding node listener {addr}"))?;
        let port = l.local_addr()?.port();
        master.set_node_listener(port, cert.fingerprint());
        master.serve_nodes(l, cert.clone())?;
        println!(
            "nodes connect to wss://<this host>:{port} (certificate {})",
            cert.fingerprint()
        );
    }
    // Printed (not logged) so tests and users can find the port; never a secret.
    let scheme = if tls { "https" } else { "http" };
    println!("{BRAND} listening on {scheme}://{addr}");
    if unclaimed {
        println!(
            "first run: open the page and enter the claim code from {} (or run `{BRAND} claim-code`)",
            dir.join("claim-code").display()
        );
    }
    // A stop (A.19): while HTTP still serves, so the UI shows the turns
    // closing, routines start nothing, both local runtimes refuse new work
    // and close their turns; then HTTP gets its grace and ends. Remote
    // nodes keep their turns and reconnect when the server is back.
    let mut signals = stop::Signals::install().context("catching stop signals")?;
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel::<Vec<String>>();
    {
        let (master, chat) = (master.clone(), chat.clone());
        let local = node.as_ref().map(|(n, _)| n.clone());
        tokio::spawn(async move {
            let name = signals.first().await;
            stop::announce(name, "the server");
            signals.force_on_second();
            master.begin_stop();
            let (mut left, more) = tokio::join!(chat.stop(stop::stop_wait()), async {
                match &local {
                    Some(n) => n.stop(stop::stop_wait()).await,
                    None => Vec::new(),
                }
            });
            left.extend(more);
            tokio::join!(chat.flush(stop::FLUSH_WAIT), async {
                if let Some(n) = &local {
                    n.flush(stop::FLUSH_WAIT).await;
                }
            });
            let _ = stopped_tx.send(left.into_iter().map(|s| s.to_string()).collect());
        });
    }
    // `None`: the stop's task ended without its answer (it panicked):
    // that is no clean stop (review R-10).
    let (left_tx, left_rx) = tokio::sync::oneshot::channel::<Option<Vec<String>>>();
    let shutdown = async move {
        let left = stopped_rx.await.ok();
        let _ = left_tx.send(left);
    };
    let limits = harness_master::serve::ServeConfig::default();
    if tls {
        let mut names = vec!["localhost".to_string(), "127.0.0.1".into(), hostname()];
        if !addr.ip().is_unspecified() {
            names.push(addr.ip().to_string());
        }
        names.extend(extra_hosts.iter().map(|h| {
            h.rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(h)
                .trim_matches(['[', ']'])
                .to_string()
        }));
        names.sort();
        names.dedup();
        let cert = harness_link::MasterCert::load_or_create_for(dir, "ui", &names)?;
        println!(
            "HTTPS certificate {} ({}); trust {} once in your browser or OS",
            cert.fingerprint(),
            names.join(", "),
            dir.join("keys/ui-cert.pem").display()
        );
        harness_master::serve::serve_with(
            ui.listeners,
            net::tls_handshake(cert.server_config()?),
            harness_master::router(master),
            limits,
            shutdown,
        )
        .await?;
    } else {
        harness_master::serve::serve(
            ui.listeners,
            harness_master::router(master),
            limits,
            shutdown,
        )
        .await?;
    }
    match left_rx.await {
        Ok(Some(left)) => stopped(left),
        _ => bail!("the stop did not finish (its task ended without closing the turns)"),
    }
}

/// The end of a stop: 0 when every turn closed, else 1 naming the sessions
/// the next start closes as `interrupted`.
fn stopped(left: Vec<String>) -> anyhow::Result<()> {
    if left.is_empty() {
        tracing::info!("stopped: every turn is closed");
        return Ok(());
    }
    bail!(
        "stopped with {} chat(s) not closed: a turn still running after {} s, or a turn's or a \
         background command's end the log could not take (sessions {}): the next start closes \
         them (a turn as interrupted, a background command as ended with the node)",
        left.len(),
        stop::stop_wait().as_secs(),
        left.join(", ")
    )
}

/// The model a node uses when the master sends none (the master resolves
/// the current default for every session it creates).
fn fallback_model(cfg: &Config) -> String {
    cfg.effective_default()
        .unwrap_or_else(|| format!("{}::mock", harness_master::presets::DEMO))
}

/// The environment of every child process the node's tools start, for the
/// embedded node and `node run` alike: the core list plus node.toml's
/// `env_pass`, never a provider key variable, a name `.env` set, or a
/// `HARNESS_*` setting (the join token).
fn tool_env(
    policy: &NodePolicy,
    provider_env: Vec<String>,
    dotenv_names: &[String],
) -> anyhow::Result<harness_tools::ToolEnv> {
    let mut deny = provider_env;
    deny.extend(harness_master::env::hidden_from_tools(dotenv_names));
    policy
        .tool_env(deny)
        .map_err(|e| anyhow::anyhow!("node.toml: {e}"))
}

/// The worker runtime (embedded `local` node, or a remote node).
#[allow(clippy::too_many_arguments)]
async fn agentic_node(
    dir: &Path,
    cfg: &Config,
    policy: NodePolicy,
    provider: Arc<dyn Provider>,
    tool_env: harness_tools::ToolEnv,
    secrets: &harness_core::SecretSet,
    node_id: &str,
    refuse_without_sandbox: bool,
) -> anyhow::Result<(NodeRuntime, Arc<harness_tools::graphify::GraphifyState>)> {
    // No session may include this runtime's data directory (for the
    // embedded node, the master's: keys, the API token, every chat), and
    // no file tool may touch it, whatever node.toml allows (AUDIT S1).
    let mut policy = policy;
    policy.protect(dir);
    // The shell sandbox (FIX_PLAN 2.7): resolved and self-tested once; the
    // execution modes this node offers follow from it (N.10a).
    let (sandbox, host_shell) = start_sandbox(dir, &mut policy, refuse_without_sandbox)?;
    // The node's own git reads a session's folder in the same Landlock
    // rules, never contained (R.1, R1b-2); with `mode = "off"` it runs
    // unconfined.
    use harness_core::policy::ExecMode;
    let git_sandbox = match &host_shell {
        harness_tools::sandbox::Confinement::Confined { sandbox, .. } => Some(sandbox.clone()),
        _ => None,
    };
    tracing::info!("checkpoints: {}", checkpoint_confinement(&host_shell));
    // graphify: only if this node's own policy allows it (node.toml).
    let graphify =
        harness_tools::graphify::GraphifyState::new(policy.graphify, dir.join("graphify"));
    let backend = NativeBackend::new(
        provider,
        agentic_tools(&graphify),
        NativeConfig {
            tool_env,
            tool_stream_bytes: policy.limits.tool_stream_bytes,
            max_steps: policy.limits.max_steps,
            bash_timeout_max: std::time::Duration::from_secs(policy.limits.bash_timeout_max_s),
            secrets: secrets.clone(),
            allow_outside_workspace: policy.allow_outside_workspace,
            protected_dirs: Arc::new(policy.protected_dirs.clone()),
            // Each mode's confinement only where the node offers it (review
            // L-f): a log that names another mode starts nothing.
            sandbox: policy.exec.confinement_for(ExecMode::Sandbox, sandbox),
            host_shell: policy.exec.confinement_for(ExecMode::Host, host_shell),
            // The master puts the provider's default effort on each turn's
            // spec, for models that take one.
            effort: None,
            summarize_after_tokens: cfg.chat.summarize_after_tokens,
            // Agentic chats: summarised for coding work (the chat runtime
            // keeps the generic summary).
            coding_summary: true,
            ..Default::default()
        },
    );
    let rt = NodeRuntime::start(NodeConfig {
        node_id: node_id.into(),
        hostname: hostname(),
        policy,
        store: Store::open(&dir.join("node.db"), Durability::Full)?,
        backend: Arc::new(backend),
        default_model: fallback_model(cfg),
        secrets: secrets.clone(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(dir.join("checkpoints")),
        git_sandbox: git_sandbox.clone(),
        // The data dir is passed as is: for the embedded node it is the
        // master's, so its databases, keys and blobs are never published.
        artifacts: Some(harness_node::artifacts::StageConfig::new(dir.to_path_buf())),
    })
    .await?;
    Ok((rt, graphify))
}

/// The tools an agentic node registers: the built-in ones, `graph_query`
/// and `remember` (it runs nothing here: the master files the mirrored call
/// as a proposal the person confirms; `denied_tools` turns it off). Each
/// turn offers those its agent allows; node policy still gates every call.
fn agentic_tools(graphify: &Arc<harness_tools::graphify::GraphifyState>) -> harness_tools::ToolSet {
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(harness_tools::graphify::GraphQuery::new(
        graphify.clone(),
    )));
    tools.push(Arc::new(harness_memory::Remember));
    tools
}

/// How the node's own git for checkpoints and sub-agents runs, in words
/// for the start-up log and `doctor`.
fn checkpoint_confinement(c: &harness_tools::sandbox::Confinement) -> &'static str {
    use harness_tools::sandbox::Confinement;
    match c {
        Confinement::Confined { sandbox, .. } if sandbox.is_best_effort() => {
            "the node's git reads session folders in the sandbox, best-effort (on a kernel \
             without Landlock it is not confined)"
        }
        Confinement::Confined { .. } => {
            "the node's git reads session folders in the sandbox (read-only on the folder, \
             write on its checkpoint store only)"
        }
        Confinement::Unconfined => {
            "NOT confined ([sandbox] mode = \"off\"): the node's git reads session folders \
             with every right of this OS user"
        }
        Confinement::Refused(_) => "no agentic sessions here, so no checkpoints",
    }
}

/// The one line on the execution modes, for the log and `doctor`: which
/// modes this node offers, or the refusal of a node that offers none.
fn exec_line(o: &harness_node::exec::ExecOffers) -> Result<String, String> {
    use harness_core::policy::ExecMode;
    if let Some(why) = o.none_offered() {
        return Err(format!("NONE: no agentic session runs here: {why}"));
    }
    let mut parts: Vec<String> = Vec::new();
    for m in ExecMode::ALL {
        match o.why_not(m) {
            None if m == ExecMode::Host => parts.push(
                "host (OFFERED: its commands run as this OS user without bubblewrap; README, \
                 \"Execution modes\")"
                    .into(),
            ),
            None => parts.push(m.id().into()),
            Some(why) => parts.push(format!("not {} ({why})", m.id())),
        }
    }
    Ok(parts.join("; "))
}

/// Set up the shell sandbox for an agentic runtime and record the modes
/// it lets the node offer in its policy: the `sandbox` mode's confinement
/// and the `host` mode's (and the helpers'). `node run` refuses to start
/// when the node offers no mode; `serve` starts and its node refuses
/// agentic sessions, saying why.
fn start_sandbox(
    dir: &Path,
    policy: &mut NodePolicy,
    refuse_to_start: bool,
) -> anyhow::Result<(
    harness_tools::sandbox::Confinement,
    harness_tools::sandbox::Confinement,
)> {
    let node_toml = dir.join("node.toml");
    let settings = policy
        .sandbox_settings()
        .map_err(|e| anyhow::anyhow!("{}: {e}", node_toml.display()))?;
    // A node in the network-off gap logs, once, that it left it (T8B-1).
    harness_tools::sandbox::set_network_log(|l| tracing::info!("{l}"));
    let s = harness_tools::sandbox::start(
        &settings,
        dir,
        &policy.protected_dirs,
        &policy.workspace_roots,
        &node_toml,
    )
    .map_err(anyhow::Error::msg)?;
    let offers = harness_node::exec::ExecOffers::from_start(&s, policy);
    match (&s.refusal, settings.mode) {
        (Some(why), _) if refuse_to_start => bail!("{why}"),
        (Some(_), _) => tracing::error!("shell sandbox: {}", s.summary),
        (None, harness_tools::sandbox::Mode::Required) => {
            tracing::info!("shell sandbox: {}", s.summary)
        }
        (None, _) => tracing::warn!("shell sandbox: {}", s.summary),
    }
    match s.contain_level {
        harness_tools::sandbox::ContainLevel::Ok => {
            tracing::info!("shell containment: {}", s.contain_summary)
        }
        harness_tools::sandbox::ContainLevel::Warn => {
            tracing::warn!("shell containment: {}", s.contain_summary)
        }
        harness_tools::sandbox::ContainLevel::Refused if offers.offered().is_empty() => {
            tracing::error!("shell containment: {}", s.contain_summary)
        }
        harness_tools::sandbox::ContainLevel::Refused => {
            tracing::warn!("shell containment: {}", s.contain_summary)
        }
    }
    if !s.view_summary.is_empty() {
        tracing::info!("file view: {}", s.view_summary);
    }
    match exec_line(&offers) {
        Err(why) if refuse_to_start => bail!("{why}"),
        Err(why) => tracing::error!("execution modes: {why}"),
        Ok(line) if offers.offers(harness_core::policy::ExecMode::Host) => {
            tracing::warn!("execution modes: {line}")
        }
        Ok(line) => tracing::info!("execution modes: {line}"),
    }
    if let Some(root) = &s.tmp_root {
        // Before it serves: what a dead node left that its guard could not
        // end (a guard killed in the moment before bubblewrap's init set
        // its death signal; T.8e).
        // The session comes from a process's own arguments: escaped.
        for w in harness_tools::contain::sweep(root) {
            use harness_tools::contain::Left;
            match (w.what, w.ended) {
                (Left::Namespace, true) => tracing::warn!(
                    "shell containment: ended a command a previous run of this node left \
                     running (pid {}, session {:?})",
                    w.pid,
                    w.session
                ),
                (Left::Namespace, false) => tracing::error!(
                    "shell containment: a command a previous run of this node left running is \
                     STILL ALIVE (pid {}, session {:?}): killed, it will end when the kernel \
                     releases it (it waits on I/O)",
                    w.pid,
                    w.session
                ),
                (Left::Guard, true) => tracing::warn!(
                    "shell containment: told the guard of a dead node to stop (pid {}, session \
                     {:?}); its command has ended",
                    w.pid,
                    w.session
                ),
                (Left::Guard, false) => tracing::error!(
                    "shell containment: told the guard of a dead node to stop (pid {}, session \
                     {:?}); it is STILL ending its command 2 s later (the kernel cannot end it \
                     yet)",
                    w.pid,
                    w.session
                ),
                (Left::GuardUnknown, _) => tracing::warn!(
                    "shell containment: left alone a guard of this data directory that names no \
                     node (pid {}, session {:?})",
                    w.pid,
                    w.session
                ),
            }
        }
        harness_tools::sandbox::reset_tmp(root).map_err(anyhow::Error::msg)?;
    }
    policy.exec = offers;
    policy.sandbox_tmp = s.tmp_root;
    Ok((s.confinement, s.uncontained))
}

async fn node_join(dir: &Path, url: &str, token: &str, fingerprint: &str) -> anyhow::Result<()> {
    harness_master::secrets::check_keys(dir)?;
    let key = harness_link::NodeKey::load_or_create(&dir.join("keys/node.key"))?;
    let node_id = harness_link::client::join(url, fingerprint, token, &hostname(), &key).await?;
    let remote = RemoteToml {
        url: url.into(),
        fingerprint: fingerprint.into(),
        node_id: node_id.clone(),
    };
    std::fs::write(dir.join("remote.toml"), toml::to_string(&remote)?)?;
    println!("joined as node `{node_id}`. Start it with: {BRAND} node run");
    println!(
        "this node's own policy is {} (it decides what the master may do here)",
        dir.join("node.toml").display()
    );
    Ok(())
}

async fn node_run(dir: &Path, dotenv_names: &[String]) -> anyhow::Result<()> {
    let remote: RemoteToml = toml::from_str(
        &std::fs::read_to_string(dir.join("remote.toml"))
            .context("not joined yet: run `node join` first")?,
    )?;
    harness_master::secrets::check_private(&dir.join("secrets.toml"))?;
    harness_master::secrets::check_keys(dir)?;
    let key = harness_link::NodeKey::load_or_create(&dir.join("keys/node.key"))?;
    let cfg = Config::load(&dir.join(CONFIG_FILE))?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let secrets = harness_core::SecretSet::default();
    let (runtime, provider_env) = ProviderRuntime::start(cfg.clone(), dir, secrets.clone())?;
    let child_env = tool_env(&policy, provider_env, dotenv_names)?;
    let provider: Arc<dyn Provider> = runtime.provider.clone();
    let (node, _graphify) = agentic_node(
        dir,
        &cfg,
        policy,
        provider,
        child_env,
        &secrets,
        &remote.node_id,
        true,
    )
    .await?;
    let mut signals = stop::Signals::install().context("catching stop signals")?;
    let stop = tokio_util::sync::CancellationToken::new();
    println!("node `{}` connecting to {}", remote.node_id, remote.url);
    let attach = node.clone();
    let mut link = tokio::spawn(harness_link::client::run(
        harness_link::client::Remote {
            url: remote.url,
            fingerprint: remote.fingerprint,
            node_id: remote.node_id,
        },
        key,
        stop.clone(),
        move || {
            let node = attach.clone();
            async move {
                let l = node.attach().await;
                (l.to_node, l.from_node)
            }
        },
    ));
    tokio::select! {
        name = signals.first() => {
            // The link stays up while the turns close, so the master sees
            // them end (and the refusals of new work); a master that is
            // away gets them replayed at the next hello.
            stop::announce(name, "the node");
            signals.force_on_second();
            let left = node.stop(stop::stop_wait()).await;
            node.flush(stop::FLUSH_WAIT).await;
            // The last message taken from the queue may still be on its way
            // out: a moment before the link closes (a lost one replays).
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            stop.cancel();
            let _ = link.await;
            stopped(left.into_iter().map(|s| s.to_string()).collect())
        }
        // The link ends by itself only on a refusal (a revoked or unknown
        // key, another protocol version): exit with the master's reason.
        r = &mut link => {
            r.context("the link to the master")??;
            Ok(())
        }
    }
}

/// `<data>/api-token`: the scripts' bearer token.
fn api_token_path(dir: &Path) -> PathBuf {
    dir.join("api-token")
}

fn doctor(dir: &Path) -> anyhow::Result<()> {
    use harness_tools::sandbox::ContainLevel;
    let mut ok = true;
    let mut check = |name: &str, res: Result<String, String>| match res {
        Ok(m) => println!("ok    {name}: {m}"),
        Err(m) => {
            ok = false;
            println!("FAIL  {name}: {m}")
        }
    };
    check("data dir", Ok(dir.display().to_string()));
    match lock::status(dir) {
        Ok(lock::Status::Unsupported(m)) => println!("warn  data dir lock: {m}"),
        Ok(lock::Status::Free(m) | lock::Status::Held(m)) => check("data dir lock", Ok(m)),
        Err(m) => check("data dir lock", Err(m)),
    }
    for bin in ["bash", "git"] {
        let found = std::process::Command::new(bin)
            .arg("--version")
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .map_err(|e| e.to_string());
        check(bin, found);
    }
    check(
        CONFIG_FILE,
        Config::load(&dir.join(CONFIG_FILE))
            .map(|c| {
                let ids: Vec<&str> = c.providers.iter().map(|p| p.id.as_str()).collect();
                format!(
                    "providers {}, default model {}",
                    ids.join(", "),
                    c.effective_default().unwrap_or_default()
                )
            })
            .map_err(|e| format!("{e:#}")),
    );
    check(
        "node.toml",
        NodePolicy::load(&dir.join("node.toml"))
            .map(|p| {
                format!(
                    "max approval mode {:?}, {} workspace root(s); tool environment: {}, LC_*{}{}; \
                     streamed output per call {} bytes; {} steps per turn; bash timeout ceiling {} s",
                    p.max_approval_mode,
                    p.workspace_roots.len(),
                    harness_tools::env::CORE_VARS.join(" "),
                    if p.env_pass.is_empty() {
                        ""
                    } else {
                        " + env_pass "
                    },
                    p.env_pass.join(" "),
                    p.limits.tool_stream_bytes,
                    p.limits.max_steps,
                    p.limits.bash_timeout_max_s
                )
            })
            .map_err(|e| e.to_string()),
    );
    // The sandbox's start, once, as the node makes it (its own data
    // directory protected), and the modes it lets the node offer (N.10a).
    let started = NodePolicy::load(&dir.join("node.toml"))
        .map_err(|e| e.to_string())
        .and_then(|mut p| {
            p.protect(dir);
            harness_tools::sandbox::start(
                &p.sandbox_settings()?,
                dir,
                &p.protected_dirs,
                &p.workspace_roots,
                &dir.join("node.toml"),
            )
            .map(|s| {
                let offers = harness_node::exec::ExecOffers::from_start(&s, &p);
                (s, offers)
            })
        });
    check(
        "shell sandbox",
        started
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(s, _)| match &s.refusal {
                Some(_) => Err(s.summary.clone()),
                None => Ok(s.summary.clone()),
            }),
    );
    // Three marks: a containment with a named gap is a warning, never "ok"
    // (review N-5); the sandbox mode not offered is a warning where the node
    // offers `host`, a failure where it then offers nothing.
    match &started {
        Ok((s, _)) if s.contain_level == ContainLevel::Warn => {
            println!("warn  shell containment: {}", s.contain_summary)
        }
        Ok((s, o)) if s.contain_level == ContainLevel::Refused && !o.offered().is_empty() => {
            println!("warn  shell containment: {}", s.contain_summary)
        }
        Ok((s, _)) if s.contain_level == ContainLevel::Refused => {
            check("shell containment", Err(s.contain_summary.clone()))
        }
        Ok((s, _)) => check("shell containment", Ok(s.contain_summary.clone())),
        Err(e) => check("shell containment", Err(e.clone())),
    }
    // What a contained command can see (T.8c).
    if let Ok((s, _)) = &started
        && !s.view_summary.is_empty()
    {
        check("file view", Ok(s.view_summary.clone()));
    }
    match &started {
        Ok((_, o)) => match exec_line(o) {
            Ok(line) if o.offers(harness_core::policy::ExecMode::Host) => {
                println!("warn  execution modes: {line}")
            }
            line => check("execution modes", line),
        },
        Err(e) => check("execution modes", Err(e.clone())),
    }
    check(
        "checkpoint git",
        started
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(s, _)| match &s.uncontained {
                harness_tools::sandbox::Confinement::Confined { .. } => {
                    Ok(checkpoint_confinement(&s.uncontained).to_string())
                }
                c => Err(checkpoint_confinement(c).to_string()),
            }),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = api_token_path(dir);
        check(
            "api-token permissions",
            match std::fs::metadata(&p) {
                Ok(m) if m.permissions().mode() & 0o077 == 0 => Ok("0600".into()),
                Ok(m) => Err(format!(
                    "{:o}; run chmod 600",
                    m.permissions().mode() & 0o777
                )),
                Err(_) => Ok("not created yet".into()),
            },
        );
    }
    if !ok {
        bail!("doctor found problems");
    }
    Ok(())
}

/// `.env` from `HARNESS_ENV_FILE`, else (for `serve` only) the working
/// directory: a node or CLI command run inside some project must not pick
/// up that project's `.env`. Loaded before the runtime starts any thread
/// (the environment is process-wide); variables already set win over the
/// file. Returns the names it set.
fn load_dotenv() -> anyhow::Result<Vec<String>> {
    let explicit = std::env::var_os("HARNESS_ENV_FILE").is_some();
    let serving = std::env::args().skip(1).any(|a| a == "serve");
    if !explicit && !serving {
        return Ok(Vec::new());
    }
    let path = match std::env::var_os("HARNESS_ENV_FILE") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(".env"),
    };
    // It holds provider keys: refused when others can read it (AUDIT L7).
    harness_master::secrets::check_private(&path)?;
    let iter = match dotenvy::from_path_iter(&path) {
        Ok(i) => i,
        Err(e) if e.not_found() && !explicit => return Ok(Vec::new()),
        Err(e) => return Err(anyhow::anyhow!("reading {}: {e}", path.display())),
    };
    let mut set = Vec::new();
    for item in iter {
        let (k, v) = item.with_context(|| format!("parsing {}", path.display()))?;
        if std::env::var_os(&k).is_none() {
            set.push((k, v));
        }
    }
    let names = set.iter().map(|(k, _)| k.clone()).collect();
    // dotenvy sets them itself (no unsafe here): only the ones not set yet.
    dotenvy::from_path(&path).with_context(|| format!("loading {}", path.display()))?;
    Ok(names)
}

fn main() -> anyhow::Result<()> {
    // The sandbox wrapper runs first: no `.env`, no data directory, no
    // threads (Landlock confines the calling thread, then `exec`).
    if std::env::args_os().nth(1).as_deref() == Some(harness_tools::sandbox::SUBCOMMAND.as_ref()) {
        // Before anything else: nothing the guard, bubblewrap or the node
        // left open (the image this runs from, review T8F-2) reaches the
        // command.
        harness_tools::sandbox::close_inherited();
    }
    if std::env::args_os().nth(1).as_deref() == Some(harness_tools::sandbox::SUBCOMMAND.as_ref())
        && let Cmd::SandboxExec {
            rw,
            ro,
            no_network,
            best_effort,
            status,
            probe_read,
            probe_unreadable,
            probe_absent,
            probe_pid_ns,
            probe_own_proc,
            probe_net_not,
            command,
            ..
        } = Cli::parse().cmd
    {
        let grants = |pairs: &[std::ffi::OsString]| {
            harness_tools::sandbox::Grant::parse_pairs(pairs).unwrap_or_else(|e| {
                eprintln!("{}: {e}", harness_tools::sandbox::SUBCOMMAND);
                std::process::exit(2)
            })
        };
        std::process::exit(harness_tools::sandbox::exec(
            harness_tools::sandbox::ExecRequest {
                rw: grants(&rw),
                ro: grants(&ro),
                no_network,
                best_effort,
                status,
                probe: harness_tools::sandbox::Probe {
                    read: probe_read.into_iter().map(Into::into).collect(),
                    unreadable: probe_unreadable.into_iter().map(Into::into).collect(),
                    pid_ns: probe_pid_ns,
                    own_proc: probe_own_proc,
                    net_not: probe_net_not,
                    absent: probe_absent.into_iter().map(Into::into).collect(),
                },
                command,
            },
        ));
    }
    // The guard of a contained command (T.8e): one thread, no `.env`, no
    // runtime; bubblewrap's death signal names its only thread.
    if std::env::args_os().nth(1).as_deref() == Some(harness_tools::guard::SUBCOMMAND.as_ref()) {
        std::process::exit(harness_tools::guard::main());
    }
    // The PDF helper too: one thread, no `.env`, no runtime (its memory
    // limit covers every mapping, thread stacks included).
    if std::env::args_os().nth(1).as_deref() == Some(harness_master::pdf::SUBCOMMAND.as_ref()) {
        std::process::exit(harness_master::pdf::helper_main());
    }
    let loaded = load_dotenv()?;
    init_tracing();
    let cli = Cli::parse();
    let dir = data_dir(&cli.data_dir)?;
    // One process per data directory (A.20), before anything else touches
    // it. Held here, outside the runtime, and never dropped: the process
    // exits with it held and the kernel releases it only once the process
    // is gone (review A19-1). `doctor`, `token`, `claim-code` and
    // `graphify` run beside a live server or node and take none.
    let role = match &cli.cmd {
        Cmd::Serve { .. } => Some("serve"),
        Cmd::Node { cmd: NodeCmd::Run } => Some("node run"),
        Cmd::Node {
            cmd: NodeCmd::Join { .. },
        } => Some("node join"),
        _ => None,
    };
    let _lock = role.map(|r| lock::take(&dir, r)).transpose()?;
    if role.is_some() {
        check_schemas(&dir)?;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    // A panic too ends in the bounded shutdown below, never in a plain drop
    // of the runtime (which would wait for a stuck thread without end).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt.block_on(async_main(cli, dir, loaded))
    }))
    .unwrap_or_else(|_| Err(anyhow::anyhow!("the harness panicked (see above)")));
    // A blocking thread a cancelled tool left (a read on a mount that does
    // not answer) would hold a plain drop of the runtime for ever.
    rt.shutdown_timeout(stop::RUNTIME_SHUTDOWN);
    let code = match &result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e:?}");
            if e.chain().any(|c| {
                matches!(
                    c.downcast_ref::<harness_link::LinkError>(),
                    Some(
                        harness_link::LinkError::Rejected(_)
                            | harness_link::LinkError::Untrusted(_)
                    )
                )
            }) {
                stop::EXIT_REFUSED
            } else {
                1
            }
        }
    };
    // With the lock still held: it goes with the process, not before.
    std::process::exit(code)
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HARNESS_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        // No colour codes in `docker logs`, journald or files.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
}

async fn async_main(cli: Cli, dir: PathBuf, dotenv_names: Vec<String>) -> anyhow::Result<()> {
    match cli.cmd {
        Cmd::Serve {
            bind,
            insecure_bind,
            tls,
            allowed_hosts,
            node_listen,
            no_local_node,
        } => {
            serve(
                &dir,
                bind,
                insecure_bind,
                tls,
                allowed_hosts,
                node_listen,
                no_local_node,
                &dotenv_names,
            )
            .await
        }
        Cmd::Graphify { cmd } => {
            let policy = NodePolicy::load(&dir.join("node.toml"))?;
            if matches!(cmd, GraphifyCmd::Install) {
                match harness_tools::graphify::install().await {
                    Ok(log) => println!("{log}"),
                    Err(log) => bail!("install failed:\n{log}"),
                }
            }
            println!(
                "graphify: {} (pinned {}); allowed by {}: {}",
                harness_tools::graphify::installed_version()
                    .await
                    .unwrap_or_else(|| "not installed".into()),
                harness_tools::graphify::VERSION,
                dir.join("node.toml").display(),
                if policy.graphify {
                    "yes"
                } else {
                    "no (add `graphify = true` and restart)"
                }
            );
            Ok(())
        }
        Cmd::Token => {
            let t = auth::load_or_create_token(&api_token_path(&dir))?;
            println!("{}", t.expose());
            Ok(())
        }
        Cmd::ClaimCode => match std::fs::read_to_string(dir.join("claim-code")) {
            Ok(code) => {
                println!("{}", code.trim());
                Ok(())
            }
            Err(_) => bail!(
                "no pending claim code: this install is already claimed (or `serve` has not run yet)"
            ),
        },
        Cmd::Doctor => doctor(&dir),
        Cmd::SandboxExec { .. } => bail!(
            "{} must be the first argument",
            harness_tools::sandbox::SUBCOMMAND
        ),
        Cmd::PdfText => bail!(
            "{} must be the first argument",
            harness_master::pdf::SUBCOMMAND
        ),
        Cmd::Node {
            cmd:
                NodeCmd::Join {
                    master_url,
                    token,
                    fingerprint,
                },
        } => node_join(&dir, &master_url, &token, &fingerprint).await,
        Cmd::Node { cmd: NodeCmd::Run } => node_run(&dir, &dotenv_names).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A coding chat can propose project facts: agentic nodes register
    /// `remember` (it runs nothing on the node; the master files the
    /// mirrored call as a proposal the person confirms).
    #[test]
    fn agentic_nodes_register_remember() {
        let graphify = harness_tools::graphify::GraphifyState::new(false, PathBuf::from("unused"));
        let names: Vec<&str> = agentic_tools(&graphify).iter().map(|t| t.name()).collect();
        assert!(names.contains(&"remember"), "{names:?}");
        assert!(names.contains(&"graph_query"), "{names:?}");
    }
}
