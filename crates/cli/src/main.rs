//! The single `harness` binary.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod net;

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
        #[arg(long, default_value = "127.0.0.1:7777")]
        bind: SocketAddr,
        /// Allow a non-loopback address that is not a WireGuard interface
        /// (only on a network you trust). WireGuard addresses need no flag.
        #[arg(long)]
        insecure_bind: bool,
        /// Serve the UI over HTTPS with a self-signed certificate (trust
        /// `keys/ui-cert.pem` once). Needed off-localhost for notifications
        /// and the microphone: browsers only allow them on secure pages.
        #[arg(long)]
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
        #[arg(long)]
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
        #[arg(long)]
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

fn data_dir(cli: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let d = match cli {
        Some(d) => d.clone(),
        None => dirs::data_local_dir()
            .context("no data directory; pass --data-dir")?
            .join(BRAND),
    };
    std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(d)
}

fn hostname() -> String {
    nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into())
}

fn allowed_hosts(bind: SocketAddr, extra: &[String]) -> Vec<String> {
    let port = bind.port();
    let mut v = vec![bind.to_string()];
    if bind.ip().is_loopback() || bind.ip().is_unspecified() {
        v.push(format!("127.0.0.1:{port}"));
        v.push(format!("localhost:{port}"));
        v.push(format!("[::1]:{port}"));
    }
    v.extend(extra.iter().cloned());
    v.sort();
    v.dedup();
    v
}

async fn serve(
    dir: &Path,
    bind: SocketAddr,
    insecure_bind: bool,
    tls: bool,
    extra_hosts: Vec<String>,
    node_listen: Option<SocketAddr>,
    no_local_node: bool,
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
    let cfg = Config::load(&dir.join(CONFIG_FILE))?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let secrets = harness_core::SecretSet::default();
    let (runtime, env_remove) = ProviderRuntime::start(cfg.clone(), dir, secrets.clone())?;
    let runtime = Arc::new(runtime);
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
                env_remove,
                &secrets,
                "local",
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
    })
    .await?;

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    // Use the bound address (port 0 resolves here) for the Host allowlist.
    let addr = listener.local_addr()?;
    let token = auth::load_or_create_token(&api_token_path(dir))?;
    let mut auth = auth::Auth::new(token, allowed_hosts(addr, &extra_hosts), tls);
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
    master.set_provider_runtime(runtime);
    master.start_incognito_purge(cfg.chat.incognito_ttl());
    // Each provider's model listing: at start when older than a day, then daily.
    master.start_model_refresh(harness_master::model_cache::REFRESH_EVERY);
    if let Some(mem) = memory {
        tracing::info!(backend = mem.name(), "memory");
        master.set_memory(mem);
    }
    if let Some(b) = builtin_memory {
        master.set_builtin_memory(b);
    }
    // Servers set up over SSH: read-only checks every 10 minutes.
    master.start_node_watchdog(std::time::Duration::from_secs(600));
    if let Some(t) = harness_master::providers::transcriber(&cfg, dir, &secrets)? {
        master.set_transcriber(t);
    }
    let (agents, agent_errors) = harness_agents::AgentSet::load(&dir.join("agents"));
    for e in &agent_errors {
        tracing::warn!(error = %e, "skipping agent definition");
    }
    master.set_agents(agents);
    if let Some((node, graphify)) = &node {
        master
            .add_link(node.attach().await, true)
            .await
            .map_err(anyhow::Error::msg)?;
        // The built-in node shares this machine and its node.toml with the
        // master, so the settings page may switch graphify for it.
        master.set_graphify(graphify.clone(), dir.join("node.toml"));
    }
    master
        .add_link(chat.attach().await, true)
        .await
        .map_err(anyhow::Error::msg)?;

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
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
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
        let l = net::TlsListener::new(listener, cert.server_config()?)?;
        axum::serve(l, harness_master::router(master))
            .with_graceful_shutdown(shutdown)
            .await?;
    } else {
        axum::serve(listener, harness_master::router(master))
            .with_graceful_shutdown(shutdown)
            .await?;
    }
    Ok(())
}

