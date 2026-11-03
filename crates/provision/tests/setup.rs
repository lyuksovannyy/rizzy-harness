//! The setup runner against a scripted fake server.

use std::sync::{Arc, Mutex};

use harness_provision::setup::*;
use harness_provision::ssh::Output;
use harness_provision::wg::Peer;
use harness_provision::{Error, Result};

const FRESH: &str = "os_id=debian\nos_version=13\nos_like=\nlogin_user=root\nuid=0\narch=x86_64\nglibc=2.41\nroot=yes\nsystemd=yes\n\
wg_tools=no\nwg_harness=down\nnft=yes\nharness_fw=no\nufw=inactive\nfirewalld=inactive\ndocker=no\n\
other_fw_rules=0\nssh_password=yes\nssh_root_login=yes\nssh_port=22\nharness=none\nharness_service=inactive\npublic_ip=203.0.113.7\n";

#[derive(Default)]
struct World {
    /// (connection kind, script, root)
    ran: Vec<(&'static str, String, bool)>,
    uploads: Vec<(String, usize)>,
    inspect: String,
    /// New key logins fail once a script containing this ran.
    lock_after: Option<&'static str>,
    locked: bool,
    peers: Vec<Peer>,
    /// The user new key connections log in as.
    user: String,
    answers: Vec<&'static str>,
}

#[derive(Clone)]
struct Srv {
    w: Arc<Mutex<World>>,
    kind: &'static str,
}

#[async_trait::async_trait]
impl Remote for Srv {
    async fn run(&self, script: &str, root: bool) -> Result<Output> {
        let mut w = self.w.lock().unwrap();
        w.ran.push((self.kind, script.to_string(), root));
        if let Some(m) = w.lock_after
            && script.contains(m)
        {
            w.locked = true;
        }
        let stdout = if script.contains("os-release") {
            w.inspect.clone()
        } else if script.contains("wg genkey") {
            "NODE_PUBKEY=nodepub=\n".into()
        } else if script.contains("node join") {
            "joined as node `vps-1`. Start it with: harness node run\ninstalled\n".into()
        } else {
            String::new()
        };
        Ok(Output {
            code: 0,
            stdout,
            stderr: String::new(),
        })
    }
    async fn upload(&self, path: &str, data: &[u8]) -> Result<()> {
        self.w
            .lock()
            .unwrap()
            .uploads
            .push((path.into(), data.len()));
        Ok(())
    }
}

struct Conn(Arc<Mutex<World>>);

#[async_trait::async_trait]
impl Connector for Conn {
    async fn connect(&self) -> Result<Box<dyn Remote>> {
        if self.0.lock().unwrap().locked {
            return Err(Error::Ssh("connection refused".into()));
        }
        let kind = if self.0.lock().unwrap().user == "harness-admin" {
            "admin"
        } else {
            "key"
        };
        Ok(Box::new(Srv {
            w: self.0.clone(),
            kind,
        }))
    }
    fn set_user(&self, user: &str) {
        self.0.lock().unwrap().user = user.into();
    }
}

struct M(Arc<Mutex<World>>);

#[async_trait::async_trait]
impl Master for M {
    fn harness_pubkey(&self) -> String {
        "ssh-ed25519 AAAAharness harness".into()
    }
    fn wg_plan(&self) -> Result<(String, String)> {
        Ok(("masterpub=".into(), "10.77.0.2".into()))
    }
    fn wg_add_peer(&self, p: Peer) -> Result<String> {
        self.0.lock().unwrap().peers.push(p);
        Ok("sudo wg-quick up /data/keys/wg-harness.conf".into())
    }
    async fn join_args(&self) -> Result<String> {
        Ok("wss://10.77.0.1:7443 --token T --fingerprint F".into())
    }
    fn provider_files(&self) -> Option<harness_provision::scripts::ProviderFiles> {
        None
    }
    fn binary(&self, _arch: &str) -> Result<Vec<u8>> {
        Ok(vec![0u8; 1000])
    }
    fn platform(&self) -> (String, String) {
        ("x86_64".into(), "2.35".into())
    }
    async fn wait_online(&self, _name: &str) -> Result<()> {
        Ok(())
    }
    async fn wait_answer(&self, _step: &str) -> String {
        let mut w = self.0.lock().unwrap();
        if w.answers.is_empty() {
            "continue".into()
        } else {
            w.answers.remove(0).into()
        }
    }
}

fn world(inspect: &str) -> Arc<Mutex<World>> {
    Arc::new(Mutex::new(World {
        inspect: inspect.into(),
        ..Default::default()
    }))
}

async fn auto_with(
    w: &Arc<Mutex<World>>,
    log: &Log,
    engine: harness_provision::facts::Engine,
    keep: bool,
) -> Result<String> {
    let first = Box::new(Srv {
        w: w.clone(),
        kind: "password",
    });
    let (conn, m) = (Conn(w.clone()), M(w.clone()));
    let opts = AutoOptions {
        name: "vps".into(),
        endpoint_host: "203.0.113.7".into(),
        own_pubkey: Some("ssh-ed25519 AAAAuser me".into()),
        engine,
        keep_admin_sudo: keep,
    };
    run_auto(first, &conn, &m, &opts, log).await
}

async fn auto(w: &Arc<Mutex<World>>, log: &Log) -> Result<String> {
    auto_with(w, log, harness_provision::facts::Engine::Podman, false).await
}

#[tokio::test]
async fn automatic_setup_runs_every_step_in_order() {
    let w = world(FRESH);
    let log = Log::default();
    assert_eq!(auto(&w, &log).await.unwrap(), "vps-1");
    let w = w.lock().unwrap();
    // The root (password) connection only inspected, created harness-admin
    // and added the user's own key to root.
    let pw: Vec<_> = w
        .ran
        .iter()
        .filter(|r| r.0 == "password")
        .map(|r| r.1.clone())
        .collect();
    assert_eq!(pw.len(), 3, "{pw:#?}");
    assert!(pw[0].contains("os-release"));
    assert!(
        pw[1].contains("useradd --create-home --shell /bin/bash harness-admin")
            && pw[1].contains("NOPASSWD")
            && pw[1].contains("AAAAharness")
    );
    assert!(pw[2].contains("AAAAuser"));
    // Everything after that ran as harness-admin.
    assert!(
        w.ran.iter().skip(3).all(|r| r.0 == "admin"),
        "{:?}",
        w.ran.iter().map(|r| r.0).collect::<Vec<_>>()
    );
    let order: Vec<&str> = w
        .ran
        .iter()
        .filter_map(|(_, s, _)| {
            [
                ("os-release", "inspect"),
                ("useradd --create-home --shell /bin/bash", "admin"),
                ("AAAAuser", "own-key"),
                ("apt-get install -y -q wireguard-tools", "packages"),
                ("10-harness.conf\ngrep", "ssh"),
                (
                    "harness-revert-ssh.timer 2>/dev/null || true\necho confirmed",
                    "ssh-confirm",
                ),
                ("firewall.nft <<", "firewall"),
                ("harness-firewall.service <<", "fw-confirm"),
                ("enable-linger", "podman"),
                ("wg genkey", "wireguard"),
                ("node join", "install"),
                ("sudo removed", "drop-sudo"),
            ]
            .iter()
            .find(|(m, _)| s.contains(m))
            .map(|(_, n)| *n)
        })
        .collect();
    assert_eq!(
        order,
        [
            "inspect",
            "admin",
            "own-key",
            "packages",
            "ssh",
            "ssh-confirm",
            "firewall",
            "fw-confirm",
            "podman",
            "wireguard",
            "install",
            "drop-sudo"
        ]
    );
    assert_eq!(w.uploads, [("/tmp/harness.upload".to_string(), 1000)]);
    assert_eq!(w.peers[0].endpoint, "203.0.113.7:51820");
    assert!(
        log.snapshot().iter().all(|s| s.status == Status::Ok),
        "{:#?}",
        log.snapshot()
    );
}

#[tokio::test]
async fn automatic_setup_needs_root() {
    let w = world(&FRESH.replace("login_user=root\nuid=0", "login_user=deploy\nuid=1000"));
    let log = Log::default();
    let e = auto(&w, &log).await.unwrap_err().to_string();
    assert!(
        e.contains("needs the root login") && e.contains("`deploy`"),
        "{e}"
    );
    assert_eq!(w.lock().unwrap().ran.len(), 1, "only inspected");
}

#[tokio::test]
async fn docker_and_keeping_sudo_are_flagged() {
    let w = world(FRESH);
    let log = Log::default();
    auto_with(&w, &log, harness_provision::facts::Engine::Docker, true)
        .await
        .unwrap();
    let steps = log.snapshot();
    let get = |id: &str| steps.iter().find(|s| s.id == id).unwrap().clone();
    assert_eq!(get("engine").status, Status::Warning);
    assert!(get("engine").detail.contains("effectively root"));
    assert_eq!(get("lockdown").status, Status::Warning);
    let w = w.lock().unwrap();
    assert!(
        w.ran
            .iter()
            .any(|r| r.1.contains("usermod -aG docker harness"))
    );
    assert!(
        !w.ran.iter().any(|r| r.1.contains("sudo removed")),
        "sudo kept on request"
    );
}

#[tokio::test]
async fn distro_tiers_in_automatic_mode() {
    // Older Ubuntu: runs, with a warning.
    let w = world(&FRESH.replace(
        "os_id=debian\nos_version=13",
        "os_id=ubuntu\nos_version=24.04",
    ));
    let log = Log::default();
    auto(&w, &log).await.unwrap();
    assert_eq!(log.snapshot()[0].status, Status::Warning);
    // Not Debian-based: refused before any change.
    let w = world(&FRESH.replace(
        "os_id=debian\nos_version=13\nos_like=",
        "os_id=fedora\nos_version=43\nos_like=",
    ));
    let e = auto(&w, &Log::default()).await.unwrap_err().to_string();
    assert!(e.contains("not Debian-based"), "{e}");
    assert_eq!(w.lock().unwrap().ran.len(), 1);
}

#[tokio::test]
async fn lockout_stops_before_confirming_so_the_change_reverts() {
    let w = world(FRESH);
    w.lock().unwrap().lock_after = Some("firewall.nft <<");
    let log = Log::default();
    let e = auto(&w, &log).await.unwrap_err().to_string();
    assert!(e.contains("reverts itself within 120s"), "{e}");
    let w = w.lock().unwrap();
    assert!(
        !w.ran
            .iter()
            .any(|r| r.1.contains("harness-firewall.service <<")),
        "never confirmed"
    );
    assert!(!w.ran.iter().any(|r| r.1.contains("wg genkey")), "stopped");
    let fw = log
        .snapshot()
        .into_iter()
        .find(|s| s.id == "firewall")
        .unwrap();
    assert_eq!(fw.status, Status::Failed);
}

#[tokio::test]
async fn existing_servers_are_refused_by_automatic_mode() {
    let w = world(
        &FRESH
            .replace("docker=no", "docker=yes")
            .replace("ufw=inactive", "ufw=active"),
    );
    let log = Log::default();
    let e = auto(&w, &log).await.unwrap_err().to_string();
    assert!(
        e.contains("ufw is active") && e.contains("use manual setup"),
        "{e}"
    );
    assert!(
        !w.lock()
            .unwrap()
            .ran
            .iter()
            .any(|r| r.1.contains("apt-get")),
        "nothing changed"
    );
}

fn manual_opts(engine: harness_provision::facts::Engine) -> ManualOptions {
    ManualOptions { engine }
}

#[tokio::test]
async fn manual_mode_only_inspects_and_explains() {
    let w = world(
        &FRESH
            .replace("login_user=root\nuid=0", "login_user=deploy\nuid=1000")
            .replace("ufw=inactive", "ufw=active")
            .replace("ssh_password=yes", "ssh_password=no"),
    );
    let (conn, m) = (Conn(w.clone()), M(w.clone()));
    let log = Log::default();
    run_manual(
        Box::new(Srv {
            w: w.clone(),
            kind: "password",
        }),
        &conn,
        &m,
        &manual_opts(harness_provision::facts::Engine::Docker),
        &log,
    )
    .await
    .unwrap();
    let steps = log.snapshot();
    let get = |id: &str| steps.iter().find(|s| s.id == id).unwrap().clone();
    assert!(
        steps.iter().all(|s| s.id != "own_user"),
        "no question for a normal user"
    );
    assert_eq!(get("login").status, Status::Ok);
    assert_eq!(get("os").status, Status::Ok);
    assert_eq!(get("ssh").status, Status::Ok);
    assert!(
        get("firewall")
            .script
            .unwrap()
            .contains("ufw allow 51820/udp")
    );
    let engine = get("engine");
    assert_eq!(engine.status, Status::Todo);
    assert!(
        engine
            .script
            .unwrap()
            .contains("usermod -aG docker harness")
            && engine.detail.contains("effectively root")
    );
    assert!(get("harness").script.unwrap().contains("--token T"));
    // Only the key install and read-only inspection ran on the server.
    let ran: Vec<_> = w.lock().unwrap().ran.iter().map(|r| r.1.clone()).collect();
    assert_eq!(ran.len(), 3, "key, inspect, re-inspect");
    assert!(ran[0].contains("authorized_keys") && ran[1].contains("os-release"));
}

#[tokio::test]
async fn manual_root_login_offers_harness_own_user() {
    // Answer "create": harness-admin without sudo, key removed from root.
    let w = world(FRESH);
    w.lock().unwrap().answers = vec!["create"];
    let (conn, m) = (Conn(w.clone()), M(w.clone()));
    let log = Log::default();
    run_manual(
        Box::new(Srv {
            w: w.clone(),
            kind: "password",
        }),
        &conn,
        &m,
        &manual_opts(harness_provision::facts::Engine::Podman),
        &log,
    )
    .await
    .unwrap();
    let own = log
        .snapshot()
        .into_iter()
        .find(|s| s.id == "own_user")
        .unwrap();
    assert_eq!(own.status, Status::Ok, "{own:?}");
    let ran: Vec<_> = w
        .lock()
        .unwrap()
        .ran
        .iter()
        .map(|r| (r.0, r.1.clone()))
        .collect();
    let admin = ran
        .iter()
        .find(|r| {
            r.1.contains("useradd --create-home --shell /bin/bash harness-admin")
        })
        .unwrap();
    assert!(
        admin.1.contains("rm -f /etc/sudoers.d/harness-admin") && !admin.1.contains("NOPASSWD"),
        "no sudo in manual mode"
    );
    assert!(
        ran.iter()
            .any(|r| r.1.contains("grep -vxF 'ssh-ed25519 AAAAharness harness'")),
        "key removed from root"
    );
    assert_eq!(ran.last().unwrap().0, "admin", "re-check as harness-admin");

    // Answer "keep": stays root, flagged.
    let w = world(FRESH);
    w.lock().unwrap().answers = vec!["keep"];
    let (conn, m) = (Conn(w.clone()), M(w.clone()));
    let log = Log::default();
    run_manual(
        Box::new(Srv {
            w: w.clone(),
            kind: "password",
        }),
        &conn,
        &m,
        &manual_opts(harness_provision::facts::Engine::None),
        &log,
    )
    .await
    .unwrap();
    let own = log
        .snapshot()
        .into_iter()
        .find(|s| s.id == "own_user")
        .unwrap();
    assert_eq!(own.status, Status::Warning);
    assert!(
        !w.lock()
            .unwrap()
            .ran
            .iter()
            .any(|r| r.1.contains("useradd"))
    );
    assert!(
        log.snapshot().iter().all(|s| s.id != "engine"),
        "no engine step when none chosen"
    );
}
