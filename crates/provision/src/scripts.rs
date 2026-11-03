//! The shell scripts a node setup runs (automatic mode) or shows (manual
//! mode). Each is idempotent bash for Debian/Ubuntu with systemd.
//!
//! Anything that could cut the connection (SSH config, firewall) is applied
//! with a **dead-man switch**: a systemd timer reverts it after
//! `REVERT_AFTER_S` unless the master logs in again over a *new* connection
//! and cancels the timer.

/// The provider config copied to a node (only if the user allowed it): the
/// node runs model calls itself, so it needs the key.
#[derive(Debug, Clone)]
pub struct ProviderFiles {
    pub harness_toml: String,
    /// `[keys]` with the API key, mode 0600 on the node.
    pub secrets_toml: Option<String>,
}

/// Seconds before an unconfirmed risky change reverts itself.
pub const REVERT_AFTER_S: u32 = 120;

/// WireGuard port on nodes (UDP).
pub const WG_PORT: u16 = 51820;

const HEAD: &str = "set -euo pipefail\nexport DEBIAN_FRONTEND=noninteractive\n";

/// Append a public key to the login user's authorized_keys (no root).
pub fn install_key(pubkey: &str) -> String {
    format!(
        "set -eu\numask 077\nmkdir -p ~/.ssh\ntouch ~/.ssh/authorized_keys\n\
         grep -qxF '{k}' ~/.ssh/authorized_keys || echo '{k}' >> ~/.ssh/authorized_keys\n",
        k = pubkey.replace('\'', "")
    )
}

pub fn packages() -> String {
    format!("{HEAD}apt-get update -q\napt-get install -y -q wireguard-tools nftables\n")
}

/// Key-only SSH as a drop-in (first match wins in sshd, so `10-` beats a
/// cloud-init `50-...` that turns passwords on). Validated with `sshd -t`
/// before reload; reverts itself unless confirmed.
pub fn ssh_harden() -> String {
    format!(
        r#"{HEAD}conf=/etc/ssh/sshd_config.d/10-harness.conf
grep -qE '^\s*Include\s+/etc/ssh/sshd_config.d/\*\.conf' /etc/ssh/sshd_config || sed -i '1i Include /etc/ssh/sshd_config.d/*.conf' /etc/ssh/sshd_config
mkdir -p /etc/ssh/sshd_config.d
cat > "$conf" <<'CONF'
# Written by harness: key-only logins.
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin prohibit-password
CONF
if ! sshd -t; then rm -f "$conf"; echo "sshd rejected the new config; nothing changed" >&2; exit 1; fi
reload() {{ systemctl reload ssh 2>/dev/null || systemctl reload sshd; }}
systemctl stop harness-revert-ssh.timer 2>/dev/null || true
systemd-run --quiet --unit=harness-revert-ssh --on-active={REVERT_AFTER_S} /bin/sh -c "rm -f $conf; systemctl reload ssh 2>/dev/null || systemctl reload sshd"
reload
echo "applied; reverts in {REVERT_AFTER_S}s unless confirmed"
"#
    )
}

pub fn ssh_confirm() -> String {
    "systemctl stop harness-revert-ssh.timer 2>/dev/null || true\necho confirmed\n".into()
}

/// The node's firewall: our own nftables table, inbound limited to SSH and
/// WireGuard (outbound open: agents need the internet).
pub fn firewall_rules(ssh_port: u16) -> String {
    format!(
        r#"table inet harness {{
  chain input {{
    type filter hook input priority 0; policy drop;
    iif "lo" accept
    ct state established,related accept
    ct state invalid drop
    meta l4proto icmp accept
    meta l4proto ipv6-icmp accept
    tcp dport {ssh_port} accept
    udp dport {WG_PORT} accept
  }}
}}
"#
    )
}

pub fn firewall(ssh_port: u16) -> String {
    format!(
        r#"{HEAD}mkdir -p /etc/harness
cat > /etc/harness/firewall.nft <<'NFT'
{rules}NFT
nft -c -f /etc/harness/firewall.nft
nft delete table inet harness 2>/dev/null || true
systemctl stop harness-revert-fw.timer 2>/dev/null || true
systemd-run --quiet --unit=harness-revert-fw --on-active={REVERT_AFTER_S} /usr/sbin/nft delete table inet harness
nft -f /etc/harness/firewall.nft
echo "applied; reverts in {REVERT_AFTER_S}s unless confirmed"
"#,
        rules = firewall_rules(ssh_port)
    )
}

/// Cancel the revert and load the rules at every boot.
pub fn firewall_confirm() -> String {
    format!(
        r#"{HEAD}systemctl stop harness-revert-fw.timer 2>/dev/null || true
cat > /etc/systemd/system/harness-firewall.service <<'UNIT'
[Unit]
Description=harness node firewall
Before=network-pre.target
Wants=network-pre.target

[Service]
Type=oneshot
ExecStart=/usr/sbin/nft -f /etc/harness/firewall.nft
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable harness-firewall.service
echo confirmed
"#
    )
}

