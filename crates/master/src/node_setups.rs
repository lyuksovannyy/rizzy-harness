//! Servers set up over SSH, after the setup: their saved mode and
//! protections, the watchdog, and the firewall on/off action.
//!
//! *Automatic* means harness may change the server itself (firewall on or
//! off), using a root login given for that action, or its kept sudo. *Manual*
//! means it only shows the commands. Either way the watchdog only reads and
//! reports; it never fixes anything.

use std::sync::Arc;
use std::time::Duration;

use harness_core::Secret;
use harness_provision::facts::Engine;
use harness_provision::scripts;
use harness_provision::setup::{Connector, Remote};
use harness_provision::ssh::{Auth, Ssh, Target};
use harness_provision::watch::{self, Problem};
use serde::{Deserialize, Serialize};

use crate::provision::Mode;
use crate::{Live, Master};

const PREFIX: &str = "node_setup:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupRecord {
    pub id: String,
    pub host: String,
    pub port: u16,
    /// The user harness logs in as (`harness-admin`, root, or the user's own).
    pub login_user: String,
    /// The pinned SSH host key.
    pub fingerprint: String,
    pub mode: Mode,
    pub engine: Engine,
    /// What setup put in place.
    pub ssh_protected: bool,
    pub firewall_enabled: bool,
    /// What the watchdog looks at.
    pub watch_ssh: bool,
    pub watch_firewall: bool,
    /// `harness-admin` kept passwordless sudo.
    pub admin_sudo: bool,
    pub node_id: Option<String>,
    pub created_ms: i64,
    pub last_check_ms: Option<i64>,
    pub problems: Vec<Problem>,
}

/// Opens harness-key connections for a saved server.
struct RecordConnector {
    target: Target,
    fingerprint: String,
    key: Secret<String>,
}

#[async_trait::async_trait]
impl Connector for RecordConnector {
    async fn connect(&self) -> harness_provision::Result<Box<dyn Remote>> {
        let s = Ssh::connect(
            &self.target,
            &self.fingerprint,
            Auth::Key(self.key.clone(), None),
        )
        .await?;
        Ok(Box::new(crate::provision::SshRemote(s)))
    }
    fn set_user(&self, _user: &str) {}
}

impl Master {
    fn key_path(&self, host: &str, port: u16) -> Option<std::path::PathBuf> {
        let rt = self.provider_runtime()?;
        Some(rt.data_dir().join("keys/ssh").join(format!(
            "{}_{}",
            host.replace(['/', ':'], "_"),
            port
        )))
    }

    fn connector(&self, r: &SetupRecord) -> anyhow::Result<RecordConnector> {
        let path = self
            .key_path(&r.host, r.port)
            .ok_or_else(|| anyhow::anyhow!("no data directory"))?;
        let key = std::fs::read_to_string(&path)?;
        Ok(RecordConnector {
            target: Target {
                host: r.host.clone(),
                port: r.port,
                user: r.login_user.clone(),
            },
            fingerprint: r.fingerprint.clone(),
            key: Secret::new(key),
        })
    }

    pub async fn save_setup(&self, r: &SetupRecord) -> Result<(), harness_store::StoreError> {
        let (k, v) = (
            format!("{PREFIX}{}", r.id),
            serde_json::to_string(r).expect("serializes"),
        );
        self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await
    }

    pub async fn setups(&self) -> Result<Vec<SetupRecord>, harness_store::StoreError> {
        let rows = self.inner.mirror.call(|s| s.kv_prefix(PREFIX)).await?;
        Ok(rows
            .into_iter()
            .filter_map(|(_, v)| serde_json::from_str(&v).ok())
            .collect())
    }

