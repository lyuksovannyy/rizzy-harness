//! The two setup modes.
//!
//! **Automatic** (fresh VPS / node-only machines) needs the *root* login.
//! With it, harness creates its own user `harness-admin`, gives it its own
//! SSH key and (for the setup only) sudo, and from then on logs in only as
//! that user. At the end harness removes its own sudo unless asked to keep
//! it: a compromised master must not be root on every node.
//!
//! **Manual** (machines with other work on them) should get a normal user.
//! Root is accepted with a warning; harness then offers to create its own
//! user (without sudo) and to take its key off root.
//!
//! In both modes the user's password or private key is used for the first
//! login only and never stored.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::facts::{Engine, Facts, INSPECT, Support};
use crate::scripts::{self, ADMIN_USER, RUNTIME_USER};
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

/// Opens *new* connections with harness's own key, as the current user.
/// Used to prove a change did not lock us out before confirming it.
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn Remote>>;
    /// Log in as another user from now on (e.g. `harness-admin`).
    fn set_user(&self, user: &str);
}

/// What the master provides during setup.
#[async_trait::async_trait]
pub trait Master: Send + Sync {
    /// Harness's public login key for this server.
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
    /// Wait for the user to pick one of a waiting step's choices.
    async fn wait_answer(&self, step: &str) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    Ok,
    /// Done, with something the user should know.
    Warning,
    Failed,
    /// Manual mode: run `script` yourself, then re-check.
    Todo,
    /// Waiting for the user to pick one of `choices`.
    Waiting,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
}

impl Step {
    fn new(
        id: &'static str,
        title: &'static str,
        status: Status,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            title,
            status,
            detail: detail.into(),
            script: None,
            root: false,
            choices: vec![],
        }
    }
}

/// Live step list, shared with the UI.
#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<Step>>>);

impl Log {
    pub fn snapshot(&self) -> Vec<Step> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn upsert(&self, step: Step) {
        let mut v = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match v.iter_mut().find(|s| s.id == step.id) {
            Some(s) => {
                let script = s.script.take();
                *s = Step {
                    script: step.script.clone().or(script),
                    ..step
                };
            }
            None => v.push(step),
        }
    }

    fn set(
        &self,
        id: &'static str,
        title: &'static str,
        status: Status,
        detail: impl Into<String>,
    ) {
        self.upsert(Step::new(id, title, status, detail));
    }