/// The model a node uses when the master sends none (the master resolves
/// the current default for every session it creates).
fn fallback_model(cfg: &Config) -> String {
    cfg.effective_default()
        .unwrap_or_else(|| format!("{}::mock", harness_master::presets::DEMO))
}

/// The worker runtime (embedded `local` node, or a remote node).
async fn agentic_node(
    dir: &Path,
    cfg: &Config,
    policy: NodePolicy,
    provider: Arc<dyn Provider>,
    env_remove: Vec<String>,
    secrets: &harness_core::SecretSet,
    node_id: &str,
) -> anyhow::Result<(NodeRuntime, Arc<harness_tools::graphify::GraphifyState>)> {
    // graphify: only if this node's own policy allows it (node.toml).
    let graphify =
        harness_tools::graphify::GraphifyState::new(policy.graphify, dir.join("graphify"));
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(harness_tools::graphify::GraphQuery::new(
        graphify.clone(),
    )));
    let backend = NativeBackend::new(
        provider,
        tools,
        NativeConfig {
            env_remove,
            secrets: secrets.clone(),
            allow_outside_workspace: policy.allow_outside_workspace,
            // The master puts the provider's default effort on each turn's
            // spec, for models that take one.
            effort: None,
            summarize_after_tokens: cfg.chat.summarize_after_tokens,
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
    })
    .await?;
    Ok((rt, graphify))
}

async fn node_join(dir: &Path, url: &str, token: &str, fingerprint: &str) -> anyhow::Result<()> {
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

async fn node_run(dir: &Path) -> anyhow::Result<()> {
    let remote: RemoteToml = toml::from_str(
        &std::fs::read_to_string(dir.join("remote.toml"))
            .context("not joined yet: run `node join` first")?,
    )?;
    let key = harness_link::NodeKey::load_or_create(&dir.join("keys/node.key"))?;
    let cfg = Config::load(&dir.join(CONFIG_FILE))?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let secrets = harness_core::SecretSet::default();
    let (runtime, env_remove) = ProviderRuntime::start(cfg.clone(), dir, secrets.clone())?;
    let provider: Arc<dyn Provider> = runtime.provider.clone();
    let (node, _graphify) = agentic_node(
        dir,
        &cfg,
        policy,
        provider,
        env_remove,
        &secrets,
        &remote.node_id,
    )
    .await?;
    let stop = tokio_util::sync::CancellationToken::new();
    let s2 = stop.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        s2.cancel();
    });
    println!("node `{}` connecting to {}", remote.node_id, remote.url);
    harness_link::client::run(
        harness_link::client::Remote {
            url: remote.url,
            fingerprint: remote.fingerprint,
            node_id: remote.node_id,
        },
        key,
        stop,
        || {
            let node = node.clone();
            async move {
                let l = node.attach().await;
                (l.to_node, l.from_node)
            }
        },
    )
    .await?;
    Ok(())
}

/// `<data>/api-token`: the scripts' bearer token.
fn api_token_path(dir: &Path) -> PathBuf {
    dir.join("api-token")
}

fn doctor(dir: &Path) -> anyhow::Result<()> {
    let mut ok = true;
    let mut check = |name: &str, res: Result<String, String>| match res {
        Ok(m) => println!("ok    {name}: {m}"),
        Err(m) => {
            ok = false;
            println!("FAIL  {name}: {m}")
        }
    };
    check("data dir", Ok(dir.display().to_string()));
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
                    "max approval mode {:?}, {} workspace root(s)",
                    p.max_approval_mode,
                    p.workspace_roots.len()
                )
            })
            .map_err(|e| e.to_string()),
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HARNESS_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        // No colour codes in `docker logs`, journald or files.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
    let cli = Cli::parse();
    let dir = data_dir(&cli.data_dir)?;
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
        Cmd::Node {
            cmd:
                NodeCmd::Join {
                    master_url,
                    token,
                    fingerprint,
                },
        } => node_join(&dir, &master_url, &token, &fingerprint).await,
        Cmd::Node { cmd: NodeCmd::Run } => node_run(&dir).await,
    }
}
