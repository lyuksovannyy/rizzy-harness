//! Setting up worker nodes over SSH from the UI (Settings → Nodes → Set up
//! a server). Jobs live in memory. The user's password or private key is
//! held only until harness's own key is installed and proven; harness's key
//! and the server's pinned host key are kept so manual mode can re-check.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::Secret;
use harness_provision::scripts::ProviderFiles;
use harness_provision::setup::{self, Connector, Log, Remote, Status};
use harness_provision::ssh::{Auth, HarnessKey, Output, Ssh, Target};
use harness_provision::wg::{IFACE, MASTER_IP, MasterWg, Peer};
use harness_provision::{Error, Result};
use serde::Serialize;
use tokio::sync::Notify;

use crate::{Master, lock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Fresh VPS / node-only machine: harness changes everything itself.
    Auto,
    /// A server with other work on it: inspect and explain.
    Manual,
}

/// An SSH connection as a setup `Remote`.
struct SshRemote(Ssh);

#[async_trait::async_trait]
impl Remote for SshRemote {
    async fn run(&self, script: &str, root: bool) -> Result<Output> {
        if root {
            self.0.root_script(script).await
        } else {
            self.0.exec("bash -s", Some(script.as_bytes())).await
        }
    }

    async fn upload(&self, path: &str, data: &[u8]) -> Result<()> {
        let o = self
            .0
            .exec(&format!("umask 077 && cat > '{path}'"), Some(data))
            .await?;
        if o.ok() {
            Ok(())
        } else {
            Err(Error::Step(format!("upload failed: {}", o.stderr.trim())))
        }
    }
}

/// New connections with harness's key and the pinned host key.
struct KeyConnector {
    target: Target,
    fingerprint: String,
    key: Secret<String>,
}

#[async_trait::async_trait]
impl Connector for KeyConnector {
    async fn connect(&self) -> Result<Box<dyn Remote>> {
        let s = Ssh::connect(
            &self.target,
            &self.fingerprint,
            Auth::Key(self.key.clone(), None),
        )
        .await?;
        Ok(Box::new(SshRemote(s)))
    }
}

pub struct Job {
    pub id: String,
    pub mode: Mode,
    pub host: String,
    pub log: Log,
    done: Mutex<Option<std::result::Result<String, String>>>,
    side: Arc<Side>,
    connector: KeyConnector,
}

impl Job {
    pub fn state(&self) -> serde_json::Value {
        let done = lock(&self.done).clone();
        serde_json::json!({
            "id": self.id,
            "mode": self.mode,
            "host": self.host,
            "steps": self.log.snapshot(),
            "finished": done.is_some(),
            "node_id": done.as_ref().and_then(|d| d.as_ref().ok()),
            "error": done.as_ref().and_then(|d| d.as_ref().err()),
        })
    }
}

/// What the master contributes to a setup.
struct Side {
    master: Master,
    data_dir: PathBuf,
    key_pub: String,
    name: String,
    copy_provider: bool,
    cont: Arc<Notify>,
}

fn local_glibc() -> String {
    std::process::Command::new("ldd")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .and_then(|l| l.rsplit(' ').next())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "2.0".into())
}

impl Side {
    fn wg(&self) -> Result<MasterWg> {
        MasterWg::load_or_create(&self.data_dir.join("keys"))
    }
}

#[async_trait::async_trait]
impl setup::Master for Side {
    fn harness_pubkey(&self) -> String {
        self.key_pub.clone()
    }

    fn wg_plan(&self) -> Result<(String, String)> {
        let wg = self.wg()?;
        Ok((wg.public_key.clone(), wg.address_for(&self.name)?))
    }

    fn wg_add_peer(&self, peer: Peer) -> Result<String> {
        let dir = self.data_dir.join("keys");
        let mut wg = self.wg()?;
        wg.upsert(peer);
        wg.save(&dir)?;
        let conf = dir.join(format!("{IFACE}.conf"));
        Ok(
            if std::path::Path::new(&format!("/sys/class/net/{IFACE}")).exists() {
                format!(
                    "sudo bash -c 'wg syncconf {IFACE} <(wg-quick strip {})'",
                    conf.display()
                )
            } else {
                format!("sudo wg-quick up {}", conf.display())
            },
        )
    }

    async fn join_args(&self) -> Result<String> {
        let (port, fp) = self.master.node_listener().ok_or_else(|| {
            Error::Step(format!(
                "remote nodes are off: start the server with --node-listen {MASTER_IP}:7443 (after the tunnel is up) or 0.0.0.0:7443"
            ))
        })?;
        let token = self
            .master
            .create_join_token(None)
            .await
            .map_err(|e| Error::Step(e.to_string()))?;
        Ok(format!(
            "wss://{MASTER_IP}:{port} --token {token} --fingerprint {fp}"
        ))
    }

    fn provider_files(&self) -> Option<ProviderFiles> {
        if !self.copy_provider {
            return None;
        }
        let rt = self.master.provider_runtime()?;
        let cfg = rt.config();
        let (key, _) = crate::providers::resolve_key(&cfg, rt.data_dir()).ok()?;
        let mut node = crate::config::Config {
            provider: cfg.provider.clone(),
            models: cfg.models.clone(),
            ..Default::default()
        };
        node.provider.api_key_env = None;
        node.provider.api_key_secret = key.as_ref().map(|_| "provider".to_string());
        let secrets_toml = key.map(|k| {
            let mut keys = toml::map::Map::new();
            keys.insert("provider".into(), toml::Value::String(k.expose().clone()));
            let mut root = toml::map::Map::new();
            root.insert("keys".into(), toml::Value::Table(keys));
            toml::to_string(&toml::Value::Table(root)).unwrap_or_default()
        });
        Some(ProviderFiles {
            harness_toml: toml::to_string(&node).ok()?,
            secrets_toml,
        })
    }

