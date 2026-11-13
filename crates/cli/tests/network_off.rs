//! Network off means off (ADOPTION_PLAN T.8b, O12). With `[sandbox]
//! network = "off"` Landlock (ABI 4) refuses TCP bind and connect, but UDP,
//! and so DNS, still went out (experiment B.11 §5, `experiments/b11/
//! net.sh`). A contained command now also gets a network namespace of its
//! own (`bwrap --unshare-net`): only its own loopback, nothing of the
//! host's. Landlock's TCP rule stays inside as a second layer.
//!
//! Through the real `rizzy-harness sandbox-exec` wrapper, the real `bwrap`
//! and the real bash tool. Every listener is the test's own, on a port the
//! kernel picks, on this machine's loopback or LAN address; nothing here
//! contacts another machine (the DNS lookup runs only once the command's
//! namespace is seen to hold nothing but `lo`). Where bubblewrap or
//! Landlock does not work they skip with a printed reason; with
//! `HARNESS_REQUIRE_BWRAP=1` they fail.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::net::{IpAddr, TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_tools::sandbox::{Confinement, Network, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
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

fn settings(network: Network) -> Settings {
    Settings {
        network,
        ..Default::default()
    }
}

impl Fixture {
    fn plain(&self, network: Network) -> Sandbox {
        Sandbox::new(
            PathBuf::from(EXE),
            &settings(network),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// Landlock alone (the fallback where bubblewrap cannot run), or `None`
    /// where Landlock with its TCP rule (ABI 4) does not work here.
    fn landlock_only(&self, network: Network) -> Option<Arc<Sandbox>> {
        let sb = self.plain(network);
        match sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            Ok(s) if harness_tools::sandbox::effective_abi(&s) >= 4 => Some(Arc::new(sb)),
            r => {
                assert!(
                    !bwrap::required(),
                    "Landlock ABI 4 does not work here: {r:?}"
                );
                eprintln!("SKIPPED: Landlock ABI 4 does not work here: {r:?}");
                None
            }
        }
    }

    /// The contained sandbox as the node sets it up (probed with this
    /// network setting), or `None` where it cannot run here.
    fn contained(&self, network: Network) -> Option<Arc<Sandbox>> {
        self.landlock_only(network)?;
        let sb = self.plain(network);
        bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain"))
            .ok()
            .map(Arc::new)
    }

    async fn bash(&self, sb: Arc<Sandbox>, cmd: &str) -> String {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        Bash.run(&ctx, json!({ "command": cmd })).await.output
    }
}

/// This machine's address on its default route, found without sending
/// anything (a UDP `connect` only picks the source address), or `None`
/// (and a printed reason) when it has only loopback.
fn lan_addr() -> Option<IpAddr> {
    let found = UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        // TEST-NET-1 (RFC 5737): nothing is sent, nothing listens there.
        s.connect("192.0.2.1:9")?;
        Ok(s.local_addr()?.ip())
    });
    match found {
        Ok(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        r => {
            eprintln!("SKIPPED (LAN half): this machine has no non-loopback address ({r:?})");
            None
        }
    }
}

fn have_python() -> bool {
    let ok = std::process::Command::new("python3")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        assert!(!bwrap::required(), "this test needs python3");
        eprintln!("SKIPPED: no python3");
    }
    ok
}

/// The host-side UDP listeners: 127.0.0.1, `[::1]` where this machine has
/// it, and the LAN address where it has one.
fn udp_listeners() -> Vec<UdpSocket> {
    let mut hosts: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap()];
    if UdpSocket::bind("[::1]:0").is_ok() {
        hosts.push("::1".parse().unwrap());
    } else {
        eprintln!("SKIPPED (IPv6 half): no ::1 here");
    }
    hosts.extend(lan_addr());
    hosts
        .into_iter()
        .map(|h| {
            let s = UdpSocket::bind((h, 0)).unwrap();
            s.set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            s
        })
        .collect()
}

/// A command that sends one datagram `<tag>-<n>` to each listener.
fn udp_probe(listeners: &[UdpSocket], tag: &str) -> String {
    listeners
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let a = l.local_addr().unwrap();
            format!(
                "printf '{tag}-{i}' >/dev/udp/{}/{} && echo 'sent {i}' || echo 'unsent {i}'; ",
                a.ip(),
                a.port()
            )
        })
        .collect()
}

