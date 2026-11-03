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

/// Harness's own login user for setup and status checks.
pub const ADMIN_USER: &str = "harness-admin";
/// The unprivileged user the node runs as (agents' commands run as it).
pub const RUNTIME_USER: &str = "harness";

/// Create `harness-admin` with harness's key (locked password, key login
/// only). With `sudo`, passwordless sudo for the setup; `drop_admin_sudo`
/// removes it again.
pub fn admin_user(pubkey: &str, sudo: bool) -> String {
    let sudo = if sudo {
        format!(
            "echo '{ADMIN_USER} ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/{ADMIN_USER}\nchmod 440 /etc/sudoers.d/{ADMIN_USER}\nvisudo -cf /etc/sudoers.d/{ADMIN_USER}\n"
        )
    } else {
        format!("rm -f /etc/sudoers.d/{ADMIN_USER}\n")
    };
    format!(
        r#"{HEAD}id {ADMIN_USER} >/dev/null 2>&1 || useradd --create-home --shell /bin/bash {ADMIN_USER}
passwd -l {ADMIN_USER} >/dev/null
{sudo}install -d -o {ADMIN_USER} -g {ADMIN_USER} -m 0700 /home/{ADMIN_USER}/.ssh
f=/home/{ADMIN_USER}/.ssh/authorized_keys
touch "$f"
grep -qxF '{k}' "$f" || echo '{k}' >> "$f"
chown {ADMIN_USER}: "$f"
chmod 600 "$f"
echo "admin user ready"
"#,
        k = pubkey.replace('\'', "")
    )
}

/// Remove a key from the login user's authorized_keys (e.g. harness's key
/// from root once `harness-admin` works).
pub fn remove_key(pubkey: &str) -> String {
    let k = pubkey.replace('\'', "");
    format!(
        "set -eu\nf=~/.ssh/authorized_keys\n[ -f \"$f\" ] || exit 0\ngrep -vxF '{k}' \"$f\" > \"$f.tmp\" || true\ncat \"$f.tmp\" > \"$f\"\nrm -f \"$f.tmp\"\n"
    )
}

/// After automatic setup: harness keeps a key login for status checks but
/// loses root. A compromised master must not be root on every node.
pub fn drop_admin_sudo() -> String {
    format!("rm -f /etc/sudoers.d/{ADMIN_USER}\necho \"sudo removed\"\n")
}

/// The runtime user: a normal (not system) user so it gets subordinate
/// uid/gid ranges for rootless containers; no password, no login shell.
fn runtime_user() -> String {
    format!(
        r#"if ! id {RUNTIME_USER} >/dev/null 2>&1; then
  useradd --create-home --home-dir /home/{RUNTIME_USER} --shell /usr/sbin/nologin {RUNTIME_USER}
  passwd -l {RUNTIME_USER} >/dev/null
fi
"#
    )
}

