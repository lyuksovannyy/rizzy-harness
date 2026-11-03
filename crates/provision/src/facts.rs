//! Read-only inspection of a server: one script, `key=value` lines out.

use serde::Serialize;

/// Runs as the login user; uses `sudo -n` only for read-only queries.
pub const INSPECT: &str = r#"
s() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo -n "$@"; fi; }
. /etc/os-release 2>/dev/null
echo "os_id=${ID:-unknown}"
echo "os_version=${VERSION_ID:-}"
echo "arch=$(uname -m)"
echo "glibc=$(ldd --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+$')"
echo "uid=$(id -u)"
if [ "$(id -u)" = 0 ] || sudo -n true 2>/dev/null; then echo "root=yes"; else echo "root=no"; fi
command -v systemctl >/dev/null && [ -d /run/systemd/system ] && echo "systemd=yes" || echo "systemd=no"
command -v wg >/dev/null && echo "wg_tools=yes" || echo "wg_tools=no"
s ip -o link show type wireguard 2>/dev/null | grep -q wg-harness && echo "wg_harness=up" || echo "wg_harness=down"
command -v nft >/dev/null && echo "nft=yes" || echo "nft=no"
s nft list table inet harness >/dev/null 2>&1 && echo "harness_fw=yes" || echo "harness_fw=no"
command -v ufw >/dev/null && s ufw status 2>/dev/null | grep -q 'Status: active' && echo "ufw=active" || echo "ufw=inactive"
systemctl is-active firewalld >/dev/null 2>&1 && echo "firewalld=active" || echo "firewalld=inactive"
command -v docker >/dev/null && echo "docker=yes" || echo "docker=no"
echo "other_fw_rules=$(s nft list ruleset 2>/dev/null | grep -v 'table inet harness' | grep -c 'chain ')"
cfg=$(s sshd -T 2>/dev/null)
echo "ssh_password=$(echo "$cfg" | awk '$1=="passwordauthentication"{print $2}')"
echo "ssh_root_login=$(echo "$cfg" | awk '$1=="permitrootlogin"{print $2}')"
echo "ssh_port=$(echo "$cfg" | awk '$1=="port"{print $2; exit}')"
command -v harness >/dev/null && echo "harness=$(harness --version 2>/dev/null | awk '{print $2}')" || echo "harness=none"
systemctl is-active harness-node >/dev/null 2>&1 && echo "harness_service=active" || echo "harness_service=inactive"
ipaddr=$(ip -o route get 1.1.1.1 2>/dev/null | grep -oE 'src [0-9.]+' | cut -d' ' -f2)
[ -n "$ipaddr" ] || ipaddr=$(hostname -I 2>/dev/null | awk '{print $1}')
echo "public_ip=$ipaddr"
echo "wg_pubkey=$(s wg show wg-harness public-key 2>/dev/null)"
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub os_id: String,
    pub os_version: String,
    pub arch: String,
    pub glibc: String,
    pub root: bool,
    pub systemd: bool,
    pub wg_tools: bool,
    pub wg_up: bool,
    pub nft: bool,
    pub harness_fw: bool,
    pub ufw_active: bool,
    pub firewalld_active: bool,
    pub docker: bool,
    pub other_fw_chains: u32,
    pub ssh_password: bool,
    pub ssh_root_login: String,
    pub ssh_port: u16,
    pub harness: Option<String>,
    pub harness_service: bool,
    pub public_ip: String,
    /// The node's WireGuard public key, once its tunnel exists.
    pub wg_pubkey: Option<String>,
}

impl Facts {
    pub fn parse(out: &str) -> Self {
        let get = |k: &str| {
            out.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .unwrap_or("")
                .trim()
                .to_string()
        };
        Self {
            os_id: get("os_id"),
            os_version: get("os_version"),
            arch: get("arch"),
            glibc: get("glibc"),
            root: get("root") == "yes",
            systemd: get("systemd") == "yes",
            wg_tools: get("wg_tools") == "yes",
            wg_up: get("wg_harness") == "up",
            nft: get("nft") == "yes",
            harness_fw: get("harness_fw") == "yes",
            ufw_active: get("ufw") == "active",
            firewalld_active: get("firewalld") == "active",
            docker: get("docker") == "yes",
            other_fw_chains: get("other_fw_rules").parse().unwrap_or(0),
            // Unknown (no sudo to ask sshd) counts as "maybe on".
            ssh_password: get("ssh_password") != "no",
            ssh_root_login: get("ssh_root_login"),
            ssh_port: get("ssh_port").parse().unwrap_or(22),
            harness: Some(get("harness")).filter(|v| !v.is_empty() && v != "none"),
            harness_service: get("harness_service") == "active",
            public_ip: get("public_ip"),
            wg_pubkey: Some(get("wg_pubkey")).filter(|v| !v.is_empty()),
        }
    }

    /// Debian and Ubuntu are what the automatic setup knows how to drive.
    pub fn supported_os(&self) -> bool {
        matches!(self.os_id.as_str(), "debian" | "ubuntu")
    }

    /// Something else already manages the firewall: automatic setup would
    /// fight it, so only manual setup is offered.
    pub fn firewall_conflict(&self) -> Option<String> {
        let mut v = vec![];
        if self.ufw_active {
            v.push("ufw is active");
        }
        if self.firewalld_active {
            v.push("firewalld is active");
        }
        if self.docker {
            v.push("Docker manages firewall rules");
        }
        if self.other_fw_chains > 0 {
            v.push("other nftables/iptables rules exist");
        }
        (!v.is_empty()).then(|| v.join(", "))
    }

    /// Glibc at least `min` ("2.35"); unknown counts as too old.
    pub fn glibc_at_least(&self, min: &str) -> bool {
        let p = |s: &str| -> Option<(u32, u32)> {
            let (a, b) = s.split_once('.')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        };
        matches!((p(&self.glibc), p(min)), (Some(a), Some(b)) if a >= b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inspection_output() {
        let f = Facts::parse(
            "os_id=ubuntu\nos_version=24.04\narch=x86_64\nglibc=2.39\nuid=1000\nroot=yes\nsystemd=yes\n\
             wg_tools=no\nwg_harness=down\nnft=yes\nharness_fw=no\nufw=active\nfirewalld=inactive\n\
             docker=no\nother_fw_rules=3\nssh_password=yes\nssh_root_login=without-password\n\
             ssh_port=2222\nharness=none\nharness_service=inactive\npublic_ip=203.0.113.7\n",
        );
        assert!(f.supported_os() && f.root && f.systemd && f.nft && f.ssh_password);
        assert_eq!((f.ssh_port, f.public_ip.as_str()), (2222, "203.0.113.7"));
        assert_eq!(f.harness, None);
        assert_eq!(
            f.firewall_conflict().unwrap(),
            "ufw is active, other nftables/iptables rules exist"
        );
        assert!(f.glibc_at_least("2.35") && !f.glibc_at_least("2.40"));
        let empty = Facts::parse("");
        assert!(empty.ssh_password && empty.ssh_port == 22 && !empty.glibc_at_least("2.0"));
    }

    #[test]
    fn inspection_script_is_valid_bash() {
        let out = std::process::Command::new("bash")
            .args(["-n", "-c", INSPECT])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
