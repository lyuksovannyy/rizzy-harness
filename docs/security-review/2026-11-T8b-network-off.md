# T.8b review, 2026-11-12: network off means off

Reviewed: `feat/T.8b-network-off` at `27aa170`, only what it adds to
`feat/T.8e-kill-guard` (`8be2d55`, `5e30949`, `c071950`, `d56313c`,
`27aa170`). Read in full, then attacked on the branch's own build. No product
or test source changed. Machine: Arch Linux, kernel 7.2.15, Landlock ABI 10,
bubblewrap 0.13.1 (not setuid), interfaces `lo enp6s0 wlp7s0`,
`user.max_net_namespaces` 124236, `net.core.fb_tunnels_only_for_init_net` 0,
no tunnel module loaded, no systemd-resolved, `hosts: mymachines resolve
[!UNAVAIL=return] files myhostname dns`. Format and severity scale as in
`2026-11-T8e-guard.md`.

## 1. Verdict

**The namespace holds; the decision whether to use it does not.** On every
path I could reach from inside a command, with the network off, nothing
arrived at a listener of this machine: IPv4, IPv6 loopback and link-local,
broadcast, multicast, a device bind, ping and raw sockets, rtnetlink, a
nested namespace, TCP, abstract Unix stream and datagram sockets, in both
forms of `/proc`. The command has no capability, an empty bounding set and no
descriptor beyond 0 to 2.

What does not hold is the start-up probe that chooses between "namespace" and
"contained without one". It runs once, takes **any** failure for "bubblewrap
cannot make a network namespace here", and judges a namespace by its
interface list instead of by its identity. In both cases an `auto` node
(the default) runs every later command with UDP and DNS open, says so once in
its start-up log, and never again; `doctor` on the same machine can say the
opposite of what the running node does.

| Id | Severity | What | Proof |
|---|---|---|---|
| T8B-1 | **Medium** (claim 4) | One failed with-namespace probe (both forms, milliseconds apart) leaves an `auto` node without the namespace for its whole life, though bubblewrap makes one a moment later. Every later command's UDP reaches the host. `doctor` then prints `ok ... a network namespace of its own` for the same machine. `required` refuses for the node's life with the wrong reason. | one test through the node's own start and the bash tool |
| T8B-2 | **Medium** (claim 4; the node's half proven, the kernel's half not run here) | The probe takes "a network namespace of its own" to mean "`/proc/net/dev` lists `lo` and nothing else". A real, fresh namespace that lists one more interface is reported as no namespace: `auto` drops the namespace, `required` refuses, both with a false sentence. | one test: the real bubblewrap, a real namespace, only the list the script reads replaced |
| T8B-3 | **Low** (claim 4) | After the start-up log line nothing says that the network is not off: not the tool result, not the chat, not the master or the UI (the node's sandbox state never leaves the node). | reading; the tool results printed by T8B-1's test |
| N-1 to N-8 | Note | the documents, the DNS test, families not opened, the tests' temp folders, `doctor` probing three times, the guard tests, cost, a nested namespace | reading; one test |

Asked for before merge: T8B-2 (compare the namespace's identity, a few
lines, and its test) and T8B-1 (try again before giving the namespace up, and
again later). T8B-3 can be one line in the tool result.

## 2. Findings

### T8B-1. Medium. One failed probe turns the namespace off for the node's life

**The claim it breaks.** Claim 4: "where bubblewrap cannot create a network
namespace ... `auto` keeps containing without the namespace and says so";
"the node never runs a command with the network silently on". `README.md:783`:
"Where bubblewrap runs but cannot make a network namespace (the node's probe
at start finds out)".

**The mechanism.** `containment` (`crates/tools/src/sandbox.rs:1310-1321`):

```rust
let mut found = probe(true);
...
if settings.network == Network::Off
    && let Err(e) = &found
    && let Ok(without) = probe(false)
{
    net_err = Some(e.clone());
    found = Ok(without);
}
```

`probe(true)` is two runs of bubblewrap a few milliseconds apart (own
`/proc`, then shared). If both fail **for any reason** and the run without
`--unshare-net` then succeeds, the node concludes that bubblewrap cannot make
a network namespace. Nothing tells a failure of the namespace from a failure
of that moment (`fork` refused at the process limit, no memory, a namespace
count reached, a slow start), and the with-namespace probe is never run
again: `Contain::net_ns` stays `false` in the one `Sandbox` the node keeps
(`contain.rs:187`, `sandbox.rs:430`).