/// WireGuard on the node. The node's private key is generated *on the node*
/// and never leaves it; the script prints `NODE_PUBKEY=...`.
pub fn wireguard(node_ip: &str, master_ip: &str, master_pubkey: &str) -> String {
    format!(
        r#"{HEAD}umask 077
mkdir -p /etc/wireguard
[ -s /etc/wireguard/harness.key ] || wg genkey > /etc/wireguard/harness.key
cat > /etc/wireguard/wg-harness.conf <<CONF
[Interface]
Address = {node_ip}/24
ListenPort = {WG_PORT}
PostUp = wg set %i private-key /etc/wireguard/harness.key

[Peer]
# the harness master
PublicKey = {master_pubkey}
AllowedIPs = {master_ip}/32
CONF
systemctl enable wg-quick@wg-harness >/dev/null 2>&1
systemctl restart wg-quick@wg-harness
echo "NODE_PUBKEY=$(wg pubkey < /etc/wireguard/harness.key)"
"#
    )
}

/// Dedicated `harness` user, the binary (uploaded to `/tmp/harness.upload`),
/// node policy, optional provider config, join, and a systemd service.
pub fn node_service(join_cmd_args: &str, provider: Option<&ProviderFiles>) -> String {
    let file = |name: &str, body: &str| {
        format!(
            "cat > /var/lib/harness/{name} <<'TOML'\n{body}TOML\nchown harness: /var/lib/harness/{name}\nchmod 600 /var/lib/harness/{name}\n"
        )
    };
    let provider = match provider {
        Some(p) => {
            let mut s = file("harness.toml", &p.harness_toml);
            if let Some(sec) = &p.secrets_toml {
                s.push_str(&file("secrets.toml", sec));
            }
            s
        }
        None => String::new(),
    };
    format!(
        r#"{HEAD}id harness >/dev/null 2>&1 || useradd --system --create-home --home-dir /home/harness --shell /usr/sbin/nologin harness
install -m 0755 /tmp/harness.upload /usr/local/bin/harness
rm -f /tmp/harness.upload
install -d -o harness -g harness -m 0700 /var/lib/harness /home/harness/work
cat > /var/lib/harness/node.toml <<'TOML'
# This node decides what the master may do here.
max_approval_mode = "ask"
workspace_roots = ["/home/harness/work"]
TOML
chown harness: /var/lib/harness/node.toml
{provider}[ -f /var/lib/harness/remote.toml ] || runuser -u harness -- /usr/local/bin/harness --data-dir /var/lib/harness node join {join_cmd_args}
cat > /etc/systemd/system/harness-node.service <<'UNIT'
[Unit]
Description=harness worker node
After=network-online.target wg-quick@wg-harness.service
Wants=network-online.target

[Service]
User=harness
ExecStart=/usr/local/bin/harness --data-dir /var/lib/harness node run
Restart=always
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/harness /home/harness

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now harness-node
echo installed
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash_ok(s: &str) {
        let out = std::process::Command::new("bash")
            .args(["-n", "-c", s])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{s}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn scripts_parse_and_carry_the_safety_nets() {
        for s in [
            install_key("ssh-ed25519 AAAA harness"),
            packages(),
            ssh_harden(),
            ssh_confirm(),
            firewall(22),
            firewall_confirm(),
            wireguard("10.77.0.2", "10.77.0.1", "pubkey="),
            node_service(
                "wss://10.77.0.1:7443 --token t --fingerprint f",
                Some(&ProviderFiles {
                    harness_toml: "[provider]\nkind = \"mock\"\n".into(),
                    secrets_toml: Some("[keys]\nopenai = \"k\"\n".into()),
                }),
            ),
        ] {
            bash_ok(&s);
        }
        let h = ssh_harden();
        assert!(
            h.contains("sshd -t")
                && h.contains("--on-active=120")
                && h.contains("PasswordAuthentication no")
        );
        let f = firewall(2222);
        assert!(
            f.contains("nft -c -f")
                && f.contains("tcp dport 2222 accept")
                && f.contains("--on-active=120")
        );
        assert!(
            f.find("systemd-run").unwrap()
                < f.find("nft -f /etc/harness/firewall.nft\necho").unwrap(),
            "revert armed before applying"
        );
        // Single quotes cannot break out of the authorized_keys line.
        assert!(!install_key("ssh-ed25519 AAAA x'; rm -rf / #").contains("x';"));
        assert!(wireguard("a", "b", "c").contains("wg genkey"));
    }

    #[test]
    fn firewall_rules_are_valid_nftables() {
        // Checked with nft itself when it is installed (no root needed for -c on most systems).
        let Ok(out) = std::process::Command::new("nft").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fw.nft");
        std::fs::write(&p, firewall_rules(22)).unwrap();
        let out = std::process::Command::new("nft")
            .args(["-c", "-f"])
            .arg(&p)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        // Without CAP_NET_ADMIN or a netlink socket, nft cannot check; only syntax errors count.
        assert!(
            out.status.success() || !err.contains("syntax error"),
            "{err}"
        );
    }
}