/// The datagrams that arrived at each listener (after the command ended).
fn arrived(listeners: &[UdpSocket]) -> Vec<Vec<String>> {
    listeners
        .iter()
        .map(|l| {
            let mut got = Vec::new();
            let mut buf = [0u8; 256];
            while let Ok(n) = l.recv(&mut buf) {
                got.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            got
        })
        .collect()
}

/// The network interfaces the command sees (`/proc/net/dev` is the reading
/// process's own namespace's).
const INTERFACES: &str =
    "echo \"ifs=[$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev | tr '\\n' ' ')]\"; ";

/// The network namespace the command is in, by identity (review T8B-2: the
/// interface list is no proof; a kernel with a tunnel module puts `tunl0`
/// and the like into every new namespace).
const IDENTITY: &str = "echo \"ns=$(readlink /proc/self/ns/net)\"; ";

/// What [`IDENTITY`] prints in this process's (the host's) namespace.
fn host_ns() -> String {
    format!(
        "ns={}",
        std::fs::read_link("/proc/self/ns/net")
            .unwrap()
            .to_string_lossy()
    )
}

/// The command printed a namespace of its own, not the host's.
fn own_ns(out: &str) -> bool {
    out.contains("ns=net:[") && !out.contains(&host_ns())
}

// ---- UDP ----

/// Network off: a datagram to the host's 127.0.0.1, `::1` or LAN address
/// never arrives. Control, same listeners: through Landlock alone (the
/// fallback) the same datagrams do arrive, so this proves the namespace,
/// not a listener that hears nothing.
#[tokio::test]
async fn network_off_udp_never_reaches_the_host() {
    let f = fixture();
    let Some(contained) = f.contained(Network::Off) else {
        return;
    };
    let landlock = f.landlock_only(Network::Off).unwrap();
    let listeners = udp_listeners();

    let out = f.bash(landlock, &udp_probe(&listeners, "landlock")).await;
    let got = arrived(&listeners);
    for (i, g) in got.iter().enumerate() {
        assert_eq!(
            g,
            &[format!("landlock-{i}")],
            "control: Landlock alone let it out; listener {i}: {out}"
        );
    }

    let out = f.bash(contained, &udp_probe(&listeners, "contained")).await;
    let got = arrived(&listeners);
    assert!(
        got.iter().all(Vec::is_empty),
        "network off, contained: datagrams reached the host: {got:?}; {out}"
    );
}

// ---- TCP and the namespace itself ----

/// Network off: TCP to the host's 127.0.0.1 and LAN listeners fails
/// (Landlock refused it before; it still does, inside the namespace), and
/// the command sees no interface but its own `lo`.
#[tokio::test]
async fn network_off_tcp_fails_and_only_loopback_is_there() {
    let f = fixture();
    let Some(sb) = f.contained(Network::Off) else {
        return;
    };
    let mut hosts: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap()];
    hosts.extend(lan_addr());
    let listeners: Vec<TcpListener> = hosts
        .iter()
        .map(|h| {
            let l = TcpListener::bind((*h, 0)).unwrap();
            l.set_nonblocking(true).unwrap();
            l
        })
        .collect();
    let mut cmd = format!("{INTERFACES}{IDENTITY}");
    for (i, l) in listeners.iter().enumerate() {
        let a = l.local_addr().unwrap();
        cmd.push_str(&format!(
            "(exec 3<>/dev/tcp/{}/{}) && echo 'connected {i}' || echo 'refused {i}'; ",
            a.ip(),
            a.port()
        ));
    }
    let out = f.bash(sb, &cmd).await;
    assert!(!out.contains("connected"), "{out}");
    for (i, l) in listeners.iter().enumerate() {
        assert!(out.contains(&format!("refused {i}")), "{out}");
        assert!(l.accept().is_err(), "listener {i} got a connection: {out}");
    }
    assert!(own_ns(&out), "not in a namespace of its own: {out}");
    // The point of this test: nothing but its own loopback on this machine.
    assert!(
        out.contains("ifs=[lo ]"),
        "not only its own loopback: {out}"
    );
}

/// Loopback inside the command, before and after T.8b: bubblewrap brings
/// the namespace's `lo` up, so UDP to itself on 127.0.0.1 works (before, it
/// went over the host's loopback); a TCP server on 127.0.0.1 was refused by
/// Landlock's TCP rule before and still is (`bind`: EACCES), unchanged.
#[tokio::test]
async fn network_off_keeps_the_commands_own_loopback() {
    let f = fixture();
    let Some(sb) = f.contained(Network::Off) else {
        return;
    };
    if !have_python() {
        return;
    }
    let py = r#"
import socket
u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
u.bind(("127.0.0.1", 0))
u.settimeout(2)
u.sendto(b"self", u.getsockname())
print("udp-self", u.recv(16).decode())
try:
    t = socket.socket()
    t.bind(("127.0.0.1", 0))
    t.listen()
    print("tcp-bind ok")
except OSError as e:
    print("tcp-bind", type(e).__name__)
"#;
    let out = f.bash(sb, &format!("{IDENTITY} python3 -c '{py}'")).await;
    assert!(own_ns(&out), "{out}");
    assert!(out.contains("udp-self self"), "loopback is not up: {out}");
    assert!(out.contains("tcp-bind PermissionError"), "{out}");
}

// ---- DNS ----

