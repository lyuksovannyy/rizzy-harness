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

## Re-check, 2026-11-13

By the reviewer who reported S2-1, against `2244ca5` on this branch's own build. No product
code and no test of the builder's was changed. The proof is `step2_loopback.py`, beside the
other step 2 proofs on `feat/T.8d-step2-stop`
(`crates/cli/tests/support/review_t8d/`); it starts the real binary on ports the kernel
picks, with its folders under this worktree's `target/review_t8d_step2/`:

```text
RIZZY=$PWD/target/debug/rizzy-harness T8D_TARGET=$PWD/target \
  python3 -I <step 2 worktree>/crates/cli/tests/support/review_t8d/step2_loopback.py [original|held|start|stop|tls|binds|slow|down|netoff|prebound|all]
```

The builder's tests at this head: `cargo test -p harness --test loopback_origin` 2 passed;
`cargo test -p harness --bin rizzy-harness listen::` 4 passed;
`cargo test -p harness-master --test server_limits` 14 passed.

### Verdict

**Fixed, with a remainder.** While the server runs, no program of the same user, and no
command of a node, can take any address of the server's port that a name the server accepts
leads to. The first proof fails against this build and still succeeds against the build
before it. What is left is the time the server is **down** (R-A, below): this note names it,
but not who can be there, and that matters.

### The finding as reported

The proof of S2-1 again, a background command of the node serving on `[::1]:<port>` (the
feature turned on in `node.toml`, since this branch is `main`):

```text
this build:        its result ends: OSError: [Errno 98] Address already in use / [exit code: 1]
                   started as a background command: []; listening on the port: ['127.0.0.1:42783', '[::1]:42783']; http://localhost:42783/ is answered by: the server
before the fix:    started as a background command: ['bg-1']; listening on the port: ['127.0.0.1:39277', '[::1]:39277']; http://localhost:39277/ is answered by: ANOTHER PROGRAM
```

### 1. Every way to take the port while the server runs

From this script and, the same file, from a command of the server's own node (bubblewrap,
the network on); the two lists are identical:

```text
--bind 127.0.0.1:0 -> port 42107; listening sockets on it: ['127.0.0.1:42107', '[::1]:42107']
[::1] plain / SO_REUSEADDR / SO_REUSEPORT / both           EADDRINUSE (each)
127.0.0.1 plain / SO_REUSEADDR / SO_REUSEPORT / both       EADDRINUSE (each)
[::] IPv6 only                               EADDRINUSE
[::] dual stack                              EADDRINUSE
0.0.0.0                                      EADDRINUSE
[::ffff:127.0.0.1] (IPv4-mapped)             EADDRINUSE
127.0.0.2 (another loopback address)         BOUND
UDP [::1]                                    BOUND
UDP 127.0.0.1                                BOUND
```

