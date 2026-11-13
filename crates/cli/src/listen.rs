//! The UI's listeners: every loopback address the server accepts as its own
//! origin is one it holds (security review S2-1).
//!
//! The server accepts `localhost:<port>`, `127.0.0.1:<port>` and
//! `[::1]:<port>` in `Host` and `Origin` when it binds a loopback or a
//! wildcard address. Browsers resolve `localhost` to `::1` first. If the
//! server held only `127.0.0.1:<port>`, a program of the same user (an
//! agent's shell command) could bind `[::1]:<port>` and serve
//! `http://localhost:<port>`: an origin the server calls its own, whose
//! `localStorage` holds the login key. So the server binds the other family
//! of its port too, and refuses to start when another program holds it.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Context, bail};
use tokio::net::TcpListener;

/// The bound UI.
pub struct Ui {
    /// The address asked for, as bound (port 0 resolved): the one printed.
    pub addr: SocketAddr,
    /// Every listener, `addr`'s first; one app serves them all.
    pub listeners: Vec<TcpListener>,
    /// The loopback addresses this server holds (by `addr` or by a listener
    /// beside it): the only ones accepted as its own besides `addr`.
    pub own: Vec<SocketAddr>,
}

/// The loopback addresses a browser reaches for `localhost:<port>`.
fn loopbacks(port: u16) -> [SocketAddr; 2] {
    [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ]
}

/// Whether a socket bound to `bind` receives connections to `a`.
/// `v6only` is `IPV6_V6ONLY` of an IPv6 wildcard socket.
fn covers(bind: SocketAddr, a: SocketAddr, v6only: bool) -> bool {
    if bind == a {
        return true;
    }
    if !bind.ip().is_unspecified() {
        return false;
    }
    match (bind, a) {
        (SocketAddr::V4(_), SocketAddr::V4(_)) => true,
        (SocketAddr::V6(_), SocketAddr::V6(_)) => true,
        // A dual-stack `[::]` takes IPv4 as mapped addresses.
        (SocketAddr::V6(_), SocketAddr::V4(_)) => !v6only,
        (SocketAddr::V4(_), SocketAddr::V6(_)) => false,
    }
}

/// What `bind` (as bound) must hold besides itself: `(covered, also)`, the
/// loopback addresses it covers and those to bind beside it. A bind that is
/// neither loopback nor a wildcard (a WireGuard or LAN address) accepts no
/// loopback name and needs neither.
pub fn plan(bind: SocketAddr, v6only: bool) -> (Vec<SocketAddr>, Vec<SocketAddr>) {
    if !(bind.ip().is_loopback() || bind.ip().is_unspecified()) {
        return (Vec::new(), Vec::new());
    }
    loopbacks(bind.port())
        .into_iter()
        .partition(|a| covers(bind, *a, v6only))
}

/// What a failed bind of an address beside the asked one means.
#[derive(Debug, PartialEq, Eq)]
pub enum Beside {
    /// The machine has no such address or family (no IPv6): nothing else
    /// can bind it either, so the server starts without it and does not
    /// accept it as its own.
    Unavailable,
    /// Another program holds it: that program would answer for an origin
    /// this server calls its own.
    InUse,
    /// Anything else: the start fails with it.
    Other,
}

pub fn beside(e: &io::Error) -> Beside {
    use nix::errno::Errno;
    match e.raw_os_error().map(Errno::from_raw) {
        Some(Errno::EADDRNOTAVAIL | Errno::EAFNOSUPPORT | Errno::EPROTONOSUPPORT) => {
            Beside::Unavailable
        }
        Some(Errno::EADDRINUSE) => Beside::InUse,
        _ => Beside::Other,
    }
}

fn in_use(a: SocketAddr, port: u16) -> String {
    format!(
        "refusing to start: {a} is in use by another program. This server accepts {a} and \
         localhost:{port} as its own address, and browsers try both: a browser opening \
         http://localhost:{port} would reach that program instead of this server, and could \
         hand it your login. Stop that program, or pick another port (--bind)"
    )
}

/// How often a port the system chose (port 0) is chosen again when its
/// other family is taken: nobody asked for that port.
const PORT0_TRIES: usize = 16;

/// Bind `bind` and every loopback address it accepts as its own.
pub async fn bind(bind: SocketAddr) -> anyhow::Result<Ui> {
    let tries = if bind.port() == 0 { PORT0_TRIES } else { 1 };
    'retry: for _ in 0..tries {
        let first = TcpListener::bind(bind)
            .await
            .with_context(|| format!("binding {bind}"))?;
        let addr = first.local_addr()?;
        let v6only = addr.is_ipv6()
            && addr.ip().is_unspecified()
            && nix::sys::socket::getsockopt(&first, nix::sys::socket::sockopt::Ipv6V6Only)
                .context("reading IPV6_V6ONLY")?;
        let (mut own, also) = plan(addr, v6only);
        let mut listeners = vec![first];
        for a in also {
            match TcpListener::bind(a).await {
                Ok(l) => {
                    listeners.push(l);
                    own.push(a);
                }
                Err(e) => match beside(&e) {
                    Beside::Unavailable => tracing::info!(
                        "not listening on {a}: this machine has none ({e}); it is not accepted \
                         as this server's address either"
                    ),
                    Beside::InUse if bind.port() == 0 => continue 'retry,
                    Beside::InUse => bail!(in_use(a, addr.port())),
                    Beside::Other => return Err(e).with_context(|| format!("binding {a}")),
                },
            }
        }
        own.sort();
        let all: Vec<String> = listeners
            .iter()
            .filter_map(|l| l.local_addr().ok())
            .map(|a| a.to_string())
            .collect();
        tracing::info!("listening on {}", all.join(" and "));
        return Ok(Ui {
            addr,
            listeners,
            own,
        });
    }
    bail!(
        "no port free on both 127.0.0.1 and [::1] after {PORT0_TRIES} tries; pass a port with \
         --bind"
    )
}

