//! The two setup modes. **Automatic** (fresh VPS / node-only machines) runs
//! every step; **manual** (machines with other work on them) inspects and
//! shows the commands to run, then re-checks. Both start the same way: the
//! user's password or key logs in once, harness installs its own key,
//! proves it works on a new connection, and from then on only that key is
//! used. The user's credentials are never stored.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::facts::{Facts, INSPECT};
use crate::scripts;
use crate::ssh::Output;
use crate::wg::{MASTER_IP, Peer};
use crate::{Error, Result};

/// Something that runs scripts on the server (an SSH connection in real
/// use, a recorder in tests).
#[async_trait::async_trait]
pub trait Remote: Send + Sync {
    async fn run(&self, script: &str, root: bool) -> Result<Output>;
    async fn upload(&self, path: &str, data: &[u8]) -> Result<()>;
}

/// Opens a *new* connection with harness's own key. Used to prove a change
/// did not lock us out before confirming it.
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn Remote>>;
}

/// What the master provides during setup.
#[async_trait::async_trait]
pub trait Master: Send + Sync {
    /// Harness's public login key for this node.
    fn harness_pubkey(&self) -> String;
    /// The master's WireGuard public key and the tunnel address for the node.
    fn wg_plan(&self) -> Result<(String, String)>;
    /// Record the node's tunnel peer; returns the command the user runs on
    /// the master (needs root once).
    fn wg_add_peer(&self, peer: Peer) -> Result<String>;
    /// `wss://... --token ... --fingerprint ...` for `node join`.
    async fn join_args(&self) -> Result<String>;
    /// Provider config to copy, if the user allowed it.
    fn provider_files(&self) -> Option<scripts::ProviderFiles>;
    /// The harness binary for the node's architecture.
    fn binary(&self, arch: &str) -> Result<Vec<u8>>;
    /// The master's own `(arch, glibc)`.
    fn platform(&self) -> (String, String);
    /// Wait until the node `name` is connected.
    async fn wait_online(&self, name: &str) -> Result<()>;
    /// Wait for the user to say they brought the master's tunnel up.
    async fn wait_continue(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    Ok,
    Failed,
    /// Manual mode: run `script` yourself, then re-check.
    Todo,
    /// Waiting for the user (e.g. bring the master's tunnel up).
    Waiting,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub title: &'static str,
    pub status: Status,
    pub detail: String,
    /// Commands to run (manual mode) or what will run (automatic mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Run with root rights (`sudo bash`).
    pub root: bool,
}

/// Live step list, shared with the UI.
#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<Step>>>);

impl Log {
    pub fn snapshot(&self) -> Vec<Step> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(
        &self,
        id: &'static str,
        title: &'static str,
        status: Status,
        detail: impl Into<String>,
    ) {
        let mut v = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let detail = detail.into();
        match v.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.status = status;
                s.detail = detail;
            }
            None => v.push(Step {
                id,
                title,
                status,
                detail,
                script: None,
                root: false,
            }),
        }
    }

    fn set_script(&self, id: &'static str, script: String, root: bool) {
        if let Some(s) = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter_mut()
            .find(|s| s.id == id)
        {
            s.script = Some(script);
            s.root = root;
        }
    }

    /// Record a failed step (e.g. the very first login).
    pub fn push_failed(&self, id: &'static str, title: &'static str, detail: &str) {
        self.set(id, title, Status::Failed, detail);
    }

    fn replace(&self, steps: Vec<Step>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = steps;
    }
}

