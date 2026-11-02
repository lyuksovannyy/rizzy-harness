//! The single `harness` binary.

mod config;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use harness_agent_native::{
    NativeBackend, NativeConfig, Provider, mock::Mock, openai::OpenAiCompat,
};
use harness_core::{APP_NAME, Secret};
use harness_master::{Master, auth};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};

use config::{Config, ProviderKind};

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
    /// Print the UI login token.
    Token,
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

fn build_provider(
    cfg: &Config,
) -> anyhow::Result<(Arc<dyn Provider>, Vec<Secret<String>>, Vec<String>)> {
    let p = &cfg.provider;
    Ok(match p.kind {
        ProviderKind::Mock => BuiltProvider {
            provider: Arc::new(Mock {
                delay: Duration::from_millis(p.mock_delay_ms),
            }),
            secrets: vec![],
            env_remove: vec![],
        },
        ProviderKind::Openai => {
            let (key, env_remove) = match &p.api_key_env {
                Some(var) => {
                    let v = std::env::var(var)
                        .with_context(|| format!("environment variable {var} is not set"))?;
                    (Some(Secret::new(v)), vec![var.clone()])
                }
                None => (None, vec![]),
            };
            let secrets = key.iter().cloned().collect();
            BuiltProvider {
                provider: Arc::new(OpenAiCompat::new(p.base_url.clone(), key)),
                secrets,
                env_remove,
            }
        }
    })
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
        tracing::warn!(%bind, "listening on a non-loopback address WITHOUT TLS (--insecure-bind)");
    }
    let cfg = Config::load(&dir.join("harness.toml"))?;
    let policy = NodePolicy::load(&dir.join("node.toml"))?;
    let BuiltProvider {
        provider,
        secrets,
        env_remove,
    } = build_provider(&cfg)?;
    tracing::info!(provider = provider.name(), model = %cfg.provider.model, "provider");

    let node_store = Store::open(&dir.join("node.db"), Durability::Full)?;
    let mirror = Store::open(&dir.join("master.db"), Durability::Normal)?;
    let backend = NativeBackend::new(
        provider,
        harness_tools::builtin(),
        NativeConfig {
            env_remove,
            secrets: secrets.clone(),
            allow_outside_workspace: policy.allow_outside_workspace,
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
        secrets,
    })
    .await?;

    let token = auth::load_or_create_token(&dir.join("ui-token"))?;
    let master = Master::new(
        mirror,
        auth::Auth::new(token, allowed_hosts(bind, &extra_hosts), false),
    );
    master
        .add_link(node.attach().await, true)
        .await
        .map_err(anyhow::Error::msg)?;

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    let addr = listener.local_addr()?;
    // Printed (not logged) so tests and users can find the port; never the token.
    println!("{APP_NAME} listening on http://{addr}");
    println!(
        "log in with the token from {} (or run `{APP_NAME} token`)",
        dir.join("ui-token").display()
    );
    axum::serve(listener, harness_master::router(master))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
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
        let p = dir.join("ui-token");
        check(
            "ui-token permissions",
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
            let t = auth::load_or_create_token(&dir.join("ui-token"))?;
            println!("{}", t.expose());
            Ok(())
        }
        Cmd::Doctor => doctor(&dir),
        Cmd::Node {
            cmd: NodeCmd::Join { .. },
        }
        | Cmd::Pair => bail!("remote nodes arrive in Phase 2 (see PLAN.md)"),
    }
}
