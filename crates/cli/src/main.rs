//! The single `harness` binary.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use harness_agent_native::{NativeBackend, NativeConfig, Provider};
use harness_core::APP_NAME;
use harness_core::event::Profile;
use harness_master::{Master, auth};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};

use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;

#[derive(Parser)]
#[command(name = APP_NAME, version, about = "Self-hosted coding-agent harness")]
struct Cli {
    /// Data directory (default: ~/.local/share/harness).
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
        /// Allow a non-loopback bind without TLS (only on a trusted tailnet/VPN).
        #[arg(long)]
        insecure_bind: bool,
        /// Extra `host:port` values accepted in Host/Origin headers.
        #[arg(long = "allowed-host")]
        allowed_hosts: Vec<String>,
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
    /// Approve or revoke node fingerprints (Phase 2).
    Pair,
}

#[derive(Subcommand)]
enum NodeCmd {
    /// Join a master as a remote node (Phase 2).
    Join { master_url: String },
}

fn data_dir(cli: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let d = match cli {
        Some(d) => d.clone(),
        None => dirs::data_local_dir()
            .context("no data directory; pass --data-dir")?
            .join(APP_NAME),
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
    extra_hosts: Vec<String>,
) -> anyhow::Result<()> {
    if !bind.ip().is_loopback() {
        if !insecure_bind {
            bail!(
                "refusing to listen on {bind} without TLS. Use 127.0.0.1, or pass --insecure-bind on a trusted private network"
            );
        }
        tracing::warn!(
            %bind,
            "listening on a non-loopback address WITHOUT TLS (--insecure-bind): passwords and \
             session cookies travel in clear text. Use only on a trusted VPN/tailnet."
        );
    }
    let cfg = Config::load(&dir.join("harness.toml"))?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let secrets = harness_core::SecretSet::default();
    let (runtime, env_remove) = ProviderRuntime::start(cfg.clone(), dir, secrets.clone())?;
    let runtime = Arc::new(runtime);
    // Both runtimes share one swappable provider: settings can change it live.
    let provider: Arc<dyn Provider> = runtime.provider.clone();
    tracing::info!(provider = provider.name(), model = %cfg.provider.model, "provider");

    let node_store = Store::open(&dir.join("node.db"), Durability::Full)?;
    let mirror = Store::open(&dir.join("master.db"), Durability::Normal)?;
    let backend = NativeBackend::new(
        provider.clone(),
        harness_tools::builtin(),
        NativeConfig {
            env_remove,
            secrets: secrets.clone(),
            allow_outside_workspace: policy.allow_outside_workspace,
            effort: cfg
                .provider
                .effort
                .filter(|_| cfg.supports_effort(&cfg.provider.model)),
            ..Default::default()
        },
    );
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: hostname(),
        policy,
        store: node_store,
        backend: Arc::new(backend),
        default_model: cfg.provider.model.clone(),
        secrets: secrets.clone(),
        profile: Profile::Agentic,
    })
    .await?;

    // The master's own chat runtime: same session code, no workspace and no
    // file or shell tools (web tools arrive in Phase 6).
    let chat_backend = NativeBackend::new(
        provider,
        Vec::new(),
        NativeConfig {
            secrets: secrets.clone(),
            ..Default::default()
        },
    );
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: hostname(),
        policy: NodePolicy::default(),
        store: Store::open(&dir.join("chat.db"), Durability::Full)?,
        backend: Arc::new(chat_backend),
        default_model: cfg.provider.model.clone(),
        secrets,
        profile: Profile::Chat,
    })
    .await?;

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    // Use the bound address (port 0 resolves here) for the Host allowlist.
    let addr = listener.local_addr()?;
    let token = auth::load_or_create_token(&api_token_path(dir))?;
    let mut auth = auth::Auth::new(token, allowed_hosts(addr, &extra_hosts), false);
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
    let (agents, agent_errors) = harness_agents::AgentSet::load(&dir.join("agents"));
    for e in &agent_errors {
        tracing::warn!(error = %e, "skipping agent definition");
    }
    master.set_agents(agents);
    master
        .add_link(node.attach().await, true)
        .await
        .map_err(anyhow::Error::msg)?;
    master
        .add_link(chat.attach().await, true)
        .await
        .map_err(anyhow::Error::msg)?;

    // Printed (not logged) so tests and users can find the port; never a secret.
    println!("{APP_NAME} listening on http://{addr}");
    if unclaimed {
        println!(
            "first run: open the page and enter the claim code from {} (or run `{APP_NAME} claim-code`)",
            dir.join("claim-code").display()
        );
    }
    axum::serve(listener, harness_master::router(master))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
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
        "harness.toml",
        Config::load(&dir.join("harness.toml"))
            .map(|c| format!("provider {:?}, model {}", c.provider.kind, c.provider.model))
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
        .init();
    let cli = Cli::parse();
    let dir = data_dir(&cli.data_dir)?;
    match cli.cmd {
        Cmd::Serve {
            bind,
            insecure_bind,
            allowed_hosts,
        } => serve(&dir, bind, insecure_bind, allowed_hosts).await,
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
            cmd: NodeCmd::Join { .. },
        }
        | Cmd::Pair => bail!("remote nodes arrive in Phase 2 (see PLAN.md)"),
    }
}