The three that bind lead nowhere the server accepts. `127.0.0.2:<port>` is another origin
and its `Host` is refused. A UDP socket matters only to HTTP/3, which a browser tries after
the origin said so over TCP (`Alt-Svc`) or in DNS: the server's page sends no `Alt-Svc`
(checked), and `localhost` has no DNS record. A socket bound with `SO_REUSEADDR` and not
listening, there **before** the start, does not stop the start and cannot listen afterwards
(`listen()`: `EADDRINUSE`; `http://[::1]:<port>/` is the server's page).

The names, over both listeners (`200` taken, `403` refused by the guard):

```text
taken:   127.0.0.1:42107; [::1]:42107; localhost:42107; LOCALHOST:42107; " localhost:42107" (a leading space)
refused: localhost.:42107; foo.localhost:42107; ip6-localhost:42107; ip6-loopback:42107; 127.0.0.2:42107; 127.1:42107;
         2130706433:42107; 0.0.0.0:42107; [::]:42107; [::ffff:127.0.0.1]:42107; [0:0:0:0:0:0:0:1]:42107; [::1%lo]:42107;
         localhost; 127.0.0.1; localhost:42107.; localhost:042107; <this machine's name>:42107; evil.example:42107
a cookie write with Origin localhost / [::1] / 127.0.0.2 / localhost. / foo.localhost: [200, 200, 403, 403, 403]
```

The guard takes exactly the three names the server holds, compared without case. Every
other spelling of a loopback address is refused, whichever listener it arrives on, so none
needs holding.

### 2. The start

```text
another program holds [::1]:49649 before the start: exit 1 after 0.01 s
   it says: refusing to start: [::1]:49649 is in use by another program. This server accepts [::1]:49649 and localhost:49649 as its own address, and browsers try both: a browser opening http://localhost:49649 would reach that program instead of this server, and could hand it your login. Stop that program, or pick another port (--bind)
   the data directory afterwards: ['config.toml', 'lock', 'node.toml']; its lock is free: True
   15 starts in a row against the held port: exit codes [1] in 0.1 s; the data directory: ['config.toml', 'lock', 'node.toml']; processes of it left: 0
   the program gives the port back: the next start holds ['127.0.0.1:49649', '[::1]:49649']; http://localhost:49649/ is the server's: True
another program holds 127.0.0.1:47791 before the start: exit 1 after 0.01 s (binding 127.0.0.1:47791: Address already in use)
12 starts with a program racing for [::1]:<port> from the first bind on: the program got it 12 time(s), and the start was refused each of those times: True
4 starts on port 0: both families of the chosen port held in 4
```

The window between the two binds exists and a program that watches for it wins it (10 of 12
and 12 of 12 in two runs); each time the start is refused, so nothing is served with one
address missing. A restart loop does no harm: an exit in 10 ms, the lock released, nothing
written. One correction to "nothing in the data directory is opened" above: the `lock` file
is made and taken before the bind (`main.rs`, `lock::take`); it is free again at once, and
no database or token is made.

### 3. The stop, and the cap

```text
520 idle connections on [::1] (the cap is 512 for the whole server), then a request on 127.0.0.1: 200 after 8.5 s (a connection's head must come within 10 s)
300 on each at once: the server holds 537 descriptors (600 connections would be over the cap of 512 if each listener had its own)
8 threads opening and closing connections on [::1] while 127.0.0.1 is asked 40 times: 40 answered 200, slowest 1 ms
SIGTERM with an idle and a half-sent connection open on [::1] and an idle one on 127.0.0.1: exit 0 after 10.1 s; listening afterwards: []; a connect is refused on [::1]: True, on 127.0.0.1: True
   the data directory's lock is free: True; processes of it left: 0
```

One cap for both, not two. Idle connections on one listener hold the other up for as long
as one listener's own idle connections always did (the 10 s for a request's head), and a
stream of short connections on one does not delay the other. The 10 s of the stop are the
half-sent request's, and the same before the fix (one such connection on `127.0.0.1`,
SIGTERM: 9.7 s on the build before the fix, 9.7 s on this one; an idle one: 0.4 s on both).

### 4. `--tls`, other binds, the wildcards

```text
--tls: listening ['127.0.0.1:44781', '[::1]:44781']; TLS and a 200 on each; plain HTTP on each: no answer; one certificate on both
   certificate names: IP Address:127.0.0.1, DNS:<this machine's name>, DNS:localhost
--bind 127.0.0.5:37171: listening ['127.0.0.1:37171', '127.0.0.5:37171', '[::1]:37171']; Host taken: 127.0.0.5, 127.0.0.1, [::1], localhost; refused: [::ffff:127.0.0.1], 127.0.0.2
--bind [::1]:42637: listening ['127.0.0.1:42637', '[::1]:42637']; Host taken: 127.0.0.1, [::1], localhost; refused: 127.0.0.5, [::ffff:127.0.0.1], 127.0.0.2
--bind [::ffff:127.0.0.1]:57555: no start (not a loopback or WireGuard address: the existing rule)
the kernel, 0.0.0.0:45143 and [::1]:45143 held (what --bind 0.0.0.0 holds): another program can take: none
the kernel, a dual-stack [::]:51919 held (what --bind [::] holds): another program can take: none
```

`https://localhost:<port>` is valid on both listeners; `https://[::1]:<port>` warns, as said
above. The wildcard binds of the server itself were not started (an unclaimed server on the
network); what such sockets cover was asked of the kernel with sockets that serve nothing,
and `listen.rs`' own test covers the plan.

### 5. Can a command make the second bind "not available" on purpose?

No. Read from inside a command of the node (nothing was changed on the machine):

```text
network on:  CapEff: 0000000000000000 | CapBnd: 0000000000000000 | net:[4026531833] (the machine's) | user:[4026534641] (its own) | disable_ipv6 = 0, not writable
network off: CapEff: 0000000000000000 | CapBnd: 0000000000000000 | net:[4026534661] (its own)       | user:[4026534866] (its own); TCP binds: EACCES; the server still holds both
```

