//! T.8b review (`docs/security-review/2026-11-T8b-network-off.md`): the
//! proofs that need the node's own code (`sandbox::start_with_exe`, the bash
//! tool). Not part of the test suite: `run_rust_proofs.sh` beside this file
//! copies it to `crates/cli/tests/`, runs it and removes the copy.
//!
//! `held_*` passes when the claim holds; `finding_*` PASSES WHEN THE FINDING
//! REPRODUCES and prints what it saw (a fix turns the named assertion
//! around).
//!
//! Every listener is this test's own, on this machine. Nothing is sent to
//! another machine: the command's probe (`inside.py`) sends only once it has
//! seen that `lo` is its only interface, and the runs outside a namespace
//! send only to this machine's own unicast addresses. Scratch folders are
//! under `$CARGO_TARGET_DIR/review_t8b/` (never /tmp). No process outlives a
//! test: every command runs through the bash tool to its end.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV6, TcpListener, UdpSocket};
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_tools::contain::ContainMode;
use harness_tools::sandbox::{
    Confinement, ContainLevel, Network, Sandbox, Settings, Started, start_with_exe,
};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const INSIDE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/review_t8b/inside.py"
);

fn scratch_root() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let p = target.join("review_t8b");
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir_in(scratch_root()).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
    }
}

impl Fixture {
    /// As the node starts its sandbox (`sandbox::start`, with the harness
    /// binary given): the Landlock self-test, then the containment probe.
    fn start(&self, network: Network, contain: ContainMode, bwrap: Option<&Path>) -> Started {
        start_with_exe(
            PathBuf::from(EXE),
            &Settings {
                network,
                contain,
                bwrap: bwrap.map(Path::to_path_buf),
                ..Default::default()
            },
            &self.data,
            std::slice::from_ref(&self.data),
            Path::new("/etc/rizzy/node.toml"),
        )
        .unwrap()
    }

    /// Landlock alone, the network off (the fallback): the control.
    fn landlock_only(&self) -> Confinement {
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &Settings {
                network: Network::Off,
                ..Default::default()
            },
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap();
        sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data)
            .expect("Landlock");
        Confinement::Confined {
            sandbox: Arc::new(sb),
            tmp: None,
        }
    }

    async fn bash(&self, c: &Confinement, cmd: &str) -> String {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = c.for_session("s1");
        Bash.run(&ctx, json!({ "command": cmd })).await.output
    }

    /// A stand-in for bubblewrap (as the branch's own test uses one).
    fn stand_in(&self, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = self.root.join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

/// This machine's address on its default route (nothing is sent).
fn lan_addr() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// This machine's first IPv6 link-local address and its interface index
/// (`/proc/net/if_inet6`, scope 0x20).
fn link_local() -> Option<(Ipv6Addr, u32)> {
    let text = std::fs::read_to_string("/proc/net/if_inet6").ok()?;
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 6 || f[3] != "20" {
            return None;
        }
        let ip = Ipv6Addr::from(u128::from_str_radix(f[0], 16).ok()?);
        Some((ip, u32::from_str_radix(f[1], 16).ok()?))
    })
}

fn host_interfaces() -> Vec<String> {
    std::fs::read_to_string("/proc/net/dev")
        .unwrap()
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next().map(|s| s.trim().to_string()))
        .filter(|n| n != "lo")
        .collect()
}

fn udp(addr: impl std::net::ToSocketAddrs) -> UdpSocket {
    let s = UdpSocket::bind(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    s
}

fn drain_udp(s: &UdpSocket) -> Vec<String> {
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while let Ok((n, _)) = s.recv_from(&mut buf) {
        got.push(String::from_utf8_lossy(&buf[..n]).into_owned());
    }
    got
}

const MCAST4: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 77);

/// Everything the test listens on, on this machine.
struct Listeners {
    udp4_loopback: UdpSocket,
    udp6_loopback: Option<UdpSocket>,
    udp4_lan: Option<UdpSocket>,
    udp6_linklocal: Option<UdpSocket>,
    /// 0.0.0.0: broadcast, and the multicast group joined on loopback only.
    any4: UdpSocket,
    any6: Option<UdpSocket>,
    tcp4_loopback: TcpListener,
    tcp4_lan: Option<TcpListener>,
    abstract_stream: (UnixListener, String),
    abstract_dgram: (UnixDatagram, String),
    path_outside: (UnixListener, PathBuf),
    path_inside: (UnixListener, PathBuf),
}