/// Network off: what this proves is the namespace, with a lookup that
/// sends nothing (review N-2). The lookup runs only once the command is
/// seen to be in a namespace of its own with no interface but `lo`, so it
/// cannot leave the machine (on the old path the command prints
/// `lookup-skipped` and this fails without sending anything); and a name
/// under `.invalid` (RFC 6761) fails on every machine anyway. Whether
/// anything leaves is proved by the UDP test above, with a listener that
/// would hear it; there is no way to test a real lookup here without
/// leaving the machine.
#[tokio::test]
async fn network_off_dns_lookups_fail() {
    let f = fixture();
    let Some(sb) = f.contained(Network::Off) else {
        return;
    };
    let cmd = format!(
        "{IDENTITY}{INTERFACES} if [ \"$(readlink /proc/self/ns/net)\" != '{host}' ] && \
         [ \"$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev)\" = lo ]; then \
         getent hosts rizzy-t8b-probe.invalid; echo \"lookup-exit=$?\"; \
         getent hosts localhost >/dev/null; echo \"files-exit=$?\"; \
         else echo lookup-skipped; fi",
        host = host_ns().trim_start_matches("ns=")
    );
    let started = std::time::Instant::now();
    let out = f.bash(sb, &cmd).await;
    assert!(
        !out.contains("lookup-skipped"),
        "not in its own namespace: {out}"
    );
    assert!(
        out.contains("lookup-exit=") && !out.contains("lookup-exit=0"),
        "{out}"
    );
    // /etc/hosts still answers: only the network is gone.
    assert!(out.contains("files-exit=0"), "{out}");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the lookup hung: {out}"
    );
}

// ---- abstract Unix sockets ----

fn abstract_listener() -> (std::os::unix::net::UnixListener, String) {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    let name = format!(
        "rizzy-t8b-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let l = UnixListener::bind_addr(&SocketAddr::from_abstract_name(&name).unwrap()).unwrap();
    l.set_nonblocking(true).unwrap();
    (l, name)
}

fn abstract_probe(name: &str) -> String {
    format!(
        "python3 -c 'import socket, errno, sys\n\
         s = socket.socket(socket.AF_UNIX)\n\
         try:\n    s.connect(\"\\0\" + sys.argv[1]); print(\"abstract-connected\")\n\
         except OSError as e:\n    print(\"abstract-\" + errno.errorcode[e.errno])' {name}"
    )
}

/// Network off: a host abstract Unix socket is not there at all (abstract
/// names belong to the network namespace): `ECONNREFUSED`, whatever the
/// Landlock ABI. Before T.8b it was Landlock's scope (ABI 6, `EPERM`), and
/// below ABI 6 it connected.
#[tokio::test]
async fn network_off_hides_the_hosts_abstract_sockets() {
    let f = fixture();
    let Some(sb) = f.contained(Network::Off) else {
        return;
    };
    if !have_python() {
        return;
    }
    let (l, name) = abstract_listener();
    let out = f.bash(sb, &abstract_probe(&name)).await;
    assert!(out.contains("abstract-ECONNREFUSED"), "{out}");
    assert!(
        l.accept().is_err(),
        "the host's socket got a connection: {out}"
    );
}

// ---- network on: nothing changes ----

/// Network on, contained: the same probes as the fallback with the network
/// on, which is what a contained command did before T.8b: TCP connects to
/// the host's listeners, UDP arrives, the host's interfaces are there, and
/// an abstract socket gives the same answer as through Landlock alone. No
/// DNS lookup here: with the network on it would leave the machine.
#[tokio::test]
async fn network_on_is_unchanged() {
    let f = fixture();
    let Some(contained) = f.contained(Network::On) else {
        return;
    };
    let landlock = f.landlock_only(Network::On).unwrap();
    let listeners = udp_listeners();
    let tcp: Vec<TcpListener> = listeners
        .iter()
        .map(|u| TcpListener::bind((u.local_addr().unwrap().ip(), 0)).unwrap())
        .collect();
    let (abs, name) = abstract_listener();
    let python = have_python();
    let probe = |tag: &str| {
        let mut cmd = format!("{INTERFACES}{IDENTITY}{}", udp_probe(&listeners, tag));
        for (i, l) in tcp.iter().enumerate() {
            let a = l.local_addr().unwrap();
            cmd.push_str(&format!(
                "(exec 3<>/dev/tcp/{}/{}) && echo 'connected {i}' || echo 'refused {i}'; ",
                a.ip(),
                a.port()
            ));
        }
        if python {
            cmd.push_str(&abstract_probe(&name));
        }
        cmd
    };
    let before = f.bash(landlock, &probe("landlock")).await;
    let got_before = arrived(&listeners);
    let after = f.bash(contained, &probe("contained")).await;
    let got_after = arrived(&listeners);
    assert_eq!(
        before, after,
        "network on: contained differs from Landlock alone"
    );
    for (i, (b, a)) in got_before.iter().zip(&got_after).enumerate() {
        assert_eq!(b, &[format!("landlock-{i}")], "{before}");
        assert_eq!(a, &[format!("contained-{i}")], "{after}");
    }
    for i in 0..tcp.len() {
        assert!(after.contains(&format!("connected {i}")), "{after}");
    }
    assert!(!after.contains("ifs=[lo ]"), "{after}");
    assert!(after.contains(&host_ns()), "{after}");
    assert!(!after.contains("abstract-ECONNREFUSED"), "{after}");
    drop(abs);
}

// ---- what the node says, and what it does when it cannot ----

/// `doctor`'s containment line with the network off: contained, it names
/// the namespace; where the sandbox mode is not offered (no bubblewrap, or
/// `[execution] sandbox = "off"`) it says that UDP and DNS still go out for
/// what runs uncontained (a `host` session): a warning where the node
/// offers `host`, a failure where it then offers nothing (N.10a).
#[test]
fn doctor_says_where_udp_and_dns_still_go_out() {
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let doctor = |toml: &str| {
        std::fs::write(f.data.join("node.toml"), toml).unwrap();
        let out = std::process::Command::new(EXE)
            .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout).into_owned();
        out.lines()
            .find(|l| l.contains(" shell containment: "))
            .unwrap_or_else(|| panic!("no containment line: {out}"))
            .to_string()
    };
    let c = doctor("[sandbox]\nnetwork = \"off\"\n");
    assert!(c.contains("contained ("), "{c}");
    assert!(c.contains("a network namespace of its own"), "{c}");
    assert!(!c.contains("UDP and DNS still go out"), "{c}");

    for toml in [
        "[sandbox]\nnetwork = \"off\"\nbwrap = \"/nonexistent/bwrap\"\n",
        "[sandbox]\nnetwork = \"off\"\n[execution]\nsandbox = \"off\"\n",
    ] {
        let c = doctor(toml);
        assert!(c.starts_with("FAIL "), "{toml}: {c}");
        assert!(c.contains("UDP and DNS still go out"), "{toml}: {c}");
        let c = doctor(&format!("{toml}[execution]\nhost = true\n").replace(
            "[execution]\nsandbox = \"off\"\n[execution]\n",
            "[execution]\nsandbox = \"off\"\n",
        ));
        assert!(c.starts_with("warn "), "{toml}: {c}");
        assert!(c.contains("UDP and DNS still go out"), "{toml}: {c}");
    }
    // The network on: no word about it.
    let c = doctor("[sandbox]\nbwrap = \"/nonexistent/bwrap\"\n");
    assert!(!c.contains("UDP"), "{c}");
}