Taking `::1` from `lo`, or switching IPv6 off, needs `CAP_NET_ADMIN` in the user namespace
that owns the machine's network namespace: root. A command has no capability at all and a
user namespace of its own; with the network off it is not in the machine's network
namespace. A machine without `::1` was not made (it needs root), so the "Unavailable"
branch itself was read, not run.

### 6. What the earlier binds opened

```text
a first start: [::1]:44911 takes a connection 0.00 s after the start; the request sent then is answered 7.04 s after the start: HTTP/1.1 200 OK
a start that fails after the binds (a broken config.toml): exit 1; listening on the port afterwards: []
```

A browser that connects during the start now waits in the kernel's queue and is answered by
the finished server; before the fix it was refused. Nothing answers before the guard exists,
and a start that fails leaves nothing listening. "The port is open" is no longer "the server
is ready" for a health check that only connects.

### What is left

**R-A. Low to Medium. While the server is down, a command of a second node on the same
machine can be at its address.** The note says "any program of the user can hold its port".
The program that matters is narrower and worth naming. A command of the server's **own**
node ends with the server. A command of a **second node on the same machine** (`node run`)
does not: it runs on when the server stops for an update or a restart.

```text
the server runs on ['127.0.0.1:41489', '[::1]:41489']; a command of its own node and one of a second node on this machine both wait for [::1]:41489: neither has it
the server is stopped (exit 0): listening on its port now: ['[::1]:41489']; of the two commands, still alive: 1
the server started again while the command holds it: exit 1: refusing to start: [::1]:41489 is in use by another program. …
the command gives it back (25 s): the next start holds ['127.0.0.1:41489', '[::1]:41489']
```

(The command here holds the socket and serves nothing.) Two things follow. First, for as
long as the server is down the origin is that command's, and an open tab of the UI keeps
calling its origin by itself, with the cookie and the login key on every call
(`web/src/api.ts`, `live.ts`: read, not run): the person need not open anything. Second, the
server cannot start until the command ends, at the latest at its timeout (an hour at the
ceiling), or its node is stopped. Before the fix the start succeeded with the origin shared,
which was worse; the refusal is the right behaviour. It is the same hole only for the time
the server is down, and only where an agent's node shares the master's machine, so it does
not hold this fix; it should not be left as one clause. The smallest step: say it in the
README where a second node on the master's machine is described, with what closes it today:
`--tls` (a program without the key file cannot be `https://localhost:<port>`; the browser
warns and sends nothing), or `[sandbox] network = "off"` on that node. A real fix is to
keep the sockets across a restart (socket activation, or handing the listeners over); a
task of its own.

**R-B. Note. The refusal names the address, not the program.** "It is named, and the person
decides" above: the address is named. One line would let them find it: `ss -ltnp 'sport =
:<port>'`.

**R-C. Note. A machine that gets `::1` after the start.** Where the second bind was "not
available", `localhost:<port>` is accepted and `[::1]:<port>` is not held; if IPv6 is then
switched on (root), the first finding is back until the next restart. Trying that bind
again now and then would close it; a line in the README is enough for now.

**The builder's residuals.** The container published with `-p 127.0.0.1:7777:7777`: agreed
that it is the host's to hold and can only be documented; not run (no container on this
machine). `--allowed-host` names: agreed. The node listener: agreed, by reading (a node
checks the pinned certificate, not a name). The certificate without `::1`: agreed, measured
above.

### Not run

The server itself on `0.0.0.0` or `[::]`. A machine without `::1`. A browser: which family
it tries first for `localhost`, and HTTP/3, are argued from `getaddrinfo` (`::1` first here)
and the missing `Alt-Svc`, not driven. A container. `cargo clippy`, `cargo fmt --check`,
`cargo deny`, the web suites and the rest of the Rust suites.

### Left clean

Every run printed `cleanup killed: nothing`. Afterwards: no process with this run's folders
or scripts in its command line, nothing listening that a run started, no mount. The owner's
server and its data directories were not touched.

## After the re-check

Written by the coordinator. From the re-check: the refusal now names how to find the program
(`ss -ltnp`, R-B); the comment in `main.rs` says what is and is not opened before the binds
(the directory's lock is); the README names R-A's case, a second node on the server's machine
whose command outlives a stop of the server, and what closes it today (`--tls`, or the network
off on that node).

R-A itself is open: while the server is down it holds no socket. The fix is to keep the
listening sockets across a restart (socket activation, or a small holder process), which is its
own task and belongs with the update and drain work (N.9). R-C (IPv6 enabled by root after a
start without it) is left as it is: it needs root and ends at the next start.