    fn binary(&self, arch: &str) -> Result<Vec<u8>> {
        let own = std::env::consts::ARCH;
        let path = if arch == own {
            std::env::current_exe().map_err(|e| Error::Step(e.to_string()))?
        } else {
            self.data_dir.join(format!("bin/harness-{arch}"))
        };
        std::fs::read(&path).map_err(|_| {
            Error::Step(format!(
                "no harness build for {arch}: place one at {}",
                self.data_dir.join(format!("bin/harness-{arch}")).display()
            ))
        })
    }

    fn platform(&self) -> (String, String) {
        (std::env::consts::ARCH.to_string(), local_glibc())
    }

    async fn wait_online(&self, name: &str) -> Result<()> {
        for _ in 0..90 {
            if self
                .master
                .nodes()
                .iter()
                .any(|n| n.node_id == name && n.connected)
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Err(Error::Step(format!(
            "node `{name}` did not connect within 90 s; is the tunnel up on this machine (step above)?"
        )))
    }

    async fn wait_continue(&self) {
        self.cont.notified().await;
    }
}

pub struct StartRequest {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub fingerprint: String,
    pub auth: Auth,
    pub mode: Mode,
    pub own_pubkey: Option<String>,
    pub copy_provider: bool,
}

impl Master {
    /// Start a setup job; the credentials in `req.auth` are consumed by the
    /// first login and dropped.
    pub fn start_provision(&self, req: StartRequest) -> anyhow::Result<Arc<Job>> {
        let rt = self
            .provider_runtime()
            .ok_or_else(|| anyhow::anyhow!("no data directory"))?;
        let data_dir = rt.data_dir().to_path_buf();
        // Harness's key for this server (reused when setting up again).
        let key_path = data_dir.join("keys/ssh").join(format!(
            "{}_{}",
            req.host.replace(['/', ':'], "_"),
            req.port
        ));
        let key = match std::fs::read_to_string(&key_path) {
            Ok(t) => HarnessKey::from_openssh(&t)?,
            Err(_) => {
                let k = HarnessKey::generate();
                harness_link::write_private(&key_path, k.private_openssh().expose().as_bytes())?;
                k
            }
        };
        let target = Target {
            host: req.host.clone(),
            port: req.port,
            user: req.user.clone(),
        };
        let cont = Arc::new(Notify::new());
        let side = Arc::new(Side {
            master: self.clone(),
            data_dir,
            key_pub: key.public_openssh(),
            name: req.host.clone(),
            copy_provider: req.copy_provider,
            cont: cont.clone(),
        });
        let job = Arc::new(Job {
            id: harness_core::new_id(),
            mode: req.mode,
            host: req.host.clone(),
            log: Log::default(),
            done: Mutex::new(None),
            side: side.clone(),
            connector: KeyConnector {
                target: target.clone(),
                fingerprint: req.fingerprint.clone(),
                key: key.private_openssh(),
            },
        });
        lock(&self.inner.provision).insert(job.id.clone(), job.clone());
        let audit = serde_json::json!({ "host": req.host, "port": req.port, "user": req.user, "mode": req.mode }).to_string();
        let m = self.clone();
        tokio::spawn(async move {
            let _ = m
                .inner
                .mirror
                .call(move |s| s.audit("node.setup", &audit))
                .await;
        });

        let j = job.clone();
        let (auth, fp, own, mode) = (req.auth, req.fingerprint, req.own_pubkey, req.mode);
        let continue_signal = cont;
        tokio::spawn(async move {
            let _keep = continue_signal;
            let res: Result<String> = async {
                let first = Ssh::connect(&target, &fp, auth).await.inspect_err(|e| {
                    j.log_failure("login", "Harness login key", e);
                })?;
                let first: Box<dyn Remote> = Box::new(SshRemote(first));
                match mode {
                    Mode::Auto => {
                        let r = setup::bootstrap(first, &j.connector, &*j.side, &j.log).await?;
                        let opts = setup::AutoOptions {
                            name: target.host.clone(),
                            endpoint_host: target.host.clone(),
                            own_pubkey: own,
                        };
                        setup::run_auto(r, &j.connector, &*j.side, &opts, &j.log).await
                    }
                    Mode::Manual => {
                        setup::run_manual(first, &j.connector, &*j.side, &j.log).await?;
                        Ok(String::new())
                    }
                }
            }
            .await;
            *lock(&j.done) = Some(res.map_err(|e| e.to_string()));
        });
        Ok(job)
    }

    pub fn provision_job(&self, id: &str) -> Option<Arc<Job>> {
        lock(&self.inner.provision).get(id).cloned()
    }
}

impl Job {
    fn log_failure(&self, id: &'static str, title: &'static str, e: &Error) {
        self.log.push_failed(id, title, &e.to_string());
    }

    /// The user brought the master's tunnel up.
    pub fn resume(&self) {
        self.side.cont.notify_one();
    }

    /// Manual mode: inspect again with harness's key.
    pub async fn recheck(&self) -> Result<()> {
        let r = self.connector.connect().await?;
        setup::recheck(&*r, &*self.side, &self.log)
            .await
            .map(|_| ())
    }

    pub fn waiting(&self) -> bool {
        self.log
            .snapshot()
            .iter()
            .any(|s| s.status == Status::Waiting)
    }
}

/// Jobs by id.
pub type Jobs = Mutex<HashMap<String, Arc<Job>>>;