    fn ask(
        &self,
        id: &'static str,
        title: &'static str,
        detail: impl Into<String>,
        choices: Vec<Choice>,
    ) {
        self.upsert(Step {
            choices,
            ..Step::new(id, title, Status::Waiting, detail)
        });
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

    /// The step waiting for an answer, if any, and whether `choice` is one
    /// of its choices.
    pub fn accepts(&self, step: &str, choice: &str) -> bool {
        self.snapshot().iter().any(|s| {
            s.id == step && s.status == Status::Waiting && s.choices.iter().any(|c| c.id == choice)
        })
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

fn engine_note(e: Engine) -> &'static str {
    match e {
        Engine::Podman => {
            "rootless Podman for the `harness` user (no daemon, no root-equivalent group)"
        }
        Engine::Docker => {
            "Docker; the `harness` user joins the `docker` group. Members of that group are \
             effectively root on this server: an agent talked into `docker run -v /:/host` can \
             take it over. Prefer Podman unless you need Docker."
        }
        Engine::None => "none",
    }
}

pub struct AutoOptions {
    /// Name for the node (its WireGuard peer); the join assigns the node id.
    pub name: String,
    /// The node's public address for the tunnel endpoint.
    pub endpoint_host: String,
    /// The user's own public key, added to root so they keep access once
    /// password logins are off.
    pub own_pubkey: Option<String>,
    pub engine: Engine,
    /// Keep passwordless sudo for `harness-admin` after setup.
    pub keep_admin_sudo: bool,
}

/// Automatic setup. `first` is the root login with the user's credentials;
/// it is used to create `harness-admin` and dropped right after.
pub async fn run_auto(
    first: Box<dyn Remote>,
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
    let f = inspect(&*first).await.map_err(|e| fail(id, title, e))?;
    if !f.is_root_login {
        return Err(fail(
            id,
            title,
            Error::Step(format!(
                "automatic setup needs the root login, not `{}`; harness then creates its own user `{ADMIN_USER}`. \
                 Use manual setup to keep your own user.",
                f.login_user
            )),
        ));
    }
    let (arch, glibc) = master.platform();
    let support = f.support();
    let mut problems: Vec<String> = vec![];
    if let Support::Unsupported(why) = &support {
        problems.push(why.clone());
    }
    if !f.systemd {
        problems.push("systemd is not running".into());
    }
    if f.arch != arch {
        problems.push(format!(
            "CPU is {} but this harness build is {arch}",
            f.arch
        ));
    }
    if !f.glibc_at_least(&glibc) {
        problems.push(format!("glibc {} is older than {glibc}", f.glibc));
    }
    if let Some(c) = f.firewall_conflict() {
        problems.push(format!("{c}: not a fresh server, use manual setup"));
    }
    if !problems.is_empty() {
        return Err(fail(id, title, Error::Step(problems.join("; "))));
    }
    let summary = format!(
        "{} {} {}, ssh port {}",
        f.os_id, f.os_version, f.arch, f.ssh_port
    );
    match support {
        Support::Limited(note) => log.set(id, title, Status::Warning, format!("{summary}. {note}")),
        _ => log.set(id, title, Status::Ok, summary),
    }

    let (id, title) = ("login", "Harness's own user");
    log.set(
        id,
        title,
        Status::Running,
        format!("creating `{ADMIN_USER}` with harness's own SSH key"),
    );
    let script = scripts::admin_user(&master.harness_pubkey(), true);
    log.set_script(id, script.clone(), true);
    must(&*first, &script, true, "creating the user")
        .await
        .map_err(|e| fail(id, title, e))?;
    if let Some(k) = &opts.own_pubkey {
        must(
            &*first,
            &scripts::install_key(k),
            false,
            "adding your key to root",
        )
        .await
        .map_err(|e| fail(id, title, e))?;
    }
    drop(first);
    conn.set_user(ADMIN_USER);
    let r = conn.connect().await.map_err(|e| {
        fail(
            id,
            title,
            Error::Step(format!(
                "`{ADMIN_USER}` was created but logging in as it failed: {e}"
            )),
        )
    })?;
    log.set(
        id,
        title,
        Status::Ok,
        format!("harness logs in as `{ADMIN_USER}` with its own key; your root password/key was not stored"),
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
    guarded(conn, &scripts::ssh_harden(), &scripts::ssh_confirm(), "SSH")
        .await
        .map_err(|e| fail(id, title, e))?;
    let root_note = if opts.own_pubkey.is_some() {
        "root by your key only"
    } else {
        "root over SSH is now off for passwords; use your provider's console if you need it"
    };
    log.set(id, title, Status::Ok, format!("passwords off; {root_note}"));

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

    let (id, title) = ("engine", "Container engine");
    if opts.engine == Engine::None {
        log.set(id, title, Status::Skipped, "none selected");
    } else {
        log.set(id, title, Status::Running, engine_note(opts.engine));
        let script = scripts::container_engine(opts.engine);
        log.set_script(id, script.clone(), true);
        let c = conn.connect().await.map_err(|e| fail(id, title, e))?;
        must(&*c, &script, true, "setting up the container engine")
            .await
            .map_err(|e| fail(id, title, e))?;
        let status = if opts.engine == Engine::Docker {
            Status::Warning
        } else {
            Status::Ok
        };
        log.set(id, title, status, engine_note(opts.engine));
    }

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
    log.ask(
        id,
        title,
        format!("Run on the master (root, once per new node), then continue:\n{cmd}"),
        vec![Choice {
            id: "continue",
            label: "I ran it, continue",
        }],
    );
    master.wait_answer(id).await;
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
        &scripts::node_service(&join, provider.as_ref(), opts.engine),
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
    log.set(
        id,
        title,
        Status::Ok,
        format!("node `{node_id}` is online, running as `{RUNTIME_USER}` (no sudo)"),
    );

    let (id, title) = ("lockdown", "Remove harness's root access");
    if opts.keep_admin_sudo {
        log.set(
            id,
            title,
            Status::Warning,
            format!("kept on request: `{ADMIN_USER}` still has passwordless sudo, so this master can act as root here"),
        );
    } else {
        must(&*c, &scripts::drop_admin_sudo(), true, "removing sudo")
            .await
            .map_err(|e| fail(id, title, e))?;
        log.set(
            id,
            title,
            Status::Ok,
            format!("`{ADMIN_USER}` keeps a key login for status checks, without sudo. Re-running automatic setup needs root again."),
        );
    }
    Ok(node_id)
}

pub struct ManualOptions {
    pub engine: Engine,
}

/// Manual mode, first run: install harness's key for the login user, offer
/// to replace a root login with harness's own user, then the checklist.
pub async fn run_manual(
    first: Box<dyn Remote>,
    conn: &dyn Connector,
    master: &dyn Master,
    opts: &ManualOptions,
    log: &Log,
) -> Result<Facts> {
    let (id, title) = ("login", "Harness login key");
    log.set(
        id,
        title,
        Status::Running,
        "installing harness's own SSH key",
    );
    let key = master.harness_pubkey();
    must(
        &*first,
        &scripts::install_key(&key),
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
    let f = inspect(&*r).await?;
    log.set(
        id,
        title,
        Status::Ok,
        format!(
            "harness logs in as `{}` with its own key; your password/key was not stored",
            f.login_user
        ),
    );

    if f.is_root_login {
        let (id, title) = ("own_user", "Harness's own user");
        log.ask(
            id,
            title,
            format!(
                "You logged in as root, and harness's key now opens root. For a server with other work on it, \
                 harness should have its own user instead: it can create `{ADMIN_USER}` (no sudo, its own key) \
                 and take its key off root. You run anything that needs root yourself."
            ),
            vec![
                Choice { id: "create", label: "Create harness's own user" },
                Choice { id: "keep", label: "Keep using root" },
            ],
        );
        match master.wait_answer(id).await.as_str() {
            "create" => {
                let res = async {
                    must(
                        &*r,
                        &scripts::admin_user(&key, false),
                        true,
                        "creating the user",
                    )
                    .await?;
                    conn.set_user(ADMIN_USER);
                    conn.connect().await?;
                    must(
                        &*r,
                        &scripts::remove_key(&key),
                        false,
                        "removing harness's key from root",
                    )
                    .await?;
                    Ok::<_, Error>(())
                }
                .await;
                match res {
                    Ok(()) => log.set(
                        id,
                        title,
                        Status::Ok,
                        format!(
                            "harness now logs in as `{ADMIN_USER}`; its key was removed from root"
                        ),
                    ),
                    Err(e) => {
                        conn.set_user("root");
                        log.set(id, title, Status::Failed, format!("{e}; still using root"));
                    }
                }
            }
            _ => log.set(
                id,
                title,
                Status::Warning,
                "keeping root: harness's key opens root on this server",
            ),
        }
    }
    drop(r);
    let r = conn.connect().await?;
    recheck(&*r, master, opts, log).await
}

/// Manual mode: what is in place, and the commands for what is not.
pub fn checklist(
    f: &Facts,
    master_pub: &str,
    node_ip: &str,
    join_hint: &str,
    engine: Engine,
) -> Vec<Step> {
    let step = |id, title, ok: bool, detail: String, script: Option<String>, root| Step {
        script: if ok { None } else { script },
        root,
        ..Step::new(
            id,
            title,
            if ok { Status::Ok } else { Status::Todo },
            detail,
        )
    };
    let os = match f.support() {
        Support::Main => Step::new(
            "os",
            "Operating system",
            Status::Ok,
            format!("{} {}: fully supported", f.os_id, f.os_version),
        ),
        Support::Limited(n) => Step::new("os", "Operating system", Status::Warning, n),
        Support::Unsupported(n) => Step::new("os", "Operating system", Status::Failed, n),
    };
    let (fw_detail, fw_script) = if let Some(c) = f.firewall_conflict() {
        (
            format!(
                "{c}. Keep your firewall and allow WireGuard: UDP {} in, SSH ({}) stays open.",
                scripts::WG_PORT,
                f.ssh_port
            ),
            Some(if f.ufw_active {
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
            }),
        )
    } else {
        (
            format!(
                "No firewall found. Run this; it reverts itself in {}s. Then log in from a NEW terminal and run the confirm part.",
                scripts::REVERT_AFTER_S
            ),
            Some(format!(
                "{}\n# after checking a new SSH login works:\n{}",
                scripts::firewall(f.ssh_port),
                scripts::firewall_confirm()
            )),
        )
    };
    let mut steps = vec![
        os,
        step(
            "root",
            "Root access",
            f.root,
            if f.root {
                "available to harness".into()
            } else {
                "harness has no sudo here: run the commands below yourself with sudo".into()
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
    ];
    if engine != Engine::None {
        let ready = f.engine_ready(engine);
        let mut s = step(
            "engine",
            "Container engine",
            ready,
            if ready {
                format!("ready: {}", engine_note(engine))
            } else {
                format!(
                    "Install it and give the `{RUNTIME_USER}` user access: {}",
                    engine_note(engine)
                )
            },
            Some(scripts::container_engine(engine)),
            true,
        );
        if ready && engine == Engine::Docker {
            s.status = Status::Warning;
        }
        steps.push(s);
    }
    steps.push(step(
        "harness",
        "Harness node",
        f.harness_service,
        if f.harness_service {
            format!("running as `{RUNTIME_USER}`")
        } else {
            format!("Copy the harness binary to /tmp/harness.upload on the server, then run:\n{join_hint}")
        },
        None,
        true,
    ));
    steps
}

/// Manual mode: inspect again and rebuild the checklist. A node WireGuard
/// key that appeared is registered with the master.
pub async fn recheck(
    r: &dyn Remote,
    master: &dyn Master,
    opts: &ManualOptions,
    log: &Log,
) -> Result<Facts> {
    let f = inspect(r).await?;
    let (master_pub, node_ip) = master.wg_plan()?;
    let mut steps = checklist(
        &f,
        &master_pub,
        &node_ip,
        "(the join command is created when this step is reached)",
        opts.engine,
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
            opts.engine,
        ));
    }
    // Keep the login / own-user outcome on top.
    let mut all: Vec<Step> = log
        .snapshot()
        .into_iter()
        .filter(|s| s.id == "login" || s.id == "own_user")
        .collect();
    all.extend(steps);
    log.replace(all);
    Ok(f)
}