async fn must(r: &dyn Remote, script: &str, root: bool, what: &str) -> Result<Output> {
    let o = r.run(script, root).await?;
    if o.ok() {
        Ok(o)
    } else {
        let msg = if o.stderr.trim().is_empty() {
            o.stdout.trim()
        } else {
            o.stderr.trim()
        };
        Err(Error::Step(format!(
            "{what} failed (exit {}): {}",
            o.code,
            last_lines(msg, 6)
        )))
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let v: Vec<&str> = s.lines().collect();
    v[v.len().saturating_sub(n)..].join("\n")
}

/// Step 1 of both modes: install harness's key with the user's login, then
/// log in again with that key alone. Returns the key-based connection.
pub async fn bootstrap(
    first: Box<dyn Remote>,
    conn: &dyn Connector,
    master: &dyn Master,
    log: &Log,
) -> Result<Box<dyn Remote>> {
    let (id, title) = ("login", "Harness login key");
    log.set(
        id,
        title,
        Status::Running,
        "installing harness's own SSH key",
    );
    must(
        &*first,
        &scripts::install_key(&master.harness_pubkey()),
        false,
        "installing the key",
    )
    .await?;
    drop(first);
    let r = conn.connect().await.map_err(|e| {
        log.set(
            id,
            title,
            Status::Failed,
            format!("the key was added but logging in with it failed: {e}"),
        );
        e
    })?;
    log.set(
        id,
        title,
        Status::Ok,
        "harness logs in with its own key; your password/key was not stored",
    );
    Ok(r)
}

pub async fn inspect(r: &dyn Remote) -> Result<Facts> {
    let o = r.run(INSPECT, false).await?;
    Ok(Facts::parse(&o.stdout))
}

/// Apply a change that could lock us out, prove a *new* key login still
/// works, then confirm (cancel the automatic revert).
async fn guarded(conn: &dyn Connector, apply: &str, confirm: &str, what: &str) -> Result<()> {
    let first = conn.connect().await?;
    must(&*first, apply, true, what).await?;
    drop(first);
    let again = conn.connect().await.map_err(|e| {
        Error::Step(format!(
            "could not log in again after the {what} change ({e}); it reverts itself within {}s",
            scripts::REVERT_AFTER_S
        ))
    })?;
    must(
        &*again,
        confirm,
        true,
        &format!("confirming the {what} change"),
    )
    .await?;
    Ok(())
}

pub struct AutoOptions {
    /// Name for the node (its WireGuard peer); the join assigns the node id.
    pub name: String,
    /// The node's public address for the tunnel endpoint.
    pub endpoint_host: String,
    /// The user's own public key, kept so they can still log in.
    pub own_pubkey: Option<String>,
}

/// Automatic setup after `bootstrap`. Stops at the first failure.
pub async fn run_auto(
    r: Box<dyn Remote>,
    conn: &dyn Connector,
    master: &dyn Master,
    opts: &AutoOptions,
    log: &Log,
) -> Result<String> {
    let fail = |id: &'static str, title: &'static str, e: Error| {
        log.set(id, title, Status::Failed, e.to_string());
        e
    };

    let (id, title) = ("inspect", "Check the server");
    log.set(id, title, Status::Running, "");
    let f = inspect(&*r).await.map_err(|e| fail(id, title, e))?;
    let (arch, glibc) = master.platform();
    let problems: Vec<String> = [
        (!f.root).then(|| "needs root or passwordless sudo".to_string()),
        (!f.supported_os()).then(|| format!("{} is not Debian/Ubuntu", f.os_id)),
        (!f.systemd).then(|| "systemd is not running".to_string()),
        (f.arch != arch).then(|| format!("CPU is {} but this harness build is {arch}", f.arch)),
        (!f.glibc_at_least(&glibc)).then(|| format!("glibc {} is older than {glibc}", f.glibc)),
        f.firewall_conflict()
            .map(|c| format!("{c}: not a fresh server, use manual setup")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !problems.is_empty() {
        return Err(fail(id, title, Error::Step(problems.join("; "))));
    }
    log.set(
        id,
        title,
        Status::Ok,
        format!(
            "{} {} {}, ssh port {}",
            f.os_id, f.os_version, f.arch, f.ssh_port
        ),
    );

    let (id, title) = ("packages", "Install WireGuard and nftables");
    log.set(id, title, Status::Running, "");
    log.set_script(id, scripts::packages(), true);
    must(&*r, &scripts::packages(), true, "installing packages")
        .await
        .map_err(|e| fail(id, title, e))?;
    log.set(id, title, Status::Ok, "");
    drop(r);

    let (id, title) = ("ssh", "Key-only SSH");
    log.set(
        id,
        title,
        Status::Running,
        format!(
            "reverts itself in {}s unless confirmed",
            scripts::REVERT_AFTER_S
        ),
    );
    log.set_script(id, scripts::ssh_harden(), true);
    if let Some(k) = &opts.own_pubkey {
        let c = conn.connect().await.map_err(|e| fail(id, title, e))?;
        must(&*c, &scripts::install_key(k), false, "adding your key")
            .await
            .map_err(|e| fail(id, title, e))?;
    }
    guarded(conn, &scripts::ssh_harden(), &scripts::ssh_confirm(), "SSH")
        .await
        .map_err(|e| fail(id, title, e))?;
    log.set(id, title, Status::Ok, "passwords off, root by key only");

    let (id, title) = ("firewall", "Firewall (nftables)");
    log.set(
        id,
        title,
        Status::Running,
        format!(
            "reverts itself in {}s unless confirmed",
            scripts::REVERT_AFTER_S
        ),
    );
    log.set_script(id, scripts::firewall(f.ssh_port), true);
    guarded(
        conn,
        &scripts::firewall(f.ssh_port),
        &scripts::firewall_confirm(),
        "firewall",
    )
    .await
    .map_err(|e| fail(id, title, e))?;
    log.set(
        id,
        title,
        Status::Ok,
        format!(
            "inbound: SSH {} and WireGuard {}/udp only",
            f.ssh_port,
            scripts::WG_PORT
        ),
    );

    let (id, title) = ("wireguard", "WireGuard tunnel");
    log.set(id, title, Status::Running, "");
    let (master_pub, node_ip) = master.wg_plan().map_err(|e| fail(id, title, e))?;
    let script = scripts::wireguard(&node_ip, MASTER_IP, &master_pub);
    log.set_script(id, script.clone(), true);
    let c = conn.connect().await.map_err(|e| fail(id, title, e))?;
    let o = must(&*c, &script, true, "WireGuard")
        .await
        .map_err(|e| fail(id, title, e))?;
    let node_pub = o
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("NODE_PUBKEY="))
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            fail(
                id,
                title,
                Error::Step("WireGuard did not report the node's key".into()),
            )
        })?
        .to_string();
    let cmd = master
        .wg_add_peer(Peer {
            name: opts.name.clone(),
            public_key: node_pub,
            endpoint: format!("{}:{}", opts.endpoint_host, scripts::WG_PORT),
            ip: node_ip.clone(),
        })
        .map_err(|e| fail(id, title, e))?;
    log.set(id, title, Status::Ok, format!("node {node_ip}"));

    let (id, title) = ("tunnel", "Bring the tunnel up on this machine");
    log.set(
        id,
        title,
        Status::Waiting,
        format!("Run on the master (root, once per new node), then continue:\n{cmd}"),
    );
    master.wait_continue().await;
    log.set(id, title, Status::Ok, "");

    let (id, title) = ("harness", "Install and join");
    log.set(id, title, Status::Running, "uploading harness");
    let bin = master.binary(&f.arch).map_err(|e| fail(id, title, e))?;
    c.upload("/tmp/harness.upload", &bin)
        .await
        .map_err(|e| fail(id, title, e))?;
    let join = master.join_args().await.map_err(|e| fail(id, title, e))?;
    let provider = master.provider_files();
    let o = must(
        &*c,
        &scripts::node_service(&join, provider.as_ref()),
        true,
        "installing the node",
    )
    .await
    .map_err(|e| fail(id, title, e))?;
    let node_id = o
        .stdout
        .lines()
        .find_map(|l| {
            l.split_once("joined as node `")
                .and_then(|(_, r)| r.split_once('`'))
                .map(|(id, _)| id.to_string())
        })
        .unwrap_or_else(|| opts.name.clone());
    log.set(
        id,
        title,
        Status::Running,
        format!("waiting for node `{node_id}` to connect"),
    );
    master
        .wait_online(&node_id)
        .await
        .map_err(|e| fail(id, title, e))?;
    log.set(id, title, Status::Ok, format!("node `{node_id}` is online"));
    Ok(node_id)
}