/// A bubblewrap that works but cannot make a network namespace (a stand-in
/// that refuses `--unshare-net`, as a sandboxed outer container might),
/// with the network off: `auto` still contains (a PID namespace, the guard,
/// Landlock inside), without the network namespace, and its line says so as
/// a gap naming bubblewrap's error; `strict` does not offer the sandbox
/// mode, with the reason.
/// Neither runs a command with the network silently on. With the network
/// on the same bubblewrap contains as usual.
#[tokio::test]
async fn a_bwrap_without_network_namespaces_still_contains_under_auto() {
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let real = harness_tools::contain::find_bwrap().unwrap();
    let fake = f.root.join("bwrap-no-net");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nfor a; do [ \"$a\" = --unshare-net ] && {{ echo 'bwrap: Creating new \
             namespace failed: Operation not permitted' >&2; exit 1; }}; done\nexec {} \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let start = |network, contain| {
        harness_tools::sandbox::start_with_exe(
            PathBuf::from(EXE),
            &Settings {
                network,
                contain,
                bwrap: Some(fake.clone()),
                ..Default::default()
            },
            &f.data,
            std::slice::from_ref(&f.data),
            Path::new("/etc/rizzy/node.toml"),
        )
        .unwrap()
    };
    use harness_tools::contain::ContainMode;
    use harness_tools::sandbox::ContainLevel;

    let on = start(Network::On, ContainMode::Auto);
    assert!(
        on.contain_summary.starts_with("contained"),
        "{}",
        on.contain_summary
    );

    let auto = start(Network::Off, ContainMode::Auto);
    assert!(auto.refusal.is_none(), "{:?}", auto.refusal);
    assert_eq!(auto.contain_level, ContainLevel::Warn);
    let line = &auto.contain_summary;
    assert!(line.starts_with("contained ("), "{line}");
    assert!(
        line.contains("network off WITH A GAP")
            && line.contains("Operation not permitted")
            && line.contains("UDP and DNS still go out"),
        "{line}"
    );
    assert!(!line.contains("only its own loopback"), "{line}");
    match &auto.confinement {
        Confinement::Confined { sandbox, .. } => assert!(sandbox.containment().is_some()),
        other => panic!("{other:?}"),
    }
    // Contained: its own PID namespace; the host's interfaces (the gap);
    // TCP still refused by Landlock.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let mut ctx = ToolCtx::new(&f.ws);
    ctx.protected = Arc::new(vec![f.data.clone()]);
    ctx.sandbox = auto.confinement.for_session("s1");
    let cmd = format!(
        "echo self=$$; {IDENTITY} (exec 3<>/dev/tcp/127.0.0.1/{port}) && echo connected || \
         echo refused"
    );
    let out = Bash.run(&ctx, json!({ "command": cmd })).await.output;
    assert!(out.contains("self=2"), "not in a PID namespace: {out}");
    assert!(
        out.contains(&host_ns()),
        "the gap is the host's namespace: {out}"
    );
    assert!(
        out.contains("refused") && !out.contains("connected"),
        "{out}"
    );

    let required = start(Network::Off, ContainMode::Strict);
    assert_eq!(required.contain_level, ContainLevel::Refused);
    assert!(required.refusal.is_none(), "the node itself is not refused");
    let why = required.sandbox_refusal().unwrap().to_string();
    assert!(
        why.contains("the probe with --unshare-net failed")
            && why.contains("Operation not permitted"),
        "{why}"
    );
    assert!(matches!(required.confinement, Confinement::Refused(_)));
}