`doctor` is another process and runs its own probe (`cli/src/main.rs:846`),
so once the moment has passed it reports the namespace for a node that is
running without it.

**Reproduction.** `crates/cli/tests/support/review_t8b/run_rust_proofs.sh
finding_one_failed_probe`. The stand-in for bubblewrap fails its first two
runs that carry `--unshare-net` with bubblewrap's own message and is the real
`/usr/bin/bwrap` from then on. Real output (scratch path shortened to `…`):

```
the node's line at start (Warn): contained (…/bwrap-fails-twice: a PID namespace and /proc of its
  own per command; nothing a command starts outlives it); network off WITH A GAP: bubblewrap cannot
  make a network namespace here (…/bwrap-fails-twice failed (exit Some(1)): bwrap: Creating new
  namespace failed: Resource temporarily unavailable; without a new /proc: … Resource temporarily
  unavailable), so commands run contained without one: TCP is stopped by Landlock, but UDP and DNS
  still go out, and abstract Unix sockets outside are closed by Landlock (ABI 6 or newer). contain =
  "required" refuses this
a second start says (Ok): contained (…); network off: a network namespace of its own per command
  (only its own loopback: no TCP, UDP or DNS out)
doctor says: ok    shell containment: contained (…); network off: a network namespace of its own per
  command (only its own loopback: no TCP, UDP or DNS out)
command 0: tool result "ifs=[lo enp6s0 wlp7s0 ]\nsent\n\n[exit code: 0]"; the host's listener heard ["gap-0"]
command 1: tool result "ifs=[lo enp6s0 wlp7s0 ]\nsent\n\n[exit code: 0]"; the host's listener heard ["gap-1"]
command 2: tool result "ifs=[lo enp6s0 wlp7s0 ]\nsent\n\n[exit code: 0]"; the host's listener heard ["gap-2"]
required: Some("this node refuses agentic sessions: [sandbox] network = \"off\" and contain =
  \"required\" in /etc/rizzy/node.toml, and bubblewrap cannot make a network namespace here (…
  Resource temporarily unavailable …), so UDP and DNS would go out. Set contain = \"auto\" to run
  contained with that gap")
```

