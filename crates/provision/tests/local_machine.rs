//! Runs the real setup scripts on THIS machine (no SSH). It changes the
//! machine: installs packages, creates the users `harness-admin` and
//! `harness`, loads and removes an nftables table. Root only; run with
//! `HARNESS_LOCAL_TESTS=1 cargo test -p harness-provision --test local_machine -- --nocapture`.
//! Steps this machine cannot run (no systemd, no WireGuard module) are
//! reported, not faked.

use harness_provision::facts::{Engine, Facts};
use harness_provision::scripts;
use harness_provision::setup::{LocalRemote, Remote, inspect};

fn enabled() -> bool {
    std::env::var_os("HARNESS_LOCAL_TESTS").is_some()
}

async fn sh(cmd: &str) -> (u32, String, String) {
    let o = LocalRemote.run(cmd, true).await.unwrap();
    (o.code, o.stdout, o.stderr)
}

#[tokio::test]
async fn setup_steps_on_this_machine() {
    if !enabled() {
        eprintln!("skipped: set HARNESS_LOCAL_TESTS=1 (root; changes this machine)");
        return;
    }
    let systemd = std::path::Path::new("/run/systemd/system").exists();
    // A previous interrupted run may have left the docker group behind.
    sh("id harness >/dev/null 2>&1 && gpasswd -d harness docker >/dev/null 2>&1; true").await;

    // 1. Inspection on a real system.
    let f: Facts = inspect(&LocalRemote).await.unwrap();
    println!(
        "facts: {} {} {} root={} systemd={} nft={} support={:?}",
        f.os_id,
        f.os_version,
        f.arch,
        f.root,
        f.systemd,
        f.nft,
        f.support()
    );
    assert!(f.is_root_login && f.root && f.nft);
    assert_eq!(f.systemd, systemd);

    // 2. Firewall rules: check, load, verify, then the revert command.
    let dir = tempfile::tempdir().unwrap();
    let rules = dir.path().join("fw.nft");
    std::fs::write(&rules, scripts::firewall_rules(f.ssh_port)).unwrap();
    let (c, _, e) = sh(&format!("nft -c -f {}", rules.display())).await;
    assert_eq!(c, 0, "{e}");
    let (c, _, e) = sh(&format!(
        "nft delete table inet harness 2>/dev/null; nft -f {}",
        rules.display()
    ))
    .await;
    assert_eq!(c, 0, "loading the rules: {e}");
    let (c, out, _) = sh("nft list table inet harness").await;
    assert!(
        c == 0 && out.contains("policy drop") && out.contains("udp dport 51820 accept"),
        "{out}"
    );
    // Outbound still works with the table loaded (agents need it).
    let (c, _, e) = sh("getent hosts github.com >/dev/null || curl -sS -o /dev/null --max-time 10 https://github.com").await;
    println!(
        "outbound with firewall loaded: {}",
        if c == 0 { "ok".to_string() } else { e }
    );
    let (c, _, e) = sh("nft delete table inet harness").await;
    assert_eq!(c, 0, "the revert command: {e}");
    assert_ne!(
        sh("nft list table inet harness").await.0,
        0,
        "table gone after revert"
    );
    println!("firewall: check, load, revert ok");

    // 3. SSH hardening drop-in: validated by the real sshd, on a copy of
    //    this machine's config (the live config is not touched).
    let d = dir.path();
    std::fs::create_dir_all(d.join("conf.d")).unwrap();
    std::fs::write(d.join("conf.d/10-harness.conf"), "PasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin prohibit-password\n").unwrap();
    std::fs::write(
        d.join("sshd_config"),
        format!(
            "Include {}/conf.d/*.conf\nPasswordAuthentication yes\n",
            d.display()
        ),
    )
    .unwrap();
    let (c, out, e) = sh(&format!("mkdir -p /run/sshd; [ -f /etc/ssh/ssh_host_ed25519_key ] || ssh-keygen -A >/dev/null; sshd -T -f {}/sshd_config", d.display())).await;
    assert_eq!(c, 0, "{e}");
    assert!(
        out.contains("passwordauthentication no"),
        "drop-in wins over the later line"
    );
    assert!(
        out.contains("permitrootlogin without-password")
            || out.contains("permitrootlogin prohibit-password")
    );
    println!("ssh drop-in: sshd -T confirms passwords off");

    // 4. harness-admin with sudo, then without.
    let key =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl harness";
    let (c, _, e) = sh(&scripts::admin_user(key, true)).await;
    assert_eq!(c, 0, "creating harness-admin: {e}");
    let (c, out, _) = sh("su -s /bin/bash harness-admin -c 'sudo -n id -u'").await;
    assert!(
        c == 0 && out.trim() == "0",
        "harness-admin has sudo during setup"
    );
    assert!(
        std::fs::read_to_string("/home/harness-admin/.ssh/authorized_keys")
            .unwrap()
            .contains("AAAAC3Nza")
    );
    let (c, out, _) = sh("passwd -S harness-admin").await;
    assert!(
        c == 0 && out.split_whitespace().nth(1) == Some("L"),
        "password locked: {out}"
    );
    sh(&scripts::drop_admin_sudo()).await;
    let (c, _, _) = sh("su -s /bin/bash harness-admin -c 'sudo -n true' 2>/dev/null").await;
    assert_ne!(c, 0, "sudo removed");
    println!("harness-admin: created, sudo granted, password locked, sudo removed");

    // 5. Rootless Podman for the runtime user.
    let (c, out, e) = sh(&scripts::container_engine(Engine::Podman)).await;
    println!(
        "podman step: exit {c}\n{}{}",
        out.lines().last().unwrap_or(""),
        if c == 0 {
            String::new()
        } else {
            format!("\n{e}")
        }
    );
    assert_eq!(c, 0, "{e}");
    let (c, out, _) = sh("grep '^harness:' /etc/subuid; id -nG harness").await;
    assert_eq!(c, 0);
    assert!(
        !out.contains("docker") && !out.contains("sudo"),
        "no root-equivalent groups: {out}"
    );
    // `--network none`: some sandboxes block /dev/net/tun (slirp4netns).
    let (c, out, e) = sh("uid=$(id -u harness); runuser -u harness -- env XDG_RUNTIME_DIR=/run/user/$uid HOME=/home/harness podman run --rm --network none docker.io/library/alpine:3.20 cat /proc/self/uid_map 2>/dev/null").await;
    assert_eq!(c, 0, "{e}");
    let first: Vec<&str> = out
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    let host_uid = sh("id -u harness").await.1.trim().to_string();
    assert_eq!(
        first,
        ["0", host_uid.as_str(), "1"],
        "container root is the unprivileged host user: {out}"
    );
    println!("rootless container: container uid 0 = host uid {host_uid} (harness)");
    let after = inspect(&LocalRemote).await.unwrap();
    assert!(after.engine_ready(Engine::Podman));

    // 6. Docker for the runtime user, when a Docker daemon is running here.
    if sh("docker info >/dev/null 2>&1").await.0 == 0 {
        let script = scripts::container_engine(Engine::Docker)
            // `systemctl enable --now docker` needs systemd; the daemon already runs.
            .replace(
                "systemctl enable --now docker",
                if systemd {
                    "systemctl enable --now docker"
                } else {
                    "true"
                },
            );
        let (c, out, e) = sh(&script).await;
        assert_eq!(c, 0, "{out}{e}");
        let groups = sh("id -nG harness").await.1;
        assert!(groups.split_whitespace().any(|g| g == "docker"), "{groups}");
        // Registries rate-limit; reuse the image Podman already pulled.
        sh("docker image inspect alpine:3.20 >/dev/null 2>&1 || (uid=$(id -u harness); runuser -u harness -- env XDG_RUNTIME_DIR=/run/user/$uid HOME=/home/harness podman save docker.io/library/alpine:3.20 | docker load)").await;
        let (c, _, e) = sh("runuser -u harness -g docker -- docker run --rm --network none docker.io/library/alpine:3.20 true").await;
        assert_eq!(c, 0, "{e}");
        assert!(
            inspect(&LocalRemote)
                .await
                .unwrap()
                .engine_ready(Engine::Docker)
        );
        println!(
            "docker: harness in the docker group, container runs (and that group is root-equivalent)"
        );
        // Leave the runtime user without the root-equivalent group.
        sh("gpasswd -d harness docker >/dev/null").await;
    } else {
        println!("NOT RUN here: Docker engine step (no Docker daemon)");
    }

    if !systemd {
        println!(
            "NOT RUN here (no systemd): revert timers (systemd-run), sshd reload, service units, linger"
        );
    }
    println!("NOT RUN here: WireGuard (no kernel module)");
}