// ---- review T8B-1, T8B-2: the probe that chooses the namespace ----
//
// `docs/security-review/2026-11-T8b-network-off.md`. The stand-ins are the
// real bubblewrap behind a script that changes one thing.

fn stand_in(f: &Fixture, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = f.root.join(name);
    // The node asks bubblewrap's version once (T.8c, which binds it may
    // use): answered by the real one, not counted as a run.
    let real = harness_tools::contain::find_bwrap().unwrap();
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then exec '{}' --version; fi\n{body}",
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// The node's start with `bwrap`, re-probing a gap after `retry`.
fn start_node(
    f: &Fixture,
    network: Network,
    contain: harness_tools::contain::ContainMode,
    bwrap: &Path,
    retry: Duration,
) -> harness_tools::sandbox::Started {
    harness_tools::sandbox::start_with_exe_retrying(
        PathBuf::from(EXE),
        &Settings {
            network,
            contain,
            bwrap: Some(bwrap.to_path_buf()),
            ..Default::default()
        },
        &f.data,
        std::slice::from_ref(&f.data),
        Path::new("/etc/rizzy/node.toml"),
        retry,
    )
    .unwrap()
}

async fn run_in(f: &Fixture, c: &Confinement, cmd: &str) -> String {
    let mut ctx = ToolCtx::new(&f.ws);
    ctx.protected = Arc::new(vec![f.data.clone()]);
    ctx.sandbox = c.clone().for_session("s1");
    Bash.run(&ctx, json!({ "command": cmd })).await.output
}

fn udp_host() -> UdpSocket {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    s
}

/// A command that prints its namespace and sends `tag` to `host`.
fn send_cmd(host: &UdpSocket, tag: &str) -> String {
    let port = host.local_addr().unwrap().port();
    format!("{IDENTITY} printf '{tag}' >/dev/udp/127.0.0.1/{port} && echo sent")
}

/// The stand-in that refuses `--unshare-net` while `flag` exists.
fn refusing_while(f: &Fixture, flag: &Path) -> PathBuf {
    let real = harness_tools::contain::find_bwrap().unwrap();
    stand_in(
        f,
        "bwrap-refuses-while-flagged",
        &format!(
            "for a; do if [ \"$a\" = --unshare-net ] && [ -e '{flag}' ]; then\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi; done\nexec '{real}' \"$@\"\n",
            flag = flag.display(),
            real = real.display()
        ),
    )
}

/// T8B-2: a fresh namespace that lists an interface besides `lo` (what a
/// kernel with a tunnel module loaded makes: `tunl0`, `sit0`) is still a
/// namespace of its own. The real bubblewrap with every option as given;
/// the stand-in swaps a probe script's read of `/proc/net/dev` for a list
/// with `lo` and `tunl0`. It failed before the fix (the probe read that
/// list). Since the fix the probe does not read it, so the stand-in changes
/// nothing and this tests plain bubblewrap: both `auto` and `strict` end
/// with the namespace. It would fail again if the list came back into the
/// probe (review R-5).
#[tokio::test]
async fn an_extra_interface_in_a_new_namespace_still_counts_as_one() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let real = harness_tools::contain::find_bwrap().unwrap();
    let sample = f.root.join("netdev-sample");
    std::fs::write(
        &sample,
        "Inter-|   Receive |  Transmit\n face |bytes    packets\n    lo: 0 0 0 0 0 0 0 0 0 0 0 \
         0 0 0 0 0\n tunl0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
    )
    .unwrap();
    let fake = stand_in(
        &f,
        "bwrap-extra-interface",
        &format!(
            "net=0; for a; do [ \"$a\" = --unshare-net ] && net=1; done\n\
             if [ \"$net\" = 1 ]; then\n\
             cp '{sample}' ./netdev\n\
             n=$#; i=0\n\
             while [ $i -lt $n ]; do a=$1; shift; i=$((i+1))\n\
             case $a in *\"</proc/net/dev\"*) a=$(printf '%s' \"$a\" | sed 's|</proc/net/dev|<./netdev|');; esac\n\
             set -- \"$@\" \"$a\"; done\nfi\nexec '{real}' \"$@\"\n",
            sample = sample.display(),
            real = real.display()
        ),
    );
    for mode in [ContainMode::Auto, ContainMode::Strict] {
        let s = start_node(&f, Network::Off, mode, &fake, Duration::from_secs(60));
        if let Some(why) = s.sandbox_refusal() {
            // Below Landlock ABI 9 (CI's kernel) `strict` does not offer the
            // mode for T.8a's reason, the Unix-socket gap. That is not this
            // test's subject; a refusal about the network namespace is.
            assert!(
                mode == ContainMode::Strict
                    && why.contains("Landlock ABI")
                    && !why.contains("network namespace")
                    && !why.contains("--unshare-net"),
                "{mode:?}: {why}"
            );
            continue;
        }
        assert!(
            s.contain_summary.contains("a network namespace of its own")
                && !s.contain_summary.contains("network off WITH A GAP"),
            "{mode:?}: {}",
            s.contain_summary
        );
        let host = udp_host();
        let out = run_in(&f, &s.confinement, &send_cmd(&host, "extra")).await;
        assert!(own_ns(&out), "{mode:?}: {out}");
        assert!(arrived(&[host])[0].is_empty(), "{mode:?}: {out}");
    }
}