/// `Host` values accepted as this server's own: the bound address, the
/// loopback addresses it holds and `localhost` with them, and `extra`.
pub fn allowed_hosts(addr: SocketAddr, own: &[SocketAddr], extra: &[String]) -> Vec<String> {
    let mut v = vec![addr.to_string()];
    v.extend(own.iter().map(SocketAddr::to_string));
    if !own.is_empty() {
        v.push(format!("localhost:{}", addr.port()));
    }
    v.extend(extra.iter().cloned());
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn what_each_bind_holds_beside_itself() {
        let (v4, v6) = (a("127.0.0.1:7777"), a("[::1]:7777"));
        assert_eq!(plan(v4, false), (vec![v4], vec![v6]));
        assert_eq!(plan(v6, false), (vec![v6], vec![v4]));
        // Another IPv4 loopback address covers neither.
        assert_eq!(plan(a("127.0.0.5:7777"), false), (vec![], vec![v4, v6]));
        // The IPv4 wildcard covers 127.0.0.1, not ::1: it binds [::1] (never
        // [::], which would widen the exposure).
        assert_eq!(plan(a("0.0.0.0:7777"), false), (vec![v4], vec![v6]));
        // A dual-stack [::] covers both; an IPv6-only one needs 127.0.0.1.
        assert_eq!(plan(a("[::]:7777"), false), (vec![v4, v6], vec![]));
        assert_eq!(plan(a("[::]:7777"), true), (vec![v6], vec![v4]));
        // A WireGuard or LAN address: no loopback name, nothing beside.
        for other in ["10.8.0.1:7777", "192.0.2.1:7777", "[fd00::1]:7777"] {
            assert_eq!(plan(a(other), false), (vec![], vec![]), "{other}");
        }
    }

    #[test]
    fn a_failed_bind_beside_is_judged_by_its_error() {
        use nix::errno::Errno;
        let e = |n: Errno| io::Error::from_raw_os_error(n as i32);
        // No IPv6 on the machine: lo without ::1, or the family switched off.
        assert_eq!(beside(&e(Errno::EADDRNOTAVAIL)), Beside::Unavailable);
        assert_eq!(beside(&e(Errno::EAFNOSUPPORT)), Beside::Unavailable);
        assert_eq!(beside(&e(Errno::EADDRINUSE)), Beside::InUse);
        assert_eq!(beside(&e(Errno::EACCES)), Beside::Other);
        assert_eq!(beside(&e(Errno::EMFILE)), Beside::Other);
        assert_eq!(beside(&io::Error::other("x")), Beside::Other);
        let m = in_use(a("[::1]:7777"), 7777);
        assert!(m.contains("[::1]:7777 is in use"), "{m}");
        assert!(
            m.contains("http://localhost:7777 would reach that program instead of this server"),
            "{m}"
        );
    }

    #[test]
    fn hosts_accepted_are_the_ones_held() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (v4, v6) = (a("127.0.0.1:7777"), a("[::1]:7777"));
        assert_eq!(
            allowed_hosts(v4, &[v4, v6], &[]),
            s(&["127.0.0.1:7777", "[::1]:7777", "localhost:7777"])
        );
        // No IPv6 here: [::1] is not this server's, localhost still is
        // (nothing can bind ::1 either).
        assert_eq!(
            allowed_hosts(v4, &[v4], &[]),
            s(&["127.0.0.1:7777", "localhost:7777"])
        );
        assert_eq!(
            allowed_hosts(a("0.0.0.0:7777"), &[v4, v6], &[]),
            s(&[
                "0.0.0.0:7777",
                "127.0.0.1:7777",
                "[::1]:7777",
                "localhost:7777"
            ])
        );
        // A WireGuard address: no loopback name at all.
        assert_eq!(
            allowed_hosts(a("10.8.0.1:7777"), &[], &s(&["chat.example:443"])),
            s(&["10.8.0.1:7777", "chat.example:443"])
        );
    }

    /// The real bind on this machine: both families on one port, and a
    /// taken `[::1]` of an asked port refuses.
    #[tokio::test]
    async fn binds_both_families_of_one_port() {
        if std::net::TcpListener::bind("[::1]:0").is_err() {
            eprintln!("skipped: this machine has no IPv6 loopback");
            return;
        }
        let ui = bind(a("127.0.0.1:0")).await.unwrap();
        let port = ui.addr.port();
        let got: Vec<_> = ui
            .listeners
            .iter()
            .map(|l| l.local_addr().unwrap())
            .collect();
        let both = vec![
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
        ];
        assert_eq!(got, both);
        assert_eq!(ui.own, both);
        // What another program of the same user meets: tokio sets
        // SO_REUSEADDR (which Linux ignores against a listening socket),
        // never SO_REUSEPORT (which would let it share the port).
        use nix::sys::socket::{getsockopt, sockopt};
        for l in &ui.listeners {
            let reuse = (
                getsockopt(l, sockopt::ReuseAddr).unwrap(),
                getsockopt(l, sockopt::ReusePort).unwrap(),
            );
            eprintln!(
                "{}: SO_REUSEADDR, SO_REUSEPORT = {reuse:?}",
                l.local_addr().unwrap()
            );
            assert_eq!(reuse, (true, false));
        }
        drop(ui);
        let taken = std::net::TcpListener::bind("[::1]:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
            eprintln!("skipped the taken case: 127.0.0.1:{port} is in use");
            return;
        }
        let e = bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await
            .err()
            .expect("a start beside a taken [::1]");
        assert!(
            e.to_string().contains("is in use by another program"),
            "{e}"
        );
    }
}