/// Manual mode: what is in place, and the commands for what is not.
pub fn checklist(f: &Facts, master_pub: &str, node_ip: &str, join_hint: &str) -> Vec<Step> {
    let step = |id, title, ok: bool, detail: String, script: Option<String>, root| Step {
        id,
        title,
        status: if ok { Status::Ok } else { Status::Todo },
        detail,
        script: if ok { None } else { script },
        root,
    };
    let fw_detail;
    let fw_script;
    if let Some(c) = f.firewall_conflict() {
        fw_detail = format!(
            "{c}. Keep your firewall and allow WireGuard: UDP {} in, SSH ({}) stays open.",
            scripts::WG_PORT,
            f.ssh_port
        );
        fw_script = Some(if f.ufw_active {
            format!(
                "ufw allow {}/udp\nufw allow {}/tcp\n",
                scripts::WG_PORT,
                f.ssh_port
            )
        } else if f.firewalld_active {
            format!(
                "firewall-cmd --permanent --add-port={}/udp\nfirewall-cmd --permanent --add-port={}/tcp\nfirewall-cmd --reload\n",
                scripts::WG_PORT,
                f.ssh_port
            )
        } else {
            format!(
                "# Add to your existing rules: allow UDP {} and keep TCP {} open.\n# Docker users: these are INPUT rules; container ports are not affected.\n",
                scripts::WG_PORT,
                f.ssh_port
            )
        });
    } else {
        fw_detail = format!(
            "No firewall found. Run this; it reverts itself in {}s. Then log in from a NEW terminal and run the confirm part.",
            scripts::REVERT_AFTER_S
        );
        fw_script = Some(format!(
            "{}\n# after checking a new SSH login works:\n{}",
            scripts::firewall(f.ssh_port),
            scripts::firewall_confirm()
        ));
    }
    vec![
        step(
            "login",
            "Harness login key",
            true,
            "installed and working".into(),
            None,
            false,
        ),
        step(
            "root",
            "Root access",
            f.root,
            if f.root {
                "available".into()
            } else {
                "the commands below need sudo".into()
            },
            None,
            false,
        ),
        step(
            "wireguard",
            "WireGuard tunnel",
            f.wg_up,
            if f.wg_up {
                "wg-harness is up".into()
            } else {
                "Install WireGuard and create the tunnel:".into()
            },
            Some(format!(
                "{}{}",
                scripts::packages(),
                scripts::wireguard(node_ip, MASTER_IP, master_pub)
            )),
            true,
        ),
        step(
            "firewall",
            "Firewall",
            f.harness_fw,
            if f.harness_fw {
                "harness rules loaded".into()
            } else {
                fw_detail
            },
            fw_script,
            true,
        ),
        step(
            "ssh",
            "Key-only SSH",
            !f.ssh_password,
            if f.ssh_password {
                format!(
                    "Password logins are on. Run this; it reverts itself in {}s. Then log in from a NEW terminal and run the confirm part.",
                    scripts::REVERT_AFTER_S
                )
            } else {
                "passwords off".into()
            },
            Some(format!(
                "{}\n# after checking a new SSH login works:\n{}",
                scripts::ssh_harden(),
                scripts::ssh_confirm()
            )),
            true,
        ),
        step(
            "harness",
            "Harness node",
            f.harness_service,
            if f.harness_service {
                "running".into()
            } else {
                format!(
                    "Copy the harness binary to /tmp/harness.upload on the server, then run:\n{join_hint}"
                )
            },
            None,
            true,
        ),
    ]
}

