# A loopback origin the server accepted but did not hold

Date: 2026-11-13. Finding S2-1 (Medium) of an independent review, proven with a script; fixed
on the branch `fix/loopback-both-families`. Threat model: `PLAN.md` T37 (and T1, T2).

## What was wrong

`serve` bound one address, `127.0.0.1:<port>` by default (`crates/cli/src/main.rs`), but its
`Host`/`Origin` allowlist also took `localhost:<port>` and `[::1]:<port>` for any loopback or
wildcard bind. Nothing listened on `[::1]:<port>`, and an IPv4 wildcard (`0.0.0.0`) does not
cover it either. Browsers resolve `localhost` to `::1` first.

An agent's shell command runs as the same user with the host's network when the network is
on (the default): a foreground command in an `auto` turn or an approved one, or a background
command where a node owner turned them on. Such a command could bind `[::1]:<port>` and stay
there. `http://localhost:<port>` was then its page. The reviewer's proof: master on
`127.0.0.1:32895`, a command on `[::1]:32895`, `http://localhost:32895/` returned the
command's page; `getaddrinfo("localhost")[0]` was `::1`.

## Since when

The allowlist with `localhost` and `[::1]` and the single bind date from the first master
(`0e81fe5`, 2026-11-02). The origin became worth taking with the login key kept in that
origin's `localStorage` (`7cbffd1`, Gate 3 P-1, 2026-11-11), and a command able to stay
running after its call came with T.8d (`60c3b99`, 2026-11-13; off by default).

## What an attacker gained

For a person who opens `http://localhost:<port>`: a page in the origin the real UI uses, so a
script there reads the login key from `localStorage`, and the browser sends it the
host-scoped session cookie. Both values together are a login (T2). For anyone else: a page at
an address the server itself prints as its own, a perfect place for a fake login form.

## The fix

The server holds every loopback address it accepts (`crates/cli/src/listen.rs`):

- A loopback bind also binds the other of `127.0.0.1:<port>` and `[::1]:<port>` (an other
  `127.x` address binds both). `0.0.0.0` also binds `[::1]:<port>`, never `[::]`. `[::]`
  covers both when dual-stack (`IPV6_V6ONLY` is read from the socket; Linux defaults to
  dual-stack, `net.ipv6.bindv6only = 0`) and binds `127.0.0.1:<port>` when it is IPv6-only.
  A WireGuard or LAN bind accepts no loopback name and is unchanged.
- The binds are the first thing `serve` does, before any runtime or node starts.
- `EADDRINUSE` on the second bind refuses the start, naming the address and that
  `http://localhost:<port>` would reach that program instead of this server; nothing in the
  data directory is opened. No flag overrides it. With port 0 the port is chosen again, at
  most 16 times.
- `EADDRNOTAVAIL`, `EAFNOSUPPORT` or `EPROTONOSUPPORT` (no IPv6 on the machine) start without
  it, with one log line, and `[::1]:<port>` is then not an allowed host; `localhost:<port>`
  stays (nothing can bind `::1` either). Any other error fails the start.
- The allowed hosts are built from what is held, not from the bind's kind.
- One serve loop accepts from every listener, in turn (`crates/master/src/serve.rs`): one
  connection cap, one shutdown, TLS or not. A stop (SIGTERM) closes both.

What another process of the same user meets: the server's sockets have `SO_REUSEADDR` set
(tokio, through mio; Linux ignores it against a listening socket) and `SO_REUSEPORT` not set
(Linux shares a port only between sockets that all set it). Measured: binding `[::1]:<port>`
plainly, with `SO_REUSEADDR`, `SO_REUSEPORT` or both, and `[::]:<port>` IPv6-only or
dual-stack, all fail with `EADDRINUSE`. Before the fix all but the dual-stack `[::]` succeeded.

## Tests

- `crates/cli/tests/loopback_origin.rs`, on the real binary, each failing before the fix:
  `the_server_holds_the_ipv6_loopback_of_its_port` (the six binds above, `http://[::1]:<port>/`
  and `http://localhost:<port>/` serve the server's own page, a SIGTERM frees both) and
  `a_taken_ipv6_loopback_port_stops_the_start` (exit non-zero with the message, no database or
  token file created, the next start on the same directory works). Both skip with a printed
  reason on a machine without `::1`.
- `crates/cli/src/listen.rs`: what each bind holds beside itself (`0.0.0.0` and `[::]` are
  tested here only: a real bind would expose a port on the network), the decision on the bind
  error (a machine without IPv6 cannot be made here), the allowed hosts without `[::1]`, and
  the socket options of the real listeners.
- `crates/master/tests/server_limits.rs`
  `two_listeners_share_the_app_the_cap_and_the_shutdown`.

## What is left

- While the server is down, any program of the user can hold its port; a program that holds
  `[::1]:<port>` before a start now blocks the start (it is named, and the person decides).
- A container published only with `-p 127.0.0.1:7777:7777` leaves the host's `[::1]:7777` to
  whoever binds it; the README says to publish `[::1]` too or to open `127.0.0.1` only.
  `compose.yaml` uses host networking and is covered by the fix.
- `--allowed-host` names are the owner's to hold: a name that resolves to an address the server
  does not listen on has the same shape.
- The `--tls` certificate names `localhost` and `127.0.0.1`, not `::1`: `https://[::1]:<port>`
  shows a certificate warning (no hole: it is the server's own listener).
- The node listener (`--node-listen`) binds one address too, but a node trusts the master's
  pinned certificate, not a name: a program on the other family can only fail the handshake.