    pub async fn setup(&self, id: &str) -> Result<Option<SetupRecord>, harness_store::StoreError> {
        let k = format!("{PREFIX}{id}");
        let v = self.inner.mirror.call(move |s| s.kv_get(&k)).await?;
        Ok(v.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub async fn forget_setup(&self, id: &str) -> Result<bool, harness_store::StoreError> {
        let k = format!("{PREFIX}{id}");
        self.inner.mirror.call(move |s| s.kv_delete(&k)).await
    }

    /// Run the watchdog for one server now and save what it found.
    pub async fn check_setup(&self, mut r: SetupRecord) -> anyhow::Result<SetupRecord> {
        let conn = self.connector(&r)?;
        r.problems = watch::check(
            &conn.target,
            &r.fingerprint,
            &conn,
            r.watch_ssh,
            r.watch_firewall,
        )
        .await;
        r.last_check_ms = Some(harness_core::now_ms());
        self.save_setup(&r).await?;
        let _ = self.inner.live.send(Live::SessionsChanged);
        Ok(r)
    }

    /// Check every server with a watch on, every `every`.
    pub fn start_node_watchdog(&self, every: Duration) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                for r in this.setups().await.unwrap_or_default() {
                    if r.watch_ssh || r.watch_firewall {
                        let id = r.id.clone();
                        if let Err(e) = this.check_setup(r).await {
                            tracing::warn!(setup = %id, error = %e, "node watchdog failed");
                        }
                    }
                }
            }
        });
    }

    /// Automatic mode: turn the firewall on or off. Root comes from `root`
    /// (a root login used once) or from harness's kept sudo.
    pub async fn set_firewall(
        &self,
        mut r: SetupRecord,
        enable: bool,
        root: Option<Auth>,
    ) -> anyhow::Result<SetupRecord> {
        let conn = self.connector(&r)?;
        let remote: Box<dyn Remote> = match root {
            Some(auth) => {
                let t = Target {
                    host: r.host.clone(),
                    port: r.port,
                    user: "root".into(),
                };
                Box::new(crate::provision::SshRemote(
                    Ssh::connect(&t, &r.fingerprint, auth).await?,
                ))
            }
            None if r.admin_sudo => conn.connect().await?,
            None => anyhow::bail!(
                "give a root login for this action (harness keeps no root access here)"
            ),
        };
        let run = |script: String| {
            let remote = &remote;
            async move {
                let o = remote.run(&script, true).await?;
                if !o.ok() {
                    anyhow::bail!("{}", o.stderr.trim());
                }
                anyhow::Ok(o)
            }
        };
        if enable {
            run(scripts::firewall(r.port)).await?;
            // Prove a new login still works before making it permanent;
            // otherwise the revert timer takes the rules out again.
            conn.connect().await.map_err(|e| {
                anyhow::anyhow!("could not log in again with the firewall on ({e}); it reverts itself within {}s", scripts::REVERT_AFTER_S)
            })?;
            run(scripts::firewall_confirm()).await?;
        } else {
            run(scripts::firewall_disable()).await?;
        }
        r.firewall_enabled = enable;
        r.watch_firewall = enable && r.watch_firewall;
        let audit = serde_json::json!({ "host": r.host, "firewall": enable }).to_string();
        let _ = self
            .inner
            .mirror
            .call(move |s| s.audit("node.firewall", &audit))
            .await;
        self.save_setup(&r).await?;
        self.check_setup(r).await
    }
}

/// Commands for the user to run themselves (manual mode, SSH protection).
pub fn commands(r: &SetupRecord) -> serde_json::Value {
    serde_json::json!({
        "firewall_on": format!("{}\n# after checking a new SSH login works:\n{}", scripts::firewall(r.port), scripts::firewall_confirm()),
        "firewall_off": scripts::firewall_disable(),
        "ssh_on": format!("{}\n# after checking a new SSH login works:\n{}", scripts::ssh_harden(), scripts::ssh_confirm()),
        "ssh_off": scripts::ssh_unharden(),
    })
}

/// Admin attention items: servers whose last check found problems.
pub async fn attention(m: &Master) -> Vec<serde_json::Value> {
    m.setups()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.problems.is_empty())
        .map(|r| serde_json::json!({ "setup_id": r.id, "host": r.host, "node_id": r.node_id, "problems": r.problems }))
        .collect()
}

/// Shared handle type for HTTP.
pub type Record = Arc<SetupRecord>;