/// Manual mode, first run: key bootstrap then the checklist.
pub async fn run_manual(
    first: Box<dyn Remote>,
    conn: &dyn Connector,
    master: &dyn Master,
    log: &Log,
) -> Result<Facts> {
    let r = bootstrap(first, conn, master, log).await?;
    recheck(&*r, master, log).await
}

/// Manual mode: inspect again and rebuild the checklist. A node WireGuard
/// key that appeared is registered with the master.
pub async fn recheck(r: &dyn Remote, master: &dyn Master, log: &Log) -> Result<Facts> {
    let f = inspect(r).await?;
    let (master_pub, node_ip) = master.wg_plan()?;
    let mut steps = checklist(
        &f,
        &master_pub,
        &node_ip,
        "(the join command is created when this step is reached)",
    );
    if let (Some(pk), Some(s)) = (&f.wg_pubkey, steps.iter_mut().find(|s| s.id == "wireguard")) {
        let cmd = master.wg_add_peer(Peer {
            name: f.public_ip.clone(),
            public_key: pk.clone(),
            endpoint: format!("{}:{}", f.public_ip, scripts::WG_PORT),
            ip: node_ip.clone(),
        })?;
        s.detail = format!("{}\nOn this machine (master), once: {cmd}", s.detail);
    }
    if let Some(s) = steps.iter_mut().find(|s| s.id == "harness")
        && s.status == Status::Todo
    {
        let join = master.join_args().await?;
        s.script = Some(scripts::node_service(
            &join,
            master.provider_files().as_ref(),
        ));
    }
    let mut all = vec![
        log.snapshot()
            .into_iter()
            .find(|s| s.id == "login")
            .unwrap_or(steps[0].clone()),
    ];
    all.extend(steps.into_iter().filter(|s| s.id != "login"));
    log.replace(all);
    Ok(f)
}