impl Listeners {
    fn new(f: &Fixture) -> Self {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::SocketAddr;
        let lan = lan_addr();
        let any4 = udp("0.0.0.0:0");
        any4.set_broadcast(true).unwrap();
        // Joined on loopback only: no IGMP report on the LAN.
        any4.join_multicast_v4(&MCAST4, &Ipv4Addr::LOCALHOST)
            .unwrap();
        let tcp = |a: IpAddr| {
            let l = TcpListener::bind((a, 0)).unwrap();
            l.set_nonblocking(true).unwrap();
            l
        };
        let unique = format!(
            "rizzy-t8b-review-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (sname, dname) = (format!("{unique}-s"), format!("{unique}-d"));
        let abs_s = UnixListener::bind_addr(&SocketAddr::from_abstract_name(&sname).unwrap())
            .unwrap();
        abs_s.set_nonblocking(true).unwrap();
        let abs_d =
            UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(&dname).unwrap()).unwrap();
        abs_d
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let outside = f.root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        // Bound through a descriptor of its folder (the scratch path is
        // longer than a socket address may be); the command is given the
        // path relative to its folder.
        let path = |dir: &Path, name: &str, relative: &str| {
            use std::os::fd::AsRawFd;
            let d = std::fs::File::open(dir).unwrap();
            let l = UnixListener::bind(format!("/proc/self/fd/{}/{name}", d.as_raw_fd())).unwrap();
            l.set_nonblocking(true).unwrap();
            (l, PathBuf::from(relative))
        };
        Self {
            udp4_loopback: udp("127.0.0.1:0"),
            udp6_loopback: UdpSocket::bind("[::1]:0").ok().map(|_| udp("[::1]:0")),
            udp4_lan: lan.map(|a| udp((a, 0))),
            udp6_linklocal: link_local().and_then(|(ip, idx)| {
                let s = UdpSocket::bind(SocketAddrV6::new(ip, 0, 0, idx)).ok()?;
                s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
                Some(s)
            }),
            any4,
            any6: UdpSocket::bind("[::]:0").ok().map(|_| udp("[::]:0")),
            tcp4_loopback: tcp(Ipv4Addr::LOCALHOST.into()),
            tcp4_lan: lan.map(|a| tcp(a.into())),
            abstract_stream: (abs_s, sname),
            abstract_dgram: (abs_d, dname),
            path_outside: path(&outside, "sock", "../../outside/sock"),
            path_inside: path(&f.ws, "review.sock", "review.sock"),
        }
    }

    fn config(&self, tag: &str, control: bool) -> serde_json::Value {
        let pair = |s: &UdpSocket| {
            let a = s.local_addr().unwrap();
            json!([a.ip().to_string(), a.port()])
        };
        let tpair = |l: &TcpListener| {
            let a = l.local_addr().unwrap();
            json!([a.ip().to_string(), a.port()])
        };
        json!({
            "tag": tag,
            "control": control,
            "udp4_loopback": pair(&self.udp4_loopback),
            "udp6_loopback": self.udp6_loopback.as_ref().map(pair),
            "udp4_lan": self.udp4_lan.as_ref().map(pair),
            "udp6_linklocal": self.udp6_linklocal.as_ref().map(pair),
            "any4_port": self.any4.local_addr().unwrap().port(),
            "any6_port": self.any6.as_ref().map(|s| s.local_addr().unwrap().port()),
            "mcast4": MCAST4.to_string(),
            "host_ifs": host_interfaces(),
            "tcp4_loopback": tpair(&self.tcp4_loopback),
            "tcp4_lan": self.tcp4_lan.as_ref().map(tpair),
            "abstract_stream": self.abstract_stream.1,
            "abstract_dgram": self.abstract_dgram.1,
            "path_outside": self.path_outside.1,
            "path_inside": self.path_inside.1,
        })
    }

