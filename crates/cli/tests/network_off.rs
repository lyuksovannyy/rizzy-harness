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
    let mut cmd = String::from(INTERFACES);
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
    let out = f.bash(sb, &format!("{INTERFACES} python3 -c '{py}'")).await;
    assert!(out.contains("ifs=[lo ]"), "{out}");
    assert!(out.contains("udp-self self"), "loopback is not up: {out}");
    assert!(out.contains("tcp-bind PermissionError"), "{out}");
}

// ---- DNS ----

/// Network off: a DNS lookup fails, and it cannot leave the machine: it
/// runs only once the command is seen to have no interface but `lo` (on
/// the old path the command prints `lookup-skipped` and this fails without
/// sending anything). The name is under `.invalid` (RFC 6761) for good
/// measure.
#[tokio::test]
async fn network_off_dns_lookups_fail() {
    let f = fixture();
    let Some(sb) = f.contained(Network::Off) else {
        return;
    };
    let cmd = format!(
        "{INTERFACES} if [ \"$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev)\" = lo ]; then \
         getent hosts rizzy-t8b-probe.invalid; echo \"lookup-exit=$?\"; \
         getent hosts localhost >/dev/null; echo \"files-exit=$?\"; \
         else echo lookup-skipped; fi"
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
        let mut cmd = format!("{INTERFACES}{}", udp_probe(&listeners, tag));
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
    assert!(!after.contains("abstract-ECONNREFUSED"), "{after}");
    drop(abs);
}

// ---- what the node says, and what it does when it cannot ----

/// `doctor`'s containment line with the network off: contained, it names
/// the namespace; on the fallback and with `contain = "off"` it says that
/// UDP and DNS still go out.
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
        "[sandbox]\nnetwork = \"off\"\ncontain = \"off\"\n",
    ] {
        let c = doctor(toml);
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
/// a gap naming bubblewrap's error; `required` refuses with the reason.
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
        "echo self=$$; {INTERFACES} (exec 3<>/dev/tcp/127.0.0.1/{port}) && echo connected || \
         echo refused"
    );
    let out = Bash.run(&ctx, json!({ "command": cmd })).await.output;
    assert!(out.contains("self=2"), "not in a PID namespace: {out}");
    assert!(!out.contains("ifs=[lo ]"), "{out}");
    assert!(out.contains("refused") && !out.contains("connected"), "{out}");

    let required = start(Network::Off, ContainMode::Required);
    assert_eq!(required.contain_level, ContainLevel::Refused);
    let why = required.refusal.unwrap();
    assert!(
        why.contains("network namespace") && why.contains("Operation not permitted"),
        "{why}"
    );
    assert!(matches!(required.confinement, Confinement::Refused(_)));
}