/// T8B-1 (a): a with-namespace probe that fails at start for a moment (the
/// stand-in fails its first two runs with `--unshare-net`, the two forms of
/// one probe) is tried once more before the namespace is given up: the
/// start ends with it.
#[test]
fn two_failed_probes_at_start_still_end_with_the_namespace() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let real = harness_tools::contain::find_bwrap().unwrap();
    let count = f.root.join("count");
    let fake = stand_in(
        &f,
        "bwrap-fails-twice",
        &format!(
            "n=$(cat '{count}' 2>/dev/null || echo 0)\n\
             for a; do if [ \"$a\" = --unshare-net ] && [ \"$n\" -lt 2 ]; then\n\
             echo $((n+1)) >'{count}'\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi; done\nexec '{real}' \"$@\"\n",
            count = count.display(),
            real = real.display()
        ),
    );
    let s = start_node(
        &f,
        Network::Off,
        ContainMode::Auto,
        &fake,
        Duration::from_secs(60),
    );
    assert_eq!(std::fs::read_to_string(&count).unwrap().trim(), "2");
    assert!(
        s.contain_summary.contains("a network namespace of its own")
            && !s.contain_summary.contains("network off WITH A GAP"),
        "{}",
        s.contain_summary
    );
}

/// T8B-1 (b): a node in the gap moves to the namespace, once bubblewrap
/// makes one, at the first contained command after the interval (injected
/// here; 60 s in the node), and never back: with bubblewrap failing again
/// later, the command does not run in the host's network.
#[tokio::test]
async fn a_node_in_the_gap_takes_the_namespace_once_bubblewrap_makes_one() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let flag = f.root.join("refuse");
    std::fs::write(&flag, "").unwrap();
    let fake = refusing_while(&f, &flag);
    let retry = Duration::from_millis(1500);
    let s = start_node(&f, Network::Off, ContainMode::Auto, &fake, retry);
    assert!(
        s.contain_summary.contains("network off WITH A GAP"),
        "{}",
        s.contain_summary
    );

    // Within the interval: still the gap (the host hears it).
    let host = udp_host();
    let out = run_in(&f, &s.confinement, &send_cmd(&host, "gap")).await;
    assert!(out.contains(&host_ns()), "{out}");
    assert_eq!(arrived(std::slice::from_ref(&host))[0], ["gap"], "{out}");

    // bubblewrap makes namespaces again. After the interval the next
    // command starts the probe beside it, in the gap (re-check R-2: it does
    // not wait); the commands started once it passed get the namespace.
    std::fs::remove_file(&flag).unwrap();
    tokio::time::sleep(retry + Duration::from_millis(200)).await;
    let out = run_in(&f, &s.confinement, &send_cmd(&host, "trigger")).await;
    assert!(out.contains("sent"), "{out}");
    let _ = arrived(std::slice::from_ref(&host));
    tokio::time::sleep(Duration::from_millis(1000)).await;
    for round in 0..2 {
        let out = run_in(&f, &s.confinement, &send_cmd(&host, "moved")).await;
        assert!(own_ns(&out), "round {round}: {out}");
        assert!(
            arrived(std::slice::from_ref(&host))[0].is_empty(),
            "round {round}: {out}"
        );
    }

    // bubblewrap fails again: the node does not move back to the gap.
    std::fs::write(&flag, "").unwrap();
    tokio::time::sleep(retry + Duration::from_millis(200)).await;
    let out = run_in(&f, &s.confinement, &send_cmd(&host, "back")).await;
    assert!(!out.contains(&host_ns()), "moved back to the gap: {out}");
    assert!(arrived(&[host])[0].is_empty(), "{out}");
}

/// T8B-1: a node that started with the namespace never runs without it,
/// whatever bubblewrap does later (no probe at run time can take it away).
#[tokio::test]
async fn a_node_with_the_namespace_never_loses_it() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let flag = f.root.join("refuse");
    let fake = refusing_while(&f, &flag);
    let s = start_node(&f, Network::Off, ContainMode::Auto, &fake, Duration::ZERO);
    assert!(
        s.contain_summary.contains("a network namespace of its own"),
        "{}",
        s.contain_summary
    );
    std::fs::write(&flag, "").unwrap();
    let host = udp_host();
    let out = run_in(&f, &s.confinement, &send_cmd(&host, "lost")).await;
    assert!(!out.contains(&host_ns()), "{out}");
    assert!(arrived(&[host])[0].is_empty(), "{out}");
}

