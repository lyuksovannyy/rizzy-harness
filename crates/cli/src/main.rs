//! The single `harness` binary.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod listen;
mod lock;
mod net;
mod push_check;
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
    /// Read a Word, Excel, PowerPoint or OpenDocument file on stdin, write
    /// its type and text on stdout, confined like `pdf-text`. The server
    /// starts it for such attachments.
    #[command(hide = true)]
    DocText,
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
            if tls { "" } else { " and WITHOUT TLS: passwords, cookies and pasted forge tokens travel in clear text" }
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
    // The web tools of `[web]`, built once: the master's own chat runtime
    // and the embedded node register the same tools (one HTTP client), and
    // the search key is in both scrubbers.
    let (web_tools, web_key) = harness_master::providers::web_tools(&cfg, dir)?;
    let node = if no_local_node {
        tracing::info!("no local node: agentic sessions run on remote nodes");
        None
    } else {
        let node_web = NodeWeb::embedded(web_tools.clone(), web_key.clone(), &cfg.web, &policy);
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
                node_web,
            )
            .await?,
        )
    };

    // The master's own chat runtime: same session code, no workspace and no
    // file or shell tools; only the web tools from `[web]`.
    let mut web_tools = web_tools;
    let (memory, builtin_memory) = harness_master::providers::memory(&cfg, dir, &secrets)?;
    if memory.is_some() {
        web_tools.push(Arc::new(harness_memory::Remember));
    }
    // Searching earlier chats needs no memory store (PLAN.md T55).
    web_tools.push(Arc::new(harness_memory::Recall));
    // Reading the chat's own log: node code, no master round trip (T91).
    web_tools.push(Arc::new(harness_agent_native::history::History));
    // Files a person attached to a plain chat: the master keeps them and
    // answers the reads (PLAN.md T95). Only this runtime, which runs plain
    // chats: a coding chat saves its files in its folder.
    web_tools.push(Arc::new(harness_tools::AttachedFileTool));
    // The project board tools: the master answers them too (PLAN.md T78).
    web_tools.push(Arc::new(harness_tools::Board));
    web_tools.push(Arc::new(harness_tools::BoardEdit));
    // Skills stored on the server (X.34 step B, PLAN.md T81): text only;
    // this runtime reads no skill folder (a chat has none).
    web_tools.push(Arc::new(harness_tools::skills::SkillTool));
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
            // `[chat] model_first_byte_timeout_s` (X.31); the gap between
            // events keeps its default.
            stream_limits: harness_agent_native::StreamLimits {
                first_byte: cfg.chat.first_byte_timeout(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: hostname(),
        // As a node with no node.toml (the owner's home skill roots).
        policy: NodePolicy::node_toml_default(),
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
    harness_master::forge::open(&master, dir);
    master.set_provider_runtime(runtime);
    // PDF and office-document attachments are read by this binary's
    // `pdf-text` and `doc-text`, never in here.
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
    master.start_handoffs();

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

/// The web tools a node's agentic runtime registers, and where they come
/// from (ADOPTION_PLAN T.3, PLAN.md T89): the server's `[web]` for the
/// embedded node, node.toml's own `[web]` for a remote one. Never anything
/// the master sends.
struct NodeWeb {
    tools: harness_tools::ToolSet,
    /// The search key, for the node's scrubber.
    key: Option<harness_core::Secret<String>>,
    /// Where what is on was decided, in words for the log and `doctor`.
    on_from: &'static str,
    /// The search engine, for the line.
    search: harness_core::policy::SearchKind,
    /// Each tool that is off here and the file that decided it.
    off: Vec<(&'static str, String)>,
}

impl NodeWeb {
    /// A remote node's: node.toml's `[web]` is the only source; no table, or
    /// a table that leaves a tool out, means off.
    fn of_node_toml(policy: &NodePolicy, dir: &Path) -> anyhow::Result<Self> {
        let w = policy.web.clone();
        let config = harness_master::config::WebConfig::from(&w.clone().unwrap_or_default());
        let (tools, key) = harness_master::providers::web_tools_from(&config, dir)
            .map_err(|e| anyhow::anyhow!("node.toml [web]: {e}"))?;
        let why = |what: &str| match &w {
            None => "no [web] in node.toml".to_string(),
            Some(_) => format!("node.toml [web] {what}"),
        };
        let mut off = Vec::new();
        if config.search == harness_core::policy::SearchKind::None {
            off.push(("web_search", why("search = \"none\"")));
        }
        if !config.fetch {
            off.push(("web_fetch", why("fetch = false")));
        }
        Ok(Self {
            tools,
            key,
            on_from: "[web] in node.toml",
            search: config.search,
            off,
        })
    }

    /// The server's own node: config.toml's `[web]` decides what exists; a
    /// `[web]` table in node.toml can only take away (`fetch = false`,
    /// `search = "none"`; a key it leaves out counts as off), never switch
    /// on what config.toml has off, and its backend settings are ignored here.
    fn embedded(
        tools: harness_tools::ToolSet,
        key: Option<harness_core::Secret<String>>,
        config: &harness_master::config::WebConfig,
        policy: &NodePolicy,
    ) -> Self {
        use harness_core::policy::SearchKind;
        let nt = policy.web.as_ref();
        let mut off: Vec<(&'static str, String)> = Vec::new();
        let mut keep = Vec::new();
        for t in tools {
            match t.name() {
                "web_fetch" if nt.is_some_and(|w| !w.fetch) => {
                    off.push(("web_fetch", "node.toml [web] fetch = false".into()));
                    continue;
                }
                "web_search" if nt.is_some_and(|w| w.search == SearchKind::None) => {
                    off.push(("web_search", "node.toml [web] search = \"none\"".into()));
                    continue;
                }
                _ => keep.push(t),
            }
        }
        if config.search == SearchKind::None {
            off.push(("web_search", "config.toml [web] search = \"none\"".into()));
        }
        if !config.fetch {
            off.push(("web_fetch", "config.toml [web] fetch = false".into()));
        }
        if let Some(w) = nt
            && (!w.searxng_url.is_empty()
                || w.brave_key_env.is_some()
                || w.brave_key_secret.is_some())
        {
            tracing::info!(
                "node.toml [web]: its search backend settings are ignored on this server's own \
                 node (config.toml's are used); only search = \"none\" and fetch = false apply"
            );
        }
        Self {
            tools: keep,
            key,
            on_from: "[web] in config.toml",
            search: config.search,
            off,
        }
    }

    /// The search key is a secret of this node like a provider key: it
    /// reaches no event, no log line and no tool result.
    fn add_key_to(&self, secrets: &harness_core::SecretSet) {
        if let Some(k) = &self.key {
            secrets.add(k.clone());
        }
    }

    /// The one line for the log and `doctor`: each tool, on from which
    /// setting or off by which file, the engine, and `denied_tools`. No key.
    fn line(&self, policy: &NodePolicy) -> String {
        use harness_core::policy::SearchKind;
        let mut parts: Vec<String> = Vec::new();
        for name in ["web_search", "web_fetch"] {
            if self.tools.iter().any(|t| t.name() == name) {
                let mut p = match (name, self.search) {
                    ("web_search", SearchKind::Searxng) => "web_search (searxng)".to_string(),
                    ("web_search", SearchKind::Brave) => "web_search (brave)".to_string(),
                    _ => name.to_string(),
                };
                p.push_str(&format!(" from {}", self.on_from));
                if policy.denies(name) {
                    p.push_str(" [refused by node.toml denied_tools]");
                }
                parts.push(p);
            } else {
                let why = self
                    .off
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or("not set up", |(_, w)| w.as_str());
                parts.push(format!("{name} off ({why})"));
            }
        }
        parts.join("; ")
    }
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
    web: NodeWeb,
) -> anyhow::Result<(NodeRuntime, Arc<harness_tools::graphify::GraphifyState>)> {
    // No session may include this runtime's data directory (for the
    // embedded node, the master's: keys, the API token, every chat), and
    // no file tool may touch it, whatever node.toml allows (AUDIT S1).
    let mut policy = policy;
    policy.protect(dir);
    // The node operator's instruction file: node.toml's, else AGENTS.md
    // in this data directory; protected wherever it lies, refused where an
    // agent could reach it (X.2, PLAN.md T62).
    policy
        .locate_instructions(dir)
        .map_err(|e| anyhow::anyhow!("node.toml: {e}"))?;
    if let Some(f) = &policy.instructions.located {
        for w in harness_node::InstructionsPolicy::warnings(f) {
            tracing::warn!("the node's instruction file: {w}");
        }
    }
    // A repository's MCP servers (X.36, PLAN.md T82): the approved copies
    // live in this data directory (protected above), never in a folder a
    // chat can reach.
    policy.mcp.approvals_file = Some(dir.join("project-mcp.json"));
    match policy.mcp.project_servers {
        harness_node::policy::ProjectServers::Ask => tracing::info!(
            "a repository's MCP servers (.mcp.json): a person may approve them, one by one, for \
             their own sandboxed chats ([mcp] project_servers = \"ask\")"
        ),
        harness_node::policy::ProjectServers::Off => {
            tracing::info!("a repository's MCP servers (.mcp.json): off")
        }
    }
    // Where new projects' folders are made (X.26, PLAN.md T77): checked
    // once here, after the data directory is protected; one node.toml
    // names that breaks a rule stops the node.
    match policy
        .locate_projects(dir)
        .map_err(|e| anyhow::anyhow!("node.toml: {e}"))?
    {
        Some(why) => tracing::warn!("{why}"),
        None => match &policy.projects.located {
            Some(d) => tracing::info!("new project folders: made in {}", d.display()),
            None => tracing::info!("new project folders: off ([projects] dir = \"\")"),
        },
    }
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
    web.add_key_to(secrets);
    tracing::info!("web tools in agentic chats: {}", web.line(&policy));
    let backend = NativeBackend::new(
        provider,
        agentic_tools(&graphify, &web.tools),
        NativeConfig {
            tool_env,
            tool_stream_bytes: policy.limits.tool_stream_bytes,
            max_steps: policy.limits.max_steps,
            bash_timeout_max: std::time::Duration::from_secs(policy.limits.bash_timeout_max_s),
            stream_limits: harness_agent_native::StreamLimits {
                first_byte: std::time::Duration::from_secs(
                    policy.limits.model_first_byte_timeout_s,
                ),
                idle: std::time::Duration::from_secs(policy.limits.model_idle_timeout_s),
            },
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

/// The tools an agentic node registers: the built-in ones, the web tools
/// its own configuration enables, `graph_query`,
/// `recall`, `board`, `board_edit` and `remember` (it runs nothing here: the master files the mirrored call
/// as a proposal the person confirms; `denied_tools` turns it off). Each
/// turn offers those its agent allows; node policy still gates every call.
fn agentic_tools(
    graphify: &Arc<harness_tools::graphify::GraphifyState>,
    web: &harness_tools::ToolSet,
) -> harness_tools::ToolSet {
    let mut tools = harness_tools::builtin();
    // `web_search` and `web_fetch`, when this node's own configuration has
    // them (T.3): they run here, in this process, never on the master.
    tools.extend(web.iter().cloned());
    tools.push(Arc::new(harness_tools::graphify::GraphQuery::new(
        graphify.clone(),
    )));
    tools.push(Arc::new(harness_memory::Remember));
    // Runs nothing here either: the master answers it (PLAN.md T55).
    tools.push(Arc::new(harness_memory::Recall));
    // Reads the session's own log on this node (PLAN.md T91).
    tools.push(Arc::new(harness_agent_native::history::History));
    // And the project board tools (PLAN.md T78).
    tools.push(Arc::new(harness_tools::Board));
    tools.push(Arc::new(harness_tools::BoardEdit));
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
        Confinement::Unconfined { .. } => {
            "NOT confined ([sandbox] mode = \"off\"): the node's git reads session folders \
             with every right of this OS user"
        }
        Confinement::Refused(_) => "no agentic sessions here, so no checkpoints",
    }
}

/// The line for a node where a host chat's shell has no file rule at all:
/// `[sandbox] mode = "off"` with the host mode offered (T82, T39). Nothing
/// of a person, a path or a token goes in it.
fn host_data_dir_line(
    mode: harness_tools::sandbox::Mode,
    o: &harness_node::exec::ExecOffers,
) -> Option<&'static str> {
    (mode == harness_tools::sandbox::Mode::Off && o.offers(harness_core::policy::ExecMode::Host))
        .then_some(
            "[sandbox] mode = \"off\" with [execution] host = true: a host chat on this node \
             can read and change the node's data directory (other people's chats, the approved \
             MCP servers, the node's keys and token)",
        )
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
    // The node user's own git configuration, read by every chat (T88).
    if let Some(f) = harness_tools::sandbox::shared_git_config(&settings) {
        tracing::warn!(
            "[sandbox] read/write makes {} readable to the agents' commands: every chat on \
             this node, whoever owns it, then runs git with that configuration (its name and \
             e-mail win over the chat owner's, and its credential helpers, insteadOf rules and \
             core.sshCommand apply)",
            f.display()
        );
    }
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
    if let Some(w) = host_data_dir_line(settings.mode, &offers) {
        tracing::warn!("host chats: {w}");
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
    // This node's own `[web]` (node.toml), off unless it says otherwise;
    // nothing the master sends turns it on.
    let web = NodeWeb::of_node_toml(&policy, dir)?;
    let (node, _graphify) = agentic_node(
        dir,
        &cfg,
        policy,
        provider,
        child_env,
        &secrets,
        &remote.node_id,
        true,
        web,
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

async fn doctor(dir: &Path) -> anyhow::Result<()> {
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
    // The web tools this node's agentic chats have, and from which setting
    // (ADOPTION_PLAN T.3, PLAN.md T89): a directory that has joined a master
    // is a remote node and reads node.toml's `[web]`; any other is a server,
    // whose embedded node takes config.toml's. A missing key is a failure
    // naming the variable or secret, never its value.
    check(
        "web tools (agentic chats)",
        NodePolicy::load(&dir.join("node.toml"))
            .map_err(|e| e.to_string())
            .and_then(|p| {
                let web = if dir.join("remote.toml").exists() {
                    NodeWeb::of_node_toml(&p, dir).map_err(|e| format!("{e:#}"))?
                } else {
                    let cfg = Config::load(&dir.join(CONFIG_FILE)).map_err(|e| format!("{e:#}"))?;
                    let (tools, key) = harness_master::providers::web_tools(&cfg, dir)
                        .map_err(|e| format!("{e:#}"))?;
                    NodeWeb::embedded(tools, key, &cfg.web, &p)
                };
                Ok(web.line(&p))
            }),
    );
    // The sandbox's start, once, as the node makes it (its own data
    // directory protected), and the modes it lets the node offer (N.10a).
    // The node operator's instruction file (X.2, PLAN.md T62).
    check(
        "instruction file",
        NodePolicy::load(&dir.join("node.toml"))
            .map_err(|e| e.to_string())
            .and_then(|mut p| {
                p.protect(dir);
                p.locate_instructions(dir)?;
                let f = p.instructions.located.unwrap_or_default();
                // A foreign owner is refused (the file is not read); the
                // rest are warnings.
                for w in harness_node::InstructionsPolicy::warnings(&f) {
                    if w.contains("it is not read") {
                        return Err(w);
                    }
                    println!("warn  instruction file: {w}");
                }
                Ok(if f.exists() {
                    format!("{} (sent with every agentic chat)", f.display())
                } else {
                    format!("{} (none there; nothing is sent)", f.display())
                })
            }),
    );
    let started = NodePolicy::load(&dir.join("node.toml"))
        .map_err(|e| e.to_string())
        .and_then(|mut p| {
            p.protect(dir);
            p.locate_instructions(dir)?;
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
    if let (Ok((_, o)), Ok(p)) = (&started, NodePolicy::load(&dir.join("node.toml")))
        && let Ok(settings) = p.sandbox_settings()
        && let Some(w) = host_data_dir_line(settings.mode, o)
    {
        println!("warn  host chats: {w}");
    }
    // Whether a host chat can push (X.40): from node.toml and this
    // process's environment alone, nothing contacted or read.
    if let (Ok((_, o)), Ok(p)) = (&started, NodePolicy::load(&dir.join("node.toml")))
        && let Ok(settings) = p.sandbox_settings()
    {
        let mut deny = harness_master::env::hidden_from_tools(&[]);
        if let Ok(c) = Config::load(&dir.join(CONFIG_FILE)) {
            deny.extend(c.providers.iter().filter_map(|x| x.api_key_env.clone()));
        }
        if let Ok(env) = p.tool_env(deny) {
            let home = dirs::home_dir();
            let get = |n: &str| std::env::var_os(n);
            let view = push_check::PushView {
                host_offered: o.offers(harness_core::policy::ExecMode::Host),
                env: &env,
                node_var: &get,
                read: &settings.read,
                write: &settings.write,
                home: home.as_deref(),
            };
            for l in push_check::lines(&view) {
                println!("info  push from a chat, {l}");
            }
        }
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
    // MCP servers (X.3): each started once where a chat would start it
    // (the sandbox mode where the node offers it, else the host mode), in
    // a scratch folder, listed and stopped.
    if let Ok(mut p) = NodePolicy::load(&dir.join("node.toml"))
        && !p.mcp_servers.is_empty()
    {
        use harness_core::policy::ExecMode;
        use harness_tools::sandbox::Confinement;
        p.protect(dir);
        let confinement = match &started {
            Ok((s, o)) if o.offers(ExecMode::Sandbox) => s.confinement.clone(),
            Ok((s, o)) if o.offers(ExecMode::Host) => s.uncontained.clone(),
            Ok(_) => Confinement::Refused("this node offers no execution mode".into()),
            Err(e) => Confinement::Refused(e.as_str().into()),
        };
        let scratch =
            std::env::temp_dir().join(format!("{BRAND}-doctor-mcp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&scratch);
        let ctx = harness_tools::ToolCtx {
            sandbox: confinement.for_session("doctor-mcp"),
            protected: Arc::new(p.protected_dirs.clone()),
            ..harness_tools::ToolCtx::new(&scratch)
        };
        for (name, server) in &p.mcp_servers {
            if !server.enabled {
                println!("ok    mcp {name}: disabled (enabled = false)");
                continue;
            }
            // A chat never starts it (T47): neither does doctor.
            if p.denies(&format!("{}{name}__", harness_tools::mcp::PREFIX)) {
                println!(
                    "ok    mcp {name}: not started, `denied_tools` in node.toml denies the whole \
                     server"
                );
                continue;
            }
            check(
                &format!("mcp {name}"),
                harness_tools::mcp::probe(name, server, &ctx).await,
            );
        }
        let _ = std::fs::remove_dir_all(&scratch);
        if let Ok((s, _)) = &started
            && let Some(t) = &s.tmp_root
        {
            let _ = std::fs::remove_dir_all(t.join("doctor-mcp"));
        }
    }
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
        // command. A repository MCP server's environment is read from its
        // descriptor first (X.36), to be set on the command alone.
        harness_tools::sandbox::take_inner_env();
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
    // And its twin for office documents (X.55).
    if std::env::args_os().nth(1).as_deref() == Some(harness_master::pdf::DOC_SUBCOMMAND.as_ref()) {
        std::process::exit(harness_master::doc::helper_main());
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
    use tracing_subscriber::util::SubscriberInitExt as _;
    let env = log_env(std::env::var("HARNESS_LOG").ok().as_deref());
    // No colour codes in `docker logs`, journald or files.
    let ansi = std::io::IsTerminal::is_terminal(&std::io::stderr());
    logger(env, std::io::stderr, ansi).init();
}

/// The `genai` crate's OpenAI streamer writes `EMPTY CHOICE CONTENT` at
/// WARN for every stream chunk that carries no text (a role-only first
/// chunk, a usage-only last one): about one line per model reply with nothing
/// for a person to do about it (the crate's own code calls it a TODO for
/// debug). Its other line in that module is an error, which stays.
const GENAI_EMPTY_CHUNK: &str = "genai::adapter::adapters::openai::streamer";

/// The levels the log runs at: `HARNESS_LOG` (an `EnvFilter` spec; `info`
/// when unset or unreadable), and that one target down to errors unless the
/// spec names `genai` itself (`genai=warn`, or the target at `debug`), so a
/// person who asks for that crate's lines gets them.
fn log_env(spec: Option<&str>) -> tracing_subscriber::EnvFilter {
    use tracing_subscriber::EnvFilter;
    let spec = spec.unwrap_or("info");
    let (env, named) = match EnvFilter::try_new(spec) {
        Ok(e) => (e, spec.contains("genai")),
        Err(_) => (EnvFilter::new("info"), false),
    };
    // Logging off stays off: the directive below would print errors.
    if named || env.max_level_hint() == Some(tracing::level_filters::LevelFilter::OFF) {
        return env;
    }
    match format!("{GENAI_EMPTY_CHUNK}=error").parse() {
        Ok(d) => env.add_directive(d),
        Err(_) => env,
    }
}

/// The server's log: `env` decides the levels, [`quiet`] drops what no
/// level may write.
fn logger<W>(
    env: tracing_subscriber::EnvFilter,
    writer: W,
    ansi: bool,
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_ansi(ansi)
            .with_filter(env)
            .with_filter(quiet()),
    )
}

/// Whatever HARNESS_LOG says, rmcp's HTTP client transport (custom
/// connectors, PLAN.md T84) writes nothing: its lines can carry a remote
/// server's words. Only that target; the node's MCP client logs as ever.
fn quiet() -> tracing_subscriber::filter::FilterFn<fn(&tracing::Metadata<'_>) -> bool> {
    tracing_subscriber::filter::filter_fn(|m| harness_master::connectors::log_allowed(m.target()))
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
        Cmd::Doctor => doctor(&dir).await,
        Cmd::SandboxExec { .. } => bail!(
            "{} must be the first argument",
            harness_tools::sandbox::SUBCOMMAND
        ),
        Cmd::PdfText => bail!(
            "{} must be the first argument",
            harness_master::pdf::SUBCOMMAND
        ),
        Cmd::DocText => bail!(
            "{} must be the first argument",
            harness_master::pdf::DOC_SUBCOMMAND
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

    /// The server's log drops rmcp's HTTP client transport, even at TRACE,
    /// and keeps rmcp's other lines (the node's MCP client) and ours.
    #[test]
    fn the_log_drops_the_connector_transport_only() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let b2 = buf.clone();
        let sub = logger(
            tracing_subscriber::EnvFilter::new("trace"),
            move || b2.clone(),
            false,
        );
        tracing::subscriber::with_default(sub, || {
            tracing::error!(target: "rmcp::transport::streamable_http_client", "remote CANARY-1");
            tracing::info!(target: "rmcp::transport::streamable_http_client::x", "CANARY-2");
            tracing::info!(target: "rmcp::service", "node line");
            tracing::info!(target: "harness_master::connectors", "our line");
        });
        let out = String::from_utf8_lossy(&buf.0.lock().unwrap()).to_string();
        assert!(!out.contains("CANARY"), "{out}");
        assert!(
            out.contains("node line") && out.contains("our line"),
            "{out}"
        );
    }

    /// The `genai` streamer's per-reply `EMPTY CHOICE CONTENT` warning no
    /// longer prints at the default level; the same target's errors, the
    /// crate's other warnings and a person's own `genai` setting still do.
    #[test]
    fn the_empty_choice_warning_is_quiet_by_default_only() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let run = |spec: Option<&str>| {
            let buf = Buf::default();
            let b2 = buf.clone();
            let sub = logger(log_env(spec), move || b2.clone(), false);
            tracing::subscriber::with_default(sub, || {
                tracing::warn!(target: "genai::adapter::adapters::openai::streamer", "EMPTY CHOICE CONTENT");
                tracing::error!(target: "genai::adapter::adapters::openai::streamer", "stream ERROR-ONE");
                tracing::warn!(target: "genai::webc::web_stream", "other WARN-TWO");
                tracing::warn!(target: "harness_node::session", "ours WARN-THREE");
            });
            String::from_utf8_lossy(&buf.0.lock().unwrap()).to_string()
        };
        for spec in [None, Some("info"), Some("warn"), Some("debug")] {
            let out = run(spec);
            assert!(!out.contains("EMPTY CHOICE"), "{spec:?}: {out}");
            assert!(
                out.contains("ERROR-ONE") && out.contains("WARN-TWO") && out.contains("WARN-THREE"),
                "{spec:?}: {out}"
            );
        }
        // Turned off, it stays off: no line of any kind, the target's errors too.
        for spec in ["off", "OFF", "off,off"] {
            assert_eq!(run(Some(spec)), "", "{spec}");
        }
        for spec in [
            "info,genai::adapter::adapters::openai::streamer=debug",
            "warn,genai::adapter::adapters::openai::streamer=warn",
            "info,genai=warn",
        ] {
            assert!(run(Some(spec)).contains("EMPTY CHOICE"), "{spec}");
        }
    }

    /// A coding chat can propose project facts: agentic nodes register
    /// `remember` (it runs nothing on the node; the master files the
    /// mirrored call as a proposal the person confirms).
    #[test]
    fn agentic_nodes_register_remember() {
        let graphify = harness_tools::graphify::GraphifyState::new(false, PathBuf::from("unused"));
        let tools = agentic_tools(&graphify, &Vec::new());
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"remember"), "{names:?}");
        assert!(names.contains(&"graph_query"), "{names:?}");
        // And `recall`, which the master answers (PLAN.md T55).
        assert!(names.contains(&"recall"), "{names:?}");
        // And `history`, which the node answers from its own store (T91).
        assert!(names.contains(&"history"), "{names:?}");
        // And the board tools, which the master answers too (PLAN.md T78).
        assert!(
            names.contains(&"board") && names.contains(&"board_edit"),
            "{names:?}"
        );
        // Not `attached_file`: a coding chat saves its files in its folder
        // (PLAN.md T95); only the chat runtime registers it.
        assert!(!names.contains(&"attached_file"), "{names:?}");
    }
    /// A node registers the web tools its own configuration built, and
    /// nothing else of the sort: the chat runtime's `[web]` default
    /// (`fetch` on, no search) and an engine in config.toml, then none.
    #[test]
    fn agentic_nodes_register_the_web_tools_they_were_given() {
        use harness_master::config::{SearchKind, WebConfig};
        let graphify = harness_tools::graphify::GraphifyState::new(false, PathBuf::from("unused"));
        let names = |w: &WebConfig| -> Vec<String> {
            let (web, _) =
                harness_master::providers::web_tools_from(w, Path::new("unused")).unwrap();
            agentic_tools(&graphify, &web)
                .iter()
                .map(|t| t.name().to_string())
                .filter(|n| n.starts_with("web_"))
                .collect()
        };
        assert_eq!(names(&WebConfig::default()), ["web_fetch"]);
        let both = WebConfig {
            search: SearchKind::Searxng,
            searxng_url: "http://127.0.0.1:8888".into(),
            ..Default::default()
        };
        assert_eq!(names(&both), ["web_search", "web_fetch"]);
        let off = WebConfig {
            fetch: false,
            ..Default::default()
        };
        assert!(names(&off).is_empty());
        assert!(
            agentic_tools(&graphify, &Vec::new())
                .iter()
                .all(|t| !t.name().starts_with("web_")),
            "a node given none registers none"
        );
    }

    /// A remote node has no web tools until its own node.toml says so, and
    /// the master has no say: `[web]` is read from that file alone.
    #[test]
    fn a_remote_nodes_web_tools_are_off_until_node_toml_turns_them_on() {
        let dir = tempfile::tempdir().unwrap();
        let load = |toml: &str| {
            let p = dir.path().join("node.toml");
            std::fs::write(&p, toml).unwrap();
            NodePolicy::load(&p)
        };
        let names = |p: &NodePolicy| -> Vec<String> {
            let w = NodeWeb::of_node_toml(p, dir.path()).unwrap();
            w.tools.iter().map(|t| t.name().to_string()).collect()
        };
        let p = load("").unwrap();
        assert!(names(&p).is_empty());
        let line = NodeWeb::of_node_toml(&p, dir.path()).unwrap().line(&p);
        assert!(
            line.contains("web_search off") && line.contains("web_fetch off"),
            "{line}"
        );
        assert!(line.contains("no [web] in node.toml"), "{line}");
        let p = load("[web]\nfetch = true\n").unwrap();
        assert_eq!(names(&p), ["web_fetch"]);
        let p =
            load("[web]\nsearch = \"searxng\"\nsearxng_url = \"http://127.0.0.1:1\"\n").unwrap();
        assert_eq!(names(&p), ["web_search"]);
        // `denied_tools` shows in the line (and the loop refuses the call).
        let p = load("denied_tools = [\"web_fetch\"]\n[web]\nfetch = true\n").unwrap();
        let line = NodeWeb::of_node_toml(&p, dir.path()).unwrap().line(&p);
        assert!(
            line.contains("web_fetch from [web] in node.toml [refused by node.toml denied_tools]"),
            "{line}"
        );
        // An engine without its address or key is refused when the file is
        // read; an unset key variable names the variable, not a value.
        let e = load("[web]\nsearch = \"searxng\"\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("needs searxng_url"), "{e}");
        let e = load("[web]\nsearch = \"brave\"\n").unwrap_err().to_string();
        assert!(e.contains("brave_key_env or brave_key_secret"), "{e}");
        let p = load("[web]\nsearch = \"brave\"\nbrave_key_env = \"HARNESS_NO_SUCH_KEY_VAR\"\n")
            .unwrap();
        let e = NodeWeb::of_node_toml(&p, dir.path())
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("HARNESS_NO_SUCH_KEY_VAR"), "{e}");
        // Unknown keys are refused, like everywhere in node.toml.
        assert!(load("[web]\nfetchh = true\n").is_err());
        // A key found in the environment is returned for the scrubber and
        // is in no line (any variable that is set will do: `PATH`).
        let p = load("[web]\nsearch = \"brave\"\nbrave_key_env = \"PATH\"\n").unwrap();
        let w = NodeWeb::of_node_toml(&p, dir.path()).unwrap();
        let path = std::env::var("PATH").unwrap();
        assert_eq!(
            w.key.as_ref().map(|k| k.expose().clone()),
            Some(path.clone())
        );
        let line = w.line(&p);
        assert!(
            line.contains("web_search (brave)") && !line.contains(&path),
            "{line}"
        );
        // And it is in the node's scrubber, which every event and tool
        // result passes through.
        let secrets = harness_core::SecretSet::default();
        assert_eq!(secrets.scrub(&path), path, "not yet a secret");
        w.add_key_to(&secrets);
        assert_eq!(secrets.scrub(&path), "[REDACTED]");
    }

    /// The server's own node: config.toml decides what exists, and a `[web]`
    /// table in node.toml can only take away. Four combinations per tool, and
    /// the line says which file decided.
    #[test]
    fn node_toml_can_only_take_web_tools_away_from_the_servers_own_node() {
        use harness_master::config::{SearchKind, WebConfig};
        let dir = tempfile::tempdir().unwrap();
        let load = |toml: &str| {
            let p = dir.path().join("node.toml");
            std::fs::write(&p, toml).unwrap();
            NodePolicy::load(&p).unwrap()
        };
        let names = |cfg_fetch: bool, cfg_search: bool, node_toml: &str| -> (Vec<String>, String) {
            let cfg = WebConfig {
                fetch: cfg_fetch,
                search: if cfg_search {
                    SearchKind::Searxng
                } else {
                    SearchKind::None
                },
                searxng_url: "http://127.0.0.1:8888".into(),
                ..Default::default()
            };
            let (tools, key) = harness_master::providers::web_tools_from(&cfg, dir.path()).unwrap();
            let p = load(node_toml);
            let w = NodeWeb::embedded(tools, key, &cfg, &p);
            let line = w.line(&p);
            (w.tools.iter().map(|t| t.name().to_string()).collect(), line)
        };
        let url = "searxng_url = \"http://127.0.0.1:1\"\n";
        let allow = format!("[web]\nsearch = \"searxng\"\n{url}fetch = true\n");
        let no_fetch = format!("[web]\nsearch = \"searxng\"\n{url}fetch = false\n");
        let no_search = "[web]\nsearch = \"none\"\nfetch = true\n".to_string();
        // fetch: config on/off x node.toml absent / allows / takes away.
        for (cfg, nt, want) in [
            (true, "", true),
            (true, &allow, true),
            (true, &no_fetch, false),
            (false, "", false),
            (false, &allow, false),
        ] {
            let (n, line) = names(cfg, true, nt);
            assert_eq!(n.contains(&"web_fetch".to_string()), want, "{cfg} {nt:?}");
            if !want {
                let file = if cfg && !nt.is_empty() {
                    "node.toml"
                } else {
                    "config.toml"
                };
                assert!(
                    line.contains(&format!("web_fetch off ({file} [web] fetch = false")),
                    "{line}"
                );
            }
        }
        // search: the same.
        for (cfg, nt, want) in [
            (true, "", true),
            (true, &allow, true),
            (true, &no_search, false),
            (false, "", false),
            (false, &allow, false),
        ] {
            let (n, line) = names(true, cfg, nt);
            assert_eq!(n.contains(&"web_search".to_string()), want, "{cfg} {nt:?}");
            if !want {
                let file = if cfg && !nt.is_empty() {
                    "node.toml"
                } else {
                    "config.toml"
                };
                assert!(
                    line.contains(&format!("web_search off ({file} [web] search = \"none\"")),
                    "{line}"
                );
            }
        }
        // A table that leaves a key out takes that tool away (off by default).
        let (n, _) = names(true, true, "[web]\nfetch = true\n");
        assert_eq!(n, ["web_fetch"]);
        // `fetch = false` equal to the default is still a restriction.
        let (n, _) = names(true, true, &no_fetch);
        assert_eq!(n, ["web_search"]);
    }
}
