//! The setup runner against a scripted fake server.

use std::sync::{Arc, Mutex};

use harness_provision::setup::*;
use harness_provision::ssh::Output;
use harness_provision::wg::Peer;
use harness_provision::{Error, Result};

const FRESH: &str = "os_id=debian\nos_version=12\narch=x86_64\nglibc=2.36\nroot=yes\nsystemd=yes\n\
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
        Ok(Box::new(Srv {
            w: self.0.clone(),
            kind: "key",
        }))
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
    fn provider_toml(&self) -> Option<String> {
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
    async fn wait_continue(&self) {}
}

fn world(inspect: &str) -> Arc<Mutex<World>> {
    Arc::new(Mutex::new(World {
        inspect: inspect.into(),
        ..Default::default()
    }))
}

async fn auto(w: &Arc<Mutex<World>>, log: &Log) -> Result<String> {
    let first = Box::new(Srv {
        w: w.clone(),
        kind: "password",
    });
    let (conn, m) = (Conn(w.clone()), M(w.clone()));
    let r = bootstrap(first, &conn, &m, log).await?;
    let opts = AutoOptions {
        name: "vps".into(),
        endpoint_host: "203.0.113.7".into(),
        own_pubkey: Some("ssh-ed25519 AAAAuser me".into()),
    };
    run_auto(r, &conn, &m, &opts, log).await
}

#[tokio::test]
async fn automatic_setup_runs_every_step_in_order() {
    let w = world(FRESH);
    let log = Log::default();
    assert_eq!(auto(&w, &log).await.unwrap(), "vps-1");
    let w = w.lock().unwrap();
    // The password connection only ever installed harness's key.
    let pw: Vec<_> = w.ran.iter().filter(|r| r.0 == "password").collect();
    assert_eq!(pw.len(), 1);
    assert!(pw[0].1.contains("AAAAharness") && !pw[0].2);
    let order: Vec<&str> = w
        .ran
        .iter()
        .filter_map(|(_, s, _)| {
            [
                ("os-release", "inspect"),
                ("apt-get install", "packages"),
                ("AAAAuser", "own-key"),
                ("10-harness.conf\ngrep", "ssh"),
                (
                    "harness-revert-ssh.timer 2>/dev/null || true\necho confirmed",
                    "ssh-confirm",
                ),
                ("firewall.nft <<", "firewall"),
                ("harness-firewall.service <<", "fw-confirm"),
                ("wg genkey", "wireguard"),
                ("node join", "install"),
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
            "packages",
            "own-key",
            "ssh",
            "ssh-confirm",
            "firewall",
            "fw-confirm",
            "wireguard",
            "install"
        ]
    );
    assert_eq!(w.uploads, [("/tmp/harness.upload".to_string(), 1000)]);
    assert_eq!(w.peers[0].endpoint, "203.0.113.7:51820");
    assert_eq!(w.peers[0].public_key, "nodepub=");
    assert!(
        log.snapshot().iter().all(|s| s.status == Status::Ok),
        "{:?}",
        log.snapshot()
    );
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

#[tokio::test]
async fn manual_mode_only_inspects_and_explains() {
    let w = world(
        &FRESH
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
        &log,
    )
    .await
    .unwrap();
    let steps = log.snapshot();
    let get = |id: &str| steps.iter().find(|s| s.id == id).unwrap().clone();
    assert_eq!(get("login").status, Status::Ok);
    assert_eq!(get("ssh").status, Status::Ok);
    let fw = get("firewall");
    assert_eq!(fw.status, Status::Todo);
    assert!(
        fw.script.unwrap().contains("ufw allow 51820/udp"),
        "works with their firewall"
    );
    assert!(get("wireguard").script.unwrap().contains("wg genkey"));
    assert!(get("harness").script.unwrap().contains("--token T"));
    // Only the key install and read-only inspection ran on the server.
    let ran: Vec<_> = w.lock().unwrap().ran.iter().map(|r| r.1.clone()).collect();
    assert_eq!(ran.len(), 2);
    assert!(ran[0].contains("authorized_keys") && ran[1].contains("os-release"));
}