/// T8B-2, both forms: the probe's identity check passes in the shared-`/proc`
/// form too (its `/proc/self` is still the reader's), and a command there
/// has its own namespace.
#[tokio::test]
async fn the_shared_proc_form_gets_the_namespace_too() {
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let shared = harness_tools::contain::Contain::new(
        harness_tools::contain::find_bwrap().unwrap(),
        harness_tools::contain::Form::SharedProc,
    );
    let sb = f.plain(Network::Off);
    sb.probe_containment(&shared, &f.data.join("probe-shared"))
        .unwrap();
    let host = udp_host();
    let out = f
        .bash(
            Arc::new(sb.with_containment(shared)),
            &send_cmd(&host, "shared"),
        )
        .await;
    assert!(own_ns(&out), "{out}");
    assert!(arrived(&[host])[0].is_empty(), "{out}");
}

// ---- re-check R-1, R-2: one way up for every step, never on the caller's path ----

/// The stand-in that fails its first `n` runs, whatever the options (as a
/// full process table or no memory would), then is the real bubblewrap.
fn failing_first(f: &Fixture, n: u32) -> (PathBuf, PathBuf) {
    let real = harness_tools::contain::find_bwrap().unwrap();
    let count = f.root.join("runs");
    let fake = stand_in(
        f,
        "bwrap-fails-first-runs",
        &format!(
            "n=$(cat '{count}' 2>/dev/null || echo 0); echo $((n+1)) >'{count}'\n\
             if [ \"$n\" -lt {n} ]; then\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi\nexec '{real}' \"$@\"\n",
            count = count.display(),
            real = real.display()
        ),
    );
    (fake, count)
}

fn runs(count: &Path) -> u32 {
    std::fs::read_to_string(count)
        .map(|s| s.trim().parse().unwrap())
        .unwrap_or(0)
}

/// What a command prints of its PID: 2 (or 3) in a namespace of its own.
fn pid_of(out: &str) -> u32 {
    out.lines()
        .find_map(|l| l.strip_prefix("pid="))
        .and_then(|p| p.split_whitespace().next()?.parse().ok())
        .unwrap_or_else(|| panic!("no pid: {out}"))
}

/// N.10a (replaces re-check R-1's step up from Landlock alone): every
/// probe at start failed (four runs: both forms, with and without the
/// namespace), so the `auto` node does not offer the sandbox mode. Its
/// shell is refused there, never run with Landlock alone, and it does not
/// probe again (nothing would use a containment found later: a node offers
/// its modes from its start). The uncontained twin (the `host` mode's)
/// runs outside any PID namespace, as it always does.
#[tokio::test]
async fn an_auto_node_that_could_not_contain_at_start_offers_no_sandbox_mode() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let (fake, count) = failing_first(&f, 4);
    let retry = Duration::from_millis(500);
    let s = start_node(&f, Network::Off, ContainMode::Auto, &fake, retry);
    assert_eq!(runs(&count), 4);
    assert!(
        s.contain_summary.starts_with("NOT OFFERED"),
        "{}",
        s.contain_summary
    );
    assert!(s.refusal.is_none());
    let why = s.sandbox_refusal().unwrap().to_string();
    let host = udp_host();
    let cmd = |tag: &str| format!("echo pid=$$; {}", send_cmd(&host, tag));
    for round in 0..3 {
        let out = run_in(&f, &s.confinement, &cmd("sandbox")).await;
        assert!(!out.contains("pid="), "round {round}: ran: {out}");
        assert!(out.contains(&why), "round {round}: {out}");
        let out = run_in(&f, &s.uncontained, &cmd("host")).await;
        assert!(pid_of(&out) > 3, "round {round}: contained: {out}");
        tokio::time::sleep(retry + Duration::from_millis(100)).await;
    }
    assert_eq!(
        runs(&count),
        4,
        "a node without the sandbox mode probed again"
    );
    assert!(!arrived(&[host])[0].contains(&"sandbox".to_string()));
}