**What is and is not shown.** Shown: the node's decision, its permanence, the
datagrams arriving, `doctor` disagreeing. Not shown: a contained command
bringing the failing moment about at a start. I did not try to fill the
process table or the namespace count on this machine (it would deny them to
the owner's other programs). The start is the only moment that matters, and a
command of this node cannot be running during it (the guard ends it with the
node), so the realistic cause is the machine's state at boot or at a restart
under load, not an agent. That is why this is Medium and not High.

One failure instead of two gives the shared-`/proc` form for life (with the
namespace). That is T.8a's behaviour and the same cause.

**The smallest fix I would accept.** (a) When the probe without the
namespace has passed, run the probe with it once more before giving the
namespace up. (b) A node that gave it up tries the with-namespace probe again
when a session starts (it costs about 5 ms here), takes the namespace when it
works, and logs the change. (c) `required`: the refusal says what failed
("bubblewrap failed with `--unshare-net`: …"), not what it concludes.

**The test that should exist.** The stand-in above: after two failures the
start must end with the namespace (a), and with a stand-in that fails ten
times the third command must run with `lo` alone (b).

### T8B-2. Medium. The probe judges a namespace by its interface list

**The claim it breaks.** Claim 4, and the truth of the line the node prints.
`contain.rs:282-288`:

```sh
n=0; { read -r _; read -r _; while IFS=': ' read -r i _; do
[ "$i" = lo ] || exit 6; n=$((n+1)); done; } </proc/net/dev; [ "$n" = 1 ] || exit 6;
```

Exit 6 is reported as "the command was not in a network namespace of its
own" (`sandbox.rs:606`).

**The mechanism.** A new network namespace is not required to hold only `lo`.
A Linux kernel with a tunnel module loaded (`ipip`, `sit`, `ip_gre`,
`ip6_tunnel`, `ip6_gre`, `ip_vti`) creates that module's fallback device
(`tunl0`, `sit0`, `gre0`, `ip6tnl0`, …) in **every** new network namespace
unless `net.core.fb_tunnels_only_for_init_net` is set; its default is 0, and
it is 0 on this machine. Those devices are down and lead nowhere: the
namespace is as closed as one with `lo` alone. Hosts that run containers with
an IP-in-IP overlay, or any 6in4 or GRE tunnel, have such a module loaded.
There the probe exits 6 in both forms, the fallback of T8B-1 takes over, and
the node runs every command in the host's network, or refuses under
`required`, although `--unshare-net` works.

**Reproduction.** `run_rust_proofs.sh finding_an_extra_interface`. The
stand-in passes every option to the real `/usr/bin/bwrap` unchanged
(`--unshare-net` included) and logs the namespace the real bubblewrap makes;
the only change is that the node's own script reads its interface list from a
file with the lines `lo` and `tunl0` instead of `/proc/net/dev`. Real output:

```
auto (Warn): contained (…/bwrap-extra-interface: a PID namespace and /proc of its own per command;
  nothing a command starts outlives it); network off WITH A GAP: bubblewrap cannot make a network
  namespace here (the command was not in a network namespace of its own; without a new /proc: the
  command was not in a network namespace of its own), so commands run contained without one: TCP is
  stopped by Landlock, but UDP and DNS still go out, …
what the real bubblewrap made each time:
host net:[4026531833] / with --unshare-net net:[4026533784]
host net:[4026531833] / with --unshare-net net:[4026534633]
a command on that node: "ifs=[lo enp6s0 wlp7s0 ]\nsent\n\n[exit code: 0]"; the host's listener heard ["no-namespace"]
required: Some("this node refuses agentic sessions: … bubblewrap cannot make a network namespace
  here (the command was not in a network namespace of its own; …), so UDP and DNS would go out. …")
```

**What is and is not shown.** Shown: the node's reaction to a fresh namespace
that lists a second interface, end to end, and that both sentences it prints
are false there. **Not run here:** a kernel that really puts a second
interface into a new namespace. No tunnel module is loaded on this machine,
and loading one is a change to the owner's system that I did not make. What
settles it, on any machine where `lsmod` shows one of the modules above:
`unshare -Urn cat /proc/net/dev` (unprivileged, harmless; it lists more than
`lo`), then `rizzy-harness doctor` with `network = "off"`. Also not tested:
whether a command can bring that state about on a machine that does not have
it.

**The smallest fix I would accept.** Ask the question the comment asks. The
node passes its own `readlink /proc/self/ns/net` to the script, and the
script fails only if its own is the same:

```sh
[ "$(readlink /proc/self/ns/net)" != "$2" ] || exit 6;
```

`readlink` on `/proc/self/ns/net` works inside the wrapper in both forms
(`inside.py` prints it: `netns net:[4026533795]` against the host's
`net:[4026531833]`). The interface list can stay as a log detail; it must not
decide.

**The test that should exist.** The stand-in of the proof: with a list of
`lo` and `tunl0` the start must end with the namespace, under `auto` and
under `required`.

### T8B-3. Low. After the start-up line, nobody is told

**The claim it breaks.** Claim 4's "says so with a warning", for everyone but
the reader of the node's log at the moment it started.

The gap line and the fallback's warning exist in two places: one `warn` log
line at start (`cli/src/main.rs:627`) and `doctor`. `doctor` probes again and
can disagree (T8B-1). `contain_summary` is read nowhere else: nothing of the
node's sandbox state is in the protocol, the master or the UI. The tool
result in gap mode is the plain output (T8B-1's `command 0..2` above); the
agent's prompt says nothing about the network either. A person who set
`network = "off"` on a node that runs as a service, and works in the chat,
has no sign that UDP and DNS are open on it.

This is how T.8a's "NOT CONTAINED" line already works, so it is not new in
kind; it weighs more here because the setting is a promise about data
leaving the machine.

**The smallest fix I would accept.** With the network off and no namespace
(the gap, the fallback, `contain = "off"`), the bash tool appends one line to
its result: "this node's network setting is off,
but UDP and DNS are not blocked here (see the node's log)". It tells the
agent nothing it cannot find out with one datagram. Reporting the node's
state to the master is the better answer and a feature; not before Gate 3.

**The test that should exist.** `a_bwrap_without_network_namespaces_…`: the
tool result of the gap node contains that line; a node with the namespace
does not.

### Notes

**N-1. The documents.** Three sentences say more than is true, one reference
is stale:

- `README.md:783` "DNS lookups fail", and the containment line "no TCP, UDP
  or DNS out", are unconditional. Below Landlock ABI 9 a command can connect
  to any named Unix socket its user may open (`PLAN.md` T4, review T8A-1),
  and a resolver that answers over one (systemd-resolved, nscd) would do the
  lookup for it from outside the namespace. **Not proven:** this machine has
  ABI 10 (`pathname-outside-the-grants EACCES` in the proof) and no such
  resolver. What settles it: a kernel below ABI 9 with systemd-resolved,
  `resolvectl query` in a contained command with the network off. Below ABI 9
  the containment line is already a warning; the network sentence should
  carry the same condition.
- `README.md:789-790` "name bubblewrap's error": in T8B-2 the named error is
  the probe's own, and it is wrong.
- The "Network off" paragraph is about the shell. It should say in one
  sentence that the agent's web tools (search and fetch) are not affected by
  `network = "off"`; a reader takes "network off" for the agent.
- `docs/nodes-credentials-environments.md` cites `sandbox.rs:76-85`; the
  comment is now lines 76 to 88.

**N-2. The DNS test.** `network_off_dns_lookups_fail` looks up a name under
`.invalid`, which fails on every machine whether or not a resolver can be
reached. What it proves is its guard: the lookup runs only where `lo` is the
only interface. That is a second namespace test, not a DNS test; it does fail
before the change (`lookup-skipped`) and sends nothing. I see no way to test
a real lookup without leaving the machine, so it is enough as long as the
comment says what it proves. The UDP test is the proof.

**N-3. Families not opened.** `inside.py` opens INET, INET6, UNIX, NETLINK
and PACKET only. Opening a family whose module is not loaded (`AF_VSOCK` is
`CONFIG_VSOCKETS=m` here, not loaded) would load it on the owner's machine,
so none was opened. `ADOPTION_PLAN.md` T.6 already gives `AF_VSOCK` to
seccomp; the README speaks of TCP, UDP and DNS only. Not tested.

**N-4. The tests' temp folders.** `network_off.rs` uses `tempfile::tempdir()`
(`/tmp`). I ran it with `TMPDIR` under the shared target. Nothing in it
contacts another machine: `lan_addr` only `connect`s a UDP socket to
192.0.2.1 (no packet), the DNS lookup is guarded, `network_on_is_unchanged`
does no lookup, `doctor` opens no socket.

**N-5. `doctor` starts the sandbox three times** in one run (`main.rs:826`,
`:846`, `:870`: the sandbox line, the containment line, the checkpoint
line), each with its own probes. Under a passing failure its own lines can
disagree with each other.

**N-6. The guard with the network off.** All 23 tests of `guard.rs` pass on
this branch (101 s). Only the cancel test runs with the network off; the
kills of the node and the sweep do not. Nothing in the sweep or the guard
reads bubblewrap's options by position (`git grep`), so I did not repeat
them.

**N-7. Cost.** `true` through the bash tool, 40 runs, twice: network on
median 4.4 and 4.1 ms, off 4.5 and 4.4 ms.

**N-8. A namespace of the command's own.** `unshare -Urn` inside a command
fails at `/proc/self/uid_map` (Landlock: `/proc` is read-only). Whatever a
command makes there is a namespace below its own and holds nothing of the
host's.

## 3. What I tried that held

Through `sandbox::start_with_exe` (the node's start with its real probe) and
the bash tool, `inside.py` run in the command, every listener the test's own
(`run_rust_proofs.sh held_`):

| Attack | Result inside | At the host's listener |
|---|---|---|
| UDP to 127.0.0.1, `::1` | `sent` (its own loopback) | nothing (control through Landlock alone: all three unicast datagrams arrive) |
| UDP to this machine's LAN address | `ENETUNREACH` | nothing |
| UDP to this machine's IPv6 link-local address, every scope the command has | `ENETUNREACH` | nothing |
| Broadcast 255.255.255.255, multicast 239.255.77.77 (default route and forced onto `lo`), `ff02::1` | `ENETUNREACH`, `sent` on its own `lo` | nothing on 0.0.0.0, on the group joined on loopback, on `[::]` |
| `SO_BINDTODEVICE` to `enp6s0`, `wlp7s0` | `ENODEV` | nothing |
| ICMP and ICMPv6 ping sockets | `EACCES` | |
| Raw IPv4, raw IPv6, `AF_PACKET` | `EPERM` | |
| rtnetlink link dump | `lo` | |
| TCP to 127.0.0.1 and the LAN address | `EACCES` (Landlock, the second layer) | no connection |
| Abstract Unix socket, stream and datagram | `ECONNREFUSED` | nothing |
| Named Unix socket outside the grants | `EACCES` (Landlock ABI 9+) | nothing |
| Named Unix socket in the session's folder | `CONNECTED` | a connection: the documented way out, noted, not counted |
| Capabilities | `CapEff`, `CapPrm`, `CapBnd`, `CapAmb` all 0; `NoNewPrivs 1`; uid 1000 | |
| Inherited descriptors | 0 `/dev/null`, 1 and 2 the output pipes, nothing else | |
| Its namespace | `net:[4026533795]`, the host's `net:[4026531833]` | |

The same in the shared-`/proc` form, forced
(`held_the_shared_proc_form_has_the_namespace_too`): identical, `/proc`
listing 496 processes of the machine and `/proc/net/dev` still only `lo`.

Claim 2, by reading and the branch's tests: `Contain::args` adds the flag
only for `net_off && self.net_ns`; with the network on the argument list is
the one of the parent branch (`the_network_off_adds_a_network_namespace`,
`network_on_is_unchanged`).

Claim 3, by reading: the node builds one `Sandbox` at start
(`sandbox.rs:1097`, the only `Sandbox::new` outside tests); `network` and
`net_ns` are private fields with no setter but `without_network_namespace`,
called only by the probe; `wrapper` reads nothing from the call, the session
or the protocol. I found no second sandbox for sub-agents or routines.

The branch's own suite: `network_off` 8 of 8, `guard` 23 of 23.

## 4. What I could not test

- A kernel that puts fallback tunnel devices into a new namespace (T8B-2's
  second half), and whether a command can cause that state.
- A kernel below Landlock ABI 9 with a resolver on a named socket (N-1).
- Socket families whose modules are not loaded here, `AF_VSOCK` first (N-3).
- A real cause of T8B-1 (a full process table, the namespace count): it
  would deny namespaces and processes to the owner's other programs.
- Nested in a container; a setuid bubblewrap.
- The start-up log line of a real `node run` or `serve`: I read
  `main.rs:623-632` and ran `doctor` and `start_with_exe`, not a node.

## 5. The claims

1. Own namespace, nothing out, abstract sockets gone: **holds** for
   everything in section 3, on this machine.
2. Network on, nothing changes: **holds**.
3. Only the node's setting decides: **holds** (reading; no input exists to
   test against).
4. `required` refuses, `auto` warns, never silently on: **does not hold as
   written.** The probe says "cannot" when bubblewrap can (T8B-1, T8B-2), and
   after the one log line the network is on in silence (T8B-3).
5. The guard, the kill and its confirmation: **hold** (23 of 23; cost about
   0.2 ms per command).
6. The documents say where UDP and DNS still go out: **mostly.** The three
   places are named; four sentences need the changes in N-1.

## 6. Running the proofs

```sh
CARGO_TARGET_DIR=<target> crates/cli/tests/support/review_t8b/run_rust_proofs.sh            # all five
CARGO_TARGET_DIR=<target> crates/cli/tests/support/review_t8b/run_rust_proofs.sh finding_   # T8B-1, T8B-2
CARGO_TARGET_DIR=<target> crates/cli/tests/support/review_t8b/run_rust_proofs.sh held_      # section 3
```

Needs bubblewrap and python3. The script copies `review_t8b_proofs.rs` to
`crates/cli/tests/` for the run and removes the copy. Scratch files go under
`$CARGO_TARGET_DIR/review_t8b/` and `TMPDIR` is pointed there. `inside.py`
runs only inside a command and sends nothing unless `lo` is its only
interface; the control run sends to this machine's own unicast addresses
only. `finding_*` pass when the finding reproduces; a fix turns the named
assertions around. No process outlives a test: every command runs through
the bash tool to its end (checked afterwards with `pgrep -af` for the scratch
folder's name, the two stand-ins and `inside.py`: none).