    /// What arrived, by listener, since the last call.
    fn arrived(&self) -> Vec<(&'static str, Vec<String>)> {
        let mut got: Vec<(&'static str, Vec<String>)> = Vec::new();
        got.push(("udp4-loopback", drain_udp(&self.udp4_loopback)));
        let opt = |s: &Option<UdpSocket>| s.as_ref().map(drain_udp).unwrap_or_default();
        got.push(("udp6-loopback", opt(&self.udp6_loopback)));
        got.push(("udp4-lan", opt(&self.udp4_lan)));
        got.push(("udp6-linklocal", opt(&self.udp6_linklocal)));
        got.push(("any4 (broadcast, multicast)", drain_udp(&self.any4)));
        got.push(("any6 (multicast)", opt(&self.any6)));
        let accepted = |l: &TcpListener| match l.accept() {
            Ok(_) => vec!["a connection".to_string()],
            Err(_) => vec![],
        };
        got.push(("tcp4-loopback", accepted(&self.tcp4_loopback)));
        got.push((
            "tcp4-lan",
            self.tcp4_lan.as_ref().map(accepted).unwrap_or_default(),
        ));
        let uaccepted = |l: &UnixListener| match l.accept() {
            Ok(_) => vec!["a connection".to_string()],
            Err(_) => vec![],
        };
        got.push(("abstract-stream", uaccepted(&self.abstract_stream.0)));
        let mut buf = [0u8; 256];
        let mut d = Vec::new();
        while let Ok(n) = self.abstract_dgram.0.recv(&mut buf) {
            d.push(String::from_utf8_lossy(&buf[..n]).into_owned());
        }
        got.push(("abstract-dgram", d));
        got.push(("pathname-outside", uaccepted(&self.path_outside.0)));
        got.push(("pathname-inside", uaccepted(&self.path_inside.0)));
        got
    }
}

fn show(what: &str, arrived: &[(&'static str, Vec<String>)]) {
    for (name, got) in arrived {
        println!("  {what}: listener {name}: {got:?}");
    }
}

/// HELD (claim 1). Through the node's own start (`start_with_exe`, the real
/// probe) and the bash tool: with the network off, nothing a command sends
/// arrives at any listener of this machine over IPv4, IPv6 (loopback and
/// link-local), broadcast, multicast, a device bind, a nested namespace,
/// TCP, or an abstract Unix socket (stream and datagram); it has no
/// capabilities, no descriptors beyond 0 to 2 and no interface but `lo`.
/// Control: the same listeners hear the unicast datagrams through Landlock
/// alone.
#[tokio::test]
async fn held_nothing_leaves_the_namespace() {
    let f = fixture();
    let node = f.start(Network::Off, ContainMode::Auto, None);
    println!("containment line: {}", node.contain_summary);
    assert!(
        node.contain_summary
            .contains("a network namespace of its own"),
        "{}",
        node.contain_summary
    );
    let l = Listeners::new(&f);
    std::fs::copy(INSIDE, f.ws.join("inside.py")).unwrap();
    println!("host netns: {:?}", std::fs::read_link("/proc/self/ns/net").unwrap());
    println!("host interfaces: lo {:?}", host_interfaces());

    // Control: Landlock alone, the same listeners.
    std::fs::write(
        f.ws.join("control.json"),
        l.config("control", true).to_string(),
    )
    .unwrap();
    let out = f
        .bash(&f.landlock_only(), "python3 inside.py control.json")
        .await;
    println!("--- control (Landlock alone, network off) ---\n{out}");
    let control = l.arrived();
    show("control", &control);
    let heard = |name: &str| {
        control
            .iter()
            .any(|(n, got)| *n == name && !got.is_empty())
    };
    assert!(heard("udp4-loopback"), "the control listener hears nothing");

    // The contained command.
    std::fs::write(
        f.ws.join("contained.json"),
        l.config("contained", false).to_string(),
    )
    .unwrap();
    let out = f
        .bash(&node.confinement, "python3 inside.py contained.json")
        .await;
    println!("--- contained (network off) ---\n{out}");
    let got = l.arrived();
    show("contained", &got);
    assert!(out.contains("interfaces lo\n"), "{out}");
    assert!(!out.contains("NOT-ISOLATED"), "{out}");
    for (name, g) in &got {
        if *name == "pathname-inside" {
            // The documented way out (a socket in a folder the command may
            // write): noted, not counted.
            continue;
        }
        assert!(g.is_empty(), "listener {name} heard {g:?}");
    }
    assert!(out.contains("CapEff 0000000000000000"), "{out}");
    assert!(!out.contains("fd-3 "), "a descriptor beyond 0 to 2: {out}");
}

/// FINDING T8B-1. The with-namespace probe runs once per start and any
/// failure of it counts as "bubblewrap cannot make a network namespace
/// here". A bubblewrap that fails twice (both forms of one probe, a moment
/// apart) and works from then on leaves an `auto` node without the
/// namespace for its whole life: every later command's UDP reaches the
/// host, the tool result says nothing, and `doctor` on the same machine, a
/// moment later, says the opposite of what the running node does.
#[tokio::test]
async fn finding_one_failed_probe_leaves_the_network_on_for_the_nodes_life() {
    let f = fixture();
    let real = harness_tools::contain::find_bwrap().unwrap();
    let count = f.root.join("count");
    let fake = f.stand_in(
        "bwrap-fails-twice",
        &format!(
            "#!/bin/sh\n# Fails its first two runs that ask for a network namespace (as a full\n\
             # process table would), then is the real one.\n\
             n=$(cat '{count}' 2>/dev/null || echo 0)\n\
             for a; do if [ \"$a\" = --unshare-net ] && [ \"$n\" -lt 2 ]; then\n\
             echo $((n+1)) >'{count}'\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi; done\nexec '{real}' \"$@\"\n",
            count = count.display(),
            real = real.display()
        ),
    );

    // The node starts while bubblewrap fails.
    let node = f.start(Network::Off, ContainMode::Auto, Some(&fake));
    println!("the node's line at start ({:?}): {}", node.contain_level, node.contain_summary);
    assert_eq!(node.contain_level, ContainLevel::Warn);
    assert!(node.contain_summary.contains("network off WITH A GAP"));

    // The same machine a moment later: a second start (what `doctor` does)
    // makes the namespace.
    let later = f.start(Network::Off, ContainMode::Auto, Some(&fake));
    println!("a second start says ({:?}): {}", later.contain_level, later.contain_summary);
    assert!(later.contain_summary.contains("a network namespace of its own"));
    std::fs::write(
        f.data.join("node.toml"),
        format!("[sandbox]\nnetwork = \"off\"\nbwrap = \"{}\"\n", fake.display()),
    )
    .unwrap();
    let doctor = std::process::Command::new(EXE)
        .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
        .output()
        .unwrap();
    let doctor = String::from_utf8_lossy(&doctor.stdout).into_owned();
    let line = doctor
        .lines()
        .find(|l| l.contains("shell containment: "))
        .unwrap_or_default();
    println!("doctor says: {line}");
    assert!(line.starts_with("ok ") && line.contains("a network namespace of its own"));

    // The node that is running goes on without the namespace, for good.
    let host = udp("127.0.0.1:0");
    let port = host.local_addr().unwrap().port();
    for round in 0..3 {
        let out = f
            .bash(
                &node.confinement,
                &format!(
                    "echo \"ifs=[$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev | tr '\\n' ' ')]\"; \
                     printf 'gap-{round}' >/dev/udp/127.0.0.1/{port} && echo sent"
                ),
            )
            .await;
        let got = drain_udp(&host);
        println!("command {round}: tool result {out:?}; the host's listener heard {got:?}");
        assert_eq!(got, [format!("gap-{round}")]);
        assert!(!out.contains("ifs=[lo ]"));
        // Nothing in the result tells the agent or the person.
        assert!(!out.to_lowercase().contains("gap") && !out.contains("network"));
    }

    // `required`, the same two failures: refused for the node's life, with
    // the wrong reason.
    std::fs::remove_file(&count).unwrap();
    let required = f.start(Network::Off, ContainMode::Required, Some(&fake));
    println!("required: {:?}", required.refusal);
    assert_eq!(required.contain_level, ContainLevel::Refused);
}

/// FINDING T8B-2. The probe takes "a network namespace of its own" to mean
/// "`/proc/net/dev` lists `lo` and nothing else". Here the real bubblewrap
/// makes a real namespace; only the list the node's script reads is
/// replaced by one with `lo` and one more interface (what a kernel with a
/// tunnel module loaded puts into every new namespace). The node concludes
/// that there is no namespace: `auto` runs every command without one,
/// `required` refuses.
#[tokio::test]
async fn finding_an_extra_interface_in_a_new_namespace_reads_as_no_namespace() {
    let f = fixture();
    let real = harness_tools::contain::find_bwrap().unwrap();
    let sample = f.root.join("netdev-sample");
    std::fs::write(
        &sample,
        "Inter-|   Receive                                                |  Transmit\n \
         face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs \
         drop fifo colls carrier compressed\n    lo:       0       0    0    0    0     0          \
         0         0        0       0    0    0    0     0       0          0\n tunl0:       0       \
         0    0    0    0     0          0         0        0       0    0    0    0     0       0  \
         0\n",
    )
    .unwrap();
    let log = f.root.join("namespaces.log");
    let fake = f.stand_in(
        "bwrap-extra-interface",
        &format!(
            "#!/bin/sh\n# The real bubblewrap with every option as given. Only the file the\n\
             # node's probe script reads its interface list from is another one.\n\
             net=0; for a; do [ \"$a\" = --unshare-net ] && net=1; done\n\
             if [ \"$net\" = 1 ]; then\n\
             echo \"host $(readlink /proc/self/ns/net) / with --unshare-net \
             $('{real}' --unshare-net --dev-bind / / readlink /proc/self/ns/net)\" >>'{log}'\n\
             cp '{sample}' ./netdev\n\
             n=$#; i=0\n\
             while [ $i -lt $n ]; do a=$1; shift; i=$((i+1))\n\
             case $a in *\"</proc/net/dev\"*) a=$(printf '%s' \"$a\" | sed 's|</proc/net/dev|<./netdev|');; esac\n\
             set -- \"$@\" \"$a\"; done\nfi\nexec '{real}' \"$@\"\n",
            real = real.display(),
            log = log.display(),
            sample = sample.display()
        ),
    );

    let auto = f.start(Network::Off, ContainMode::Auto, Some(&fake));
    println!("auto ({:?}): {}", auto.contain_level, auto.contain_summary);
    println!(
        "what the real bubblewrap made each time:\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    assert_eq!(auto.contain_level, ContainLevel::Warn);
    assert!(auto.contain_summary.contains("network off WITH A GAP"));
    assert!(
        auto.contain_summary
            .contains("the command was not in a network namespace of its own")
    );

    let host = udp("127.0.0.1:0");
    let port = host.local_addr().unwrap().port();
    let out = f
        .bash(
            &auto.confinement,
            &format!(
                "echo \"ifs=[$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev | tr '\\n' ' ')]\"; \
                 printf 'no-namespace' >/dev/udp/127.0.0.1/{port} && echo sent"
            ),
        )
        .await;
    let got = drain_udp(&host);
    println!("a command on that node: {out:?}; the host's listener heard {got:?}");
    assert_eq!(got, ["no-namespace"]);

    let required = f.start(Network::Off, ContainMode::Required, Some(&fake));
    println!("required: {:?}", required.refusal);
    assert_eq!(required.contain_level, ContainLevel::Refused);
}

/// NOTE (claim 5, cost). What the network namespace adds to one command:
/// `true` through the bash tool, the network on and off, 40 runs each.
#[tokio::test]
async fn note_what_the_namespace_costs_per_command() {
    let f = fixture();
    for network in [Network::On, Network::Off, Network::On, Network::Off] {
        let node = f.start(network, ContainMode::Auto, None);
        let mut times = Vec::new();
        for _ in 0..40 {
            let t = std::time::Instant::now();
            let out = f.bash(&node.confinement, "true").await;
            times.push(t.elapsed());
            assert!(out.contains("exit code: 0") || out.trim().is_empty(), "{out}");
        }
        times.sort();
        println!(
            "network {network:?}: median {:.1} ms, worst {:.1} ms per `true`",
            times[20].as_secs_f64() * 1e3,
            times[39].as_secs_f64() * 1e3
        );
    }
}

/// HELD (claim 1, the other form). The shared-`/proc` form (where a fresh
/// `/proc` cannot be mounted) with the network off, forced here: the same
/// probe, nothing arrives, and `/proc/net/dev` is still the command's own
/// namespace's although `/proc` is the machine's.
#[tokio::test]
async fn held_the_shared_proc_form_has_the_namespace_too() {
    use harness_tools::contain::{Contain, Form};
    let f = fixture();
    let sb = Sandbox::new(
        PathBuf::from(EXE),
        &Settings {
            network: Network::Off,
            ..Default::default()
        },
        Some(f.data.join("sandbox-tmp")),
        std::slice::from_ref(&f.data),
    )
    .unwrap();
    let c = Contain::new(harness_tools::contain::find_bwrap().unwrap(), Form::SharedProc);
    sb.probe_containment(&c, &f.data.join("sandbox-tmp/probe-contain"))
        .expect("the node's probe, shared form, network off");
    let conf = Confinement::Confined {
        sandbox: Arc::new(sb.with_containment(c)),
        tmp: None,
    };
    let l = Listeners::new(&f);
    std::fs::copy(INSIDE, f.ws.join("inside.py")).unwrap();
    std::fs::write(
        f.ws.join("shared.json"),
        l.config("shared", false).to_string(),
    )
    .unwrap();
    let out = f.bash(&conf, "python3 inside.py shared.json; ls /proc | grep -c '^[0-9]'").await;
    println!("--- contained, shared /proc (network off) ---\n{out}");
    let got = l.arrived();
    show("shared", &got);
    assert!(out.contains("interfaces lo\n"), "{out}");
    for (name, g) in &got {
        if *name != "pathname-inside" {
            assert!(g.is_empty(), "listener {name} heard {g:?}");
        }
    }
}