/// A node that found no bubblewrap at all never probes: one that appears
/// at that path later is not run, and nothing is contained.
#[tokio::test]
async fn a_node_without_bubblewrap_never_probes() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let missing = f.root.join("bwrap-appears-later");
    let s = start_node(
        &f,
        Network::Off,
        ContainMode::Auto,
        &missing,
        Duration::ZERO,
    );
    assert!(
        s.contain_summary.starts_with("NOT OFFERED"),
        "{}",
        s.contain_summary
    );
    let (fake, count) = failing_first(&f, 0);
    std::fs::rename(&fake, &missing).unwrap();
    for _ in 0..3 {
        let out = run_in(&f, &s.uncontained, "echo pid=$$").await;
        assert!(pid_of(&out) > 3, "{out}");
        let out = run_in(&f, &s.confinement, "echo pid=$$").await;
        assert!(!out.contains("pid="), "{out}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(runs(&count), 0, "a node without bubblewrap probed");
}

/// R-1, the twin of `a_node_with_the_namespace_never_loses_it`: a node that
/// contains never goes back to Landlock alone, whatever bubblewrap does
/// later: the command fails, it does not run uncontained.
#[tokio::test]
async fn a_node_that_contains_never_goes_back_to_landlock_alone() {
    use harness_tools::contain::ContainMode;
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let real = harness_tools::contain::find_bwrap().unwrap();
    let flag = f.root.join("broken");
    let fake = stand_in(
        &f,
        "bwrap-breaks-later",
        &format!(
            "if [ -e '{flag}' ]; then echo 'bwrap: Creating new namespace failed: Resource \
             temporarily unavailable' >&2; exit 1; fi\nexec '{real}' \"$@\"\n",
            flag = flag.display(),
            real = real.display()
        ),
    );
    let s = start_node(&f, Network::Off, ContainMode::Auto, &fake, Duration::ZERO);
    assert!(
        s.contain_summary.starts_with("contained"),
        "{}",
        s.contain_summary
    );
    std::fs::write(&flag, "").unwrap();
    let host = udp_host();
    for _ in 0..2 {
        let out = run_in(
            &f,
            &s.confinement,
            &format!("echo pid=$$; {}", send_cmd(&host, "x")),
        )
        .await;
        assert!(!out.contains("pid="), "ran without bubblewrap: {out}");
        assert!(out.contains("Creating new namespace failed"), "{out}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(arrived(&[host])[0].is_empty());
}

/// Live processes whose command line is exactly `sleep <arg>`.
fn sleeping(arg: &str) -> Vec<i32> {
    let want = format!("sleep\0{arg}\0");
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| c == want.as_bytes()))
        .collect()
}

/// R-2: a later probe that hangs (the stand-in sleeps 6 s before it runs a
/// probe with `--unshare-net`) holds no command: the one that noticed the
/// probe was due returns within its own 1 s timeout, in the mode the node
/// was in. The probe is bounded: it is killed, counted as failed, and a
/// later probe still runs and steps up.
#[tokio::test]
async fn a_hung_later_probe_holds_no_command() {
    use harness_tools::contain::ContainMode;
    const SLEEP: &str = "6.0731";
    let f = fixture();
    if f.contained(Network::Off).is_none() {
        return;
    }
    let real = harness_tools::contain::find_bwrap().unwrap();
    let refuse = f.root.join("refuse");
    let slow = f.root.join("slow");
    let fake = stand_in(
        &f,
        "bwrap-slow-probe",
        &format!(
            "for a; do if [ \"$a\" = --unshare-net ]; then\n\
             if [ -e '{refuse}' ]; then echo 'bwrap: Creating new namespace failed: Resource \
             temporarily unavailable' >&2; exit 1; fi\n\
             if [ -e '{slow}' ]; then sleep {SLEEP}; fi; fi; done\nexec '{real}' \"$@\"\n",
            refuse = refuse.display(),
            slow = slow.display(),
            real = real.display()
        ),
    );
    std::fs::write(&refuse, "").unwrap();
    let retry = Duration::from_millis(1000);
    let s = start_node(&f, Network::Off, ContainMode::Auto, &fake, retry);
    assert!(
        s.contain_summary.contains("network off WITH A GAP"),
        "{}",
        s.contain_summary
    );
    std::fs::remove_file(&refuse).unwrap();
    std::fs::write(&slow, "").unwrap();
    tokio::time::sleep(retry + Duration::from_millis(100)).await;

    // A: the probe is due; A runs at once in the gap, within its timeout.
    let mut ctx = ToolCtx::new(&f.ws);
    ctx.protected = Arc::new(vec![f.data.clone()]);
    ctx.sandbox = s.confinement.clone().for_session("s1");
    let started = std::time::Instant::now();
    let a = Bash
        .run(
            &ctx,
            json!({"command": format!("{IDENTITY} echo hi"), "timeout_secs": 1}),
        )
        .await
        .output;
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "A waited {:?} for the probe: {a}",
        started.elapsed()
    );
    assert!(a.contains("hi") && a.contains(&host_ns()), "{a}");

    // The hung probe is killed at its bound (well before its 6 s).
    let by = std::time::Instant::now() + Duration::from_secs(4);
    while sleeping(SLEEP).is_empty() && std::time::Instant::now() < by {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let by = std::time::Instant::now() + Duration::from_secs(4);
    while !sleeping(SLEEP).is_empty() {
        assert!(
            std::time::Instant::now() < by,
            "the hung probe was not killed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // bubblewrap answers again: a later probe runs and the node steps up.
    std::fs::remove_file(&slow).unwrap();
    tokio::time::sleep(retry + Duration::from_millis(100)).await;
    let _ = run_in(&f, &s.confinement, "true").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let out = run_in(&f, &s.confinement, IDENTITY).await;
    assert!(own_ns(&out), "no later probe after the hung one: {out}");
}
