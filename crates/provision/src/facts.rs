//! Read-only inspection of a server: one script, `key=value` lines out.

use serde::Serialize;

/// Runs as the login user; uses `sudo -n` only for read-only queries.
pub const INSPECT: &str = r#"
s() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo -n "$@"; fi; }
. /etc/os-release 2>/dev/null
echo "os_id=${ID:-unknown}"
echo "os_version=${VERSION_ID:-}"
echo "os_like=${ID_LIKE:-}"
echo "os_name=${PRETTY_NAME:-}"
echo "login_user=$(id -un)"
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
command -v podman >/dev/null && echo "podman=yes" || echo "podman=no"
command -v docker >/dev/null && docker --version 2>/dev/null | grep -qi podman && echo "docker_is_podman=yes"
if id harness >/dev/null 2>&1; then
  echo "runtime_user=yes"
  id -nG harness | tr ' ' '\n' | grep -qx docker && echo "runtime_docker=yes" || echo "runtime_docker=no"
  grep -q '^harness:' /etc/subuid 2>/dev/null && echo "runtime_subuid=yes" || echo "runtime_subuid=no"
  [ -e /var/lib/systemd/linger/harness ] && echo "runtime_linger=yes" || echo "runtime_linger=no"
else
  echo "runtime_user=no"
fi
echo "wg_pubkey=$(s wg show wg-harness public-key 2>/dev/null)"
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub os_id: String,
    pub os_version: String,
    /// `ID_LIKE` from os-release (forks name their parents here).
    pub os_like: String,
    pub os_name: String,
    /// The user harness is logged in as.
    pub login_user: String,
    /// Logged in as root (uid 0).
    pub is_root_login: bool,
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
    pub podman: bool,
    /// The `harness` runtime user exists.
    pub runtime_user: bool,
    pub runtime_in_docker_group: bool,
    pub runtime_subuid: bool,
    pub runtime_linger: bool,
}

/// Debian and Ubuntu releases that get full support (latest stable /
/// LTS). Update when a new one ships.
pub const MAIN_SUPPORT: &[(&str, &str)] = &[("debian", "13"), ("ubuntu", "26.04")];

/// How well harness supports a server's OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "level", content = "note", rename_all = "snake_case")]
pub enum Support {
    /// Debian 13 or Ubuntu 26.04 LTS.
    Main,
    /// Other Debian/Ubuntu releases and Debian-based distributions: setup
    /// runs, but it is not tested there.
    Limited(String),
    Unsupported(String),
}

/// Container engine for agents on the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// Rootless Podman for the runtime user (no daemon, no root-equivalent group).
    #[default]
    Podman,
    /// Docker; the runtime user joins the `docker` group (root-equivalent).
    Docker,
    None,
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
            os_like: get("os_like"),
            os_name: get("os_name"),
            login_user: get("login_user"),
            is_root_login: get("uid") == "0",
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
            podman: get("podman") == "yes",
            runtime_user: get("runtime_user") == "yes",
            runtime_in_docker_group: get("runtime_docker") == "yes",
            runtime_subuid: get("runtime_subuid") == "yes",
            runtime_linger: get("runtime_linger") == "yes",
        }
    }

    pub fn support(&self) -> Support {
        let name = if self.os_name.is_empty() {
            format!("{} {}", self.os_id, self.os_version)
        } else {
            self.os_name.clone()
        };
        if MAIN_SUPPORT
            .iter()
            .any(|(id, v)| *id == self.os_id && *v == self.os_version)
        {
            return Support::Main;
        }
        let main = MAIN_SUPPORT
            .iter()
            .map(|(id, v)| format!("{} {v}", if *id == "debian" { "Debian" } else { "Ubuntu" }))
            .collect::<Vec<_>>()
            .join(" and ");
        let debian_family = matches!(self.os_id.as_str(), "debian" | "ubuntu")
            || self
                .os_like
                .split_whitespace()
                .any(|l| l == "debian" || l == "ubuntu");
        if debian_family {
            Support::Limited(format!(
                "{name} works but is not tested; fully supported: {main}"
            ))
        } else {
            Support::Unsupported(format!(
                "{name} is not Debian-based; supported: {main} (and, untested, other Debian-based systems)"
            ))
        }
    }

    /// The container engine is installed and usable by the runtime user.
    pub fn engine_ready(&self, e: Engine) -> bool {
        match e {
            Engine::None => true,
            Engine::Docker => self.docker && self.runtime_in_docker_group,
            // Linger keeps the user session for a system service; without
            // systemd (containers) there is nothing to linger.
            Engine::Podman => {
                self.podman && self.runtime_subuid && (self.runtime_linger || !self.systemd)
            }
        }
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
        assert!(f.root && f.systemd && f.nft && f.ssh_password && !f.is_root_login);
        assert!(
            matches!(f.support(), Support::Limited(_)),
            "24.04 is not the latest LTS"
        );
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

#[cfg(test)]
mod support_tests {
    use super::*;

    fn os(id: &str, v: &str, like: &str) -> Facts {
        Facts {
            os_id: id.into(),
            os_version: v.into(),
            os_like: like.into(),
            ..Default::default()
        }
    }

    #[test]
    fn distro_tiers() {
        assert_eq!(os("debian", "13", "").support(), Support::Main);
        assert_eq!(os("ubuntu", "26.04", "debian").support(), Support::Main);
        assert!(matches!(
            os("ubuntu", "24.04", "debian").support(),
            Support::Limited(_)
        ));
        assert!(matches!(
            os("debian", "12", "").support(),
            Support::Limited(_)
        ));
        assert!(matches!(
            os("linuxmint", "22", "ubuntu debian").support(),
            Support::Limited(_)
        ));
        assert!(matches!(
            os("raspbian", "13", "debian").support(),
            Support::Limited(_)
        ));
        assert!(matches!(
            os("fedora", "43", "").support(),
            Support::Unsupported(_)
        ));
        assert!(matches!(
            os("rocky", "10", "rhel centos fedora").support(),
            Support::Unsupported(_)
        ));
    }

    #[test]
    fn engine_readiness() {
        let mut f = Facts::default();
        assert!(f.engine_ready(Engine::None) && !f.engine_ready(Engine::Podman));
        f.docker = true;
        assert!(
            !f.engine_ready(Engine::Docker),
            "installed but harness is not in the group"
        );
        f.runtime_in_docker_group = true;
        assert!(f.engine_ready(Engine::Docker));
        f.podman = true;
        f.runtime_subuid = true;
        assert!(
            f.engine_ready(Engine::Podman),
            "no systemd: nothing to linger"
        );
        f.systemd = true;
        assert!(
            !f.engine_ready(Engine::Podman),
            "with systemd it needs linger for a user service"
        );
        f.runtime_linger = true;
        assert!(f.engine_ready(Engine::Podman));
    }
}