/// Install the container engine and give the runtime user access to it.
/// Podman: rootless (own subuid/subgid range, lingering user session, no
/// group). Docker: the distribution's `docker.io` and the `docker` group,
/// which is root-equivalent.
pub fn container_engine(engine: crate::facts::Engine) -> String {
    use crate::facts::Engine;
    let user = runtime_user();
    match engine {
        Engine::None => String::new(),
        Engine::Podman => format!(
            r#"{HEAD}apt-get update -q
apt-get install -y -q podman uidmap
{user}if ! grep -q '^{RUNTIME_USER}:' /etc/subuid; then
  start=$(awk -F: 'BEGIN{{m=100000}} {{e=$2+$3; if (e>m) m=e}} END{{print m}}' /etc/subuid /etc/subgid 2>/dev/null)
  usermod --add-subuids "$start-$((start+65535))" --add-subgids "$start-$((start+65535))" {RUNTIME_USER}
fi
loginctl enable-linger {RUNTIME_USER}
uid=$(id -u {RUNTIME_USER})
for _ in $(seq 1 20); do [ -d /run/user/$uid ] && break; sleep 0.5; done
runuser -u {RUNTIME_USER} -- env XDG_RUNTIME_DIR=/run/user/$uid podman info --format '{{{{.Host.Security.Rootless}}}}' | grep -qx true
echo "podman (rootless) ready for {RUNTIME_USER}"
"#
        ),
        Engine::Docker => format!(
            r#"{HEAD}if ! command -v docker >/dev/null; then
  apt-get update -q
  apt-get install -y -q docker.io
fi
systemctl enable --now docker
{user}usermod -aG docker {RUNTIME_USER}
runuser -u {RUNTIME_USER} -g docker -- docker info >/dev/null
echo "docker ready for {RUNTIME_USER} (docker group)"
"#
        ),
    }
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
pub fn node_service(
    join_cmd_args: &str,
    provider: Option<&ProviderFiles>,
    engine: crate::facts::Engine,
) -> String {
    use crate::facts::Engine;
    let file = |name: &str, body: &str| {
        format!(
            "cat > /var/lib/harness/{name} <<'TOML'\n{body}TOML\nchown {RUNTIME_USER}: /var/lib/harness/{name}\nchmod 600 /var/lib/harness/{name}\n"
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
    // Rootless podman runs setuid newuidmap and needs the user's runtime
    // dir, so its unit cannot use NoNewPrivileges.
    let (hardening, env) = match engine {
        Engine::Podman => (
            "ReadWritePaths=/var/lib/harness /home/harness /run/user/RUNTIME_UID",
            "Environment=XDG_RUNTIME_DIR=/run/user/RUNTIME_UID",
        ),
        _ => (
            "NoNewPrivileges=yes\nReadWritePaths=/var/lib/harness /home/harness",
            "",
        ),
    };
    let user = runtime_user();
    format!(
        r#"{HEAD}{user}install -m 0755 /tmp/harness.upload /usr/local/bin/harness
rm -f /tmp/harness.upload
install -d -o {RUNTIME_USER} -g {RUNTIME_USER} -m 0700 /var/lib/harness /home/{RUNTIME_USER}/work
cat > /var/lib/harness/node.toml <<'TOML'
# This node decides what the master may do here.
max_approval_mode = "ask"
workspace_roots = ["/home/{RUNTIME_USER}/work"]
TOML
chown {RUNTIME_USER}: /var/lib/harness/node.toml
{provider}[ -f /var/lib/harness/remote.toml ] || runuser -u {RUNTIME_USER} -- /usr/local/bin/harness --data-dir /var/lib/harness node join {join_cmd_args}
uid=$(id -u {RUNTIME_USER})
cat > /etc/systemd/system/harness-node.service <<UNIT
[Unit]
Description=harness worker node
After=network-online.target wg-quick@wg-harness.service
Wants=network-online.target

[Service]
User={RUNTIME_USER}
ExecStart=/usr/local/bin/harness --data-dir /var/lib/harness node run
Restart=always
RestartSec=5
ProtectSystem=strict
{hardening}
{env}

[Install]
WantedBy=multi-user.target
UNIT
sed -i "s/RUNTIME_UID/$uid/g" /etc/systemd/system/harness-node.service
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
                crate::facts::Engine::Podman,
            ),
            node_service("x", None, crate::facts::Engine::Docker),
            admin_user("ssh-ed25519 AAAA harness", true),
            admin_user("ssh-ed25519 AAAA harness", false),
            remove_key("ssh-ed25519 AAAA harness"),
            drop_admin_sudo(),
            container_engine(crate::facts::Engine::Podman),
            container_engine(crate::facts::Engine::Docker),
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
        assert!(
            admin_user("k", true).contains("visudo -cf")
                && !admin_user("k", false).contains("NOPASSWD")
        );
        let p = container_engine(crate::facts::Engine::Podman);
        assert!(
            p.contains("enable-linger")
                && p.contains("--add-subuids")
                && !p.contains("usermod -aG docker")
        );
        assert!(
            container_engine(crate::facts::Engine::Docker).contains("usermod -aG docker harness")
        );
        assert!(node_service("x", None, crate::facts::Engine::Podman).contains("XDG_RUNTIME_DIR"));
        assert!(!node_service("x", None, crate::facts::Engine::Podman).contains("NoNewPrivileges"));
        assert!(
            node_service("x", None, crate::facts::Engine::Docker).contains("NoNewPrivileges=yes")
        );
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
