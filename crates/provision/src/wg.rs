//! The master's side of the WireGuard tunnels: its key, the addresses
//! handed to nodes, and the config file the user brings up once with root
//! (`sudo wg-quick up`). Each node listens on UDP 51820; the master dials
//! out to it, so the master can sit behind NAT.

use std::path::Path;

use aws_lc_rs::agreement::{PrivateKey, X25519};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The tunnel network; the master is `.1`.
pub const NET: &str = "10.77.0";
pub const MASTER_IP: &str = "10.77.0.1";
pub const IFACE: &str = "rizzy-harness";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub public_key: String,
    /// `ip:port` of the node.
    pub endpoint: String,
    /// The node's tunnel address.
    pub ip: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MasterWg {
    /// Base64 private key (file mode 0600).
    private_key: String,
    pub public_key: String,
    pub peers: Vec<Peer>,
}

/// `(private, public)` as WireGuard prints them (base64 of 32 bytes).
pub fn keypair() -> (String, String) {
    let mut sk = [0u8; 32];
    getrandom::fill(&mut sk).expect("OS randomness");
    // Clamping as WireGuard does (x25519 clamps internally too).
    sk[0] &= 248;
    sk[31] = (sk[31] & 127) | 64;
    let key = PrivateKey::from_private_key(&X25519, &sk).expect("32 bytes is a valid x25519 key");
    let pk = key.compute_public_key().expect("x25519 public key");
    (B64.encode(sk), B64.encode(pk.as_ref()))
}

impl MasterWg {
    /// Load `<dir>/wg.json`, creating the master's key on first use.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let p = dir.join("wg.json");
        if let Ok(s) = std::fs::read_to_string(&p) {
            return serde_json::from_str(&s)
                .map_err(|e| Error::Step(format!("{}: {e}", p.display())));
        }
        let (private_key, public_key) = keypair();
        let me = Self {
            private_key,
            public_key,
            peers: vec![],
        };
        me.save(dir)?;
        Ok(me)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_private(
            &dir.join("wg.json"),
            serde_json::to_string_pretty(self)
                .expect("serializes")
                .as_bytes(),
        )?;
        write_private(&dir.join(format!("{IFACE}.conf")), self.config().as_bytes())
    }

    /// The tunnel address for `name` (reused if it already has one).
    pub fn address_for(&self, name: &str) -> Result<String> {
        if let Some(p) = self.peers.iter().find(|p| p.name == name) {
            return Ok(p.ip.clone());
        }
        (2..=254)
            .map(|i| format!("{NET}.{i}"))
            .find(|ip| !self.peers.iter().any(|p| &p.ip == ip))
            .ok_or_else(|| Error::Step("the tunnel network is full (253 nodes)".into()))
    }

    pub fn upsert(&mut self, peer: Peer) {
        self.peers.retain(|p| p.name != peer.name);
        self.peers.push(peer);
    }

    /// `wg-quick` config for the master.
    pub fn config(&self) -> String {
        let mut s = format!(
            "# Written by harness. Bring up once with: sudo wg-quick up <this file>\n\
             [Interface]\nAddress = {MASTER_IP}/24\nPrivateKey = {}\n",
            self.private_key
        );
        for p in &self.peers {
            s.push_str(&format!(
                "\n[Peer]\n# {}\nPublicKey = {}\nEndpoint = {}\nAllowedIPs = {}/32\nPersistentKeepalive = 25\n",
                p.name, p.public_key, p.endpoint, p.ip
            ));
        }
        s
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| Error::Step(e.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o.open(&tmp).map_err(|e| Error::Step(e.to_string()))?;
    f.write_all(data).map_err(|e| Error::Step(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::Step(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_wireguard_tools() {
        let (sk, pk) = keypair();
        assert_eq!(B64.decode(&sk).unwrap().len(), 32);
        // When `wg` is installed, it must derive the same public key.
        if let Ok(mut child) = std::process::Command::new("wg")
            .arg("pubkey")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("{sk}\n").as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pk);
        }
    }

    #[test]
    fn addresses_and_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = MasterWg::load_or_create(dir.path()).unwrap();
        assert_eq!(m.address_for("a").unwrap(), "10.77.0.2");
        m.upsert(Peer {
            name: "a".into(),
            public_key: "PK".into(),
            endpoint: "203.0.113.7:51820".into(),
            ip: "10.77.0.2".into(),
        });
        assert_eq!(m.address_for("a").unwrap(), "10.77.0.2");
        assert_eq!(m.address_for("b").unwrap(), "10.77.0.3");
        m.save(dir.path()).unwrap();
        let again = MasterWg::load_or_create(dir.path()).unwrap();
        assert_eq!(again.public_key, m.public_key);
        let conf = std::fs::read_to_string(dir.path().join("rizzy-harness.conf")).unwrap();
        assert!(
            conf.contains("Endpoint = 203.0.113.7:51820")
                && conf.contains("PersistentKeepalive = 25")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in ["wg.json", "rizzy-harness.conf"] {
                assert_eq!(
                    std::fs::metadata(dir.path().join(f))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
    }
}
