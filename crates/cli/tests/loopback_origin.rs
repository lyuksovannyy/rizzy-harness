//! The UI's loopback origins are the server's own (security review S2-1),
//! on the real binary. The server accepts `localhost:<port>`,
//! `127.0.0.1:<port>` and `[::1]:<port>` as its own `Host`/`Origin`, and
//! browsers resolve `localhost` to `::1` first. A program of the same user
//! (an agent's shell command) that could bind `[::1]:<port>` would serve
//! `http://localhost:<port>`: an origin whose `localStorage` holds the login
//! key. So the server holds `[::1]:<port>` beside `127.0.0.1:<port>`, and a
//! start whose `[::1]:<port>` is taken is refused.
//!
//! Every test here needs an IPv6 loopback and says so when it skips.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// Whether this machine has `::1` (CI normally does; a machine booted with
/// `ipv6.disable=1` does not).
fn ipv6_loopback() -> bool {
    match std::net::TcpListener::bind("[::1]:0") {
        Ok(_) => true,
        Err(e) => {
            eprintln!("skipped: this machine has no IPv6 loopback ({e})");
            false
        }
    }
}

/// `<root>/data`, demo provider, under this crate's target directory.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("loopback-origin")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!("workspace_roots = [{:?}]\n", ws.display().to_string()),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
    (dir, data)
}

/// A `serve`, killed (by its own pid) and reaped when dropped.
struct Server {
    child: Child,
    addr: SocketAddr,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(data: &Path, bind: &str) -> Server {
    let mut child = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", bind])
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn harness");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let addr = loop {
        let Some(Ok(line)) = lines.next() else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the server ended before it listened");
        };
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.trim().parse().unwrap();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    Server { child, addr }
}

/// `serve --bind <bind>` on `data`: its exit status and stderr, or `None`
/// when it was still running after `within` (then killed by its pid).
fn run(data: &Path, bind: &str, within: Duration) -> Option<(std::process::ExitStatus, String)> {
    let mut child = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", bind])
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn harness");
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if started.elapsed() > within {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    status.map(|s| (s, err))
}

#[derive(Clone, Copy, Debug)]
enum Opt {
    Plain,
    ReuseAddr,
    ReusePort,
    ReuseBoth,
    V6Only,
}

/// Bind and listen on `addr` the way another program could, with `opt`
/// set first. The socket is closed again before this returns.
fn try_listen(addr: SocketAddr, opt: Opt) -> nix::Result<()> {
    use nix::sys::socket::{
        AddressFamily, Backlog, SockFlag, SockType, SockaddrStorage, bind, listen, setsockopt,
        socket, sockopt,
    };
    let family = if addr.is_ipv6() {
        AddressFamily::Inet6
    } else {
        AddressFamily::Inet
    };
    let fd = socket(family, SockType::Stream, SockFlag::SOCK_CLOEXEC, None)?;
    match opt {
        Opt::Plain => {}
        Opt::ReuseAddr => setsockopt(&fd, sockopt::ReuseAddr, &true)?,
        Opt::ReusePort => setsockopt(&fd, sockopt::ReusePort, &true)?,
        Opt::ReuseBoth => {
            setsockopt(&fd, sockopt::ReuseAddr, &true)?;
            setsockopt(&fd, sockopt::ReusePort, &true)?;
        }
        Opt::V6Only => setsockopt(&fd, sockopt::Ipv6V6Only, &true)?,
    }
    bind(fd.as_raw_fd(), &SockaddrStorage::from(addr))?;
    listen(&fd, Backlog::new(8)?)?;
    Ok(())
}

async fn page(url: &str) -> (u16, String) {
    let r = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: {e}"));
    (r.status().as_u16(), r.text().await.unwrap())
}

/// The abuse case: once the server listens on `127.0.0.1:<port>`, another
/// process of the same user cannot take `[::1]:<port>` (plainly, with
/// `SO_REUSEADDR`, with `SO_REUSEPORT`, or as an IPv6-only wildcard), and
/// `http://[::1]:<port>/` is the server's own page.
#[tokio::test]
async fn the_server_holds_the_ipv6_loopback_of_its_port() {
    if !ipv6_loopback() {
        return;
    }
    let (_dir, data) = setup();
    let s = start(&data, "127.0.0.1:0");
    let port = s.addr.port();
    let v6: SocketAddr = format!("[::1]:{port}").parse().unwrap();
    let any6: SocketAddr = format!("[::]:{port}").parse().unwrap();
    let tries = [
        (v6, Opt::Plain),
        (v6, Opt::ReuseAddr),
        (v6, Opt::ReusePort),
        (v6, Opt::ReuseBoth),
        (any6, Opt::V6Only),
        (any6, Opt::Plain),
    ];
    let mut taken = Vec::new();
    for (a, opt) in tries {
        let r = try_listen(a, opt);
        eprintln!("another process binds {a} with {opt:?}: {r:?}");
        if r.is_ok() {
            taken.push(format!("{a} {opt:?}"));
        }
    }
    assert!(
        taken.is_empty(),
        "another process took the server's IPv6 loopback: {taken:?}"
    );
    let (st4, own) = page(&format!("http://127.0.0.1:{port}/")).await;
    let (st6, got) = page(&format!("http://[::1]:{port}/")).await;
    let (stl, by_name) = page(&format!("http://localhost:{port}/")).await;
    assert_eq!((st4, st6, stl), (200, 200, 200));
    assert_eq!(got, own, "[::1]:{port} serves another page");
    assert_eq!(by_name, own, "localhost:{port} serves another page");

    // A stop (SIGTERM, A.19) ends both listeners with the server.
    let mut s = s;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(s.child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let by = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(st) = s.child.try_wait().unwrap() {
            break st;
        }
        assert!(Instant::now() < by, "the server did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status}");
    for a in [format!("127.0.0.1:{port}"), format!("[::1]:{port}")] {
        std::net::TcpListener::bind(&a).unwrap_or_else(|e| panic!("{a} still held: {e}"));
    }
}

/// A program already holding `[::1]:<port>` stops `serve --bind
/// 127.0.0.1:<port>`: it exits non-zero, names the address and why it
/// matters, and opens nothing in the data directory (no node, no
/// database); the next start on the same directory is not blocked by a
/// lock it left.
#[test]
fn a_taken_ipv6_loopback_port_stops_the_start() {
    if !ipv6_loopback() {
        return;
    }
    let (_dir, data) = setup();
    // A port free on 127.0.0.1 whose [::1] side this test holds.
    let (blocker, port) = (0..32)
        .find_map(|_| {
            let l = std::net::TcpListener::bind("[::1]:0").unwrap();
            let port = l.local_addr().unwrap().port();
            std::net::TcpListener::bind(("127.0.0.1", port))
                .ok()
                .map(|_| (l, port))
        })
        .expect("a port free on both families");
    let bind = format!("127.0.0.1:{port}");
    let (status, err) = run(&data, &bind, Duration::from_secs(30))
        .expect("the server started although [::1] of its port is taken");
    eprintln!("{err}");
    assert!(!status.success(), "{status}");
    assert!(err.contains(&format!("[::1]:{port}")), "{err}");
    assert!(
        err.contains(&format!("http://localhost:{port} would reach that program")),
        "{err}"
    );
    for f in ["node.db", "master.db", "chat.db", "api-token"] {
        assert!(!data.join(f).exists(), "{f} was created by a refused start");
    }
    drop(blocker);
    let s = start(&data, &bind);
    assert_eq!(s.addr.port(), port);
}
