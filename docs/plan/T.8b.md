# T.8b **sec**

## Adds

**Done (branch `feat/T.8b-network-off`): with `[sandbox] network = "off"` the contained shell's bubblewrap gets `--unshare-net` (`Contain::args`, decided by the node's `Sandbox`, never per call); bwrap brings the namespace's `lo` up;
Landlock's TCP rule stays inside, so a TCP server on 127.0.0.1 inside a command is refused as before and UDP to itself works.
Both forms (own and shared `/proc`) take it.
The containment probe runs with it and checks that `/proc/net/dev` lists only `lo`: the probe judges the namespace by identity (`/proc/self/ns/net` against the node's, review T8B-2).
Where the probe with `--unshare-net` fails and the one without passes, the start tries once more; then `required` refuses, naming what failed, and `auto` still contains (PID namespace, guard, Landlock inside) without it, warning "network off WITH A GAP" (coordinator's change: falling back to Landlock alone lost the containment for nothing).
An `auto` node below where it could be (that gap, or Landlock alone after bubblewrap failed every probe at start) probes again beside a shell command at most every 60 s, on its own thread, bounded by 2 s, and steps up from the first pass, never down (`StepUp`; re-check R-1, R-2).
Reviewed in `docs/security-review/2026-11-T8b-network-off.md` with its re-check: T8B-2 closed;
T8B-1 closed as far as a later probe can (a node is below where it could be until its next shell command after the interval; commands already running keep their mode; `required` refuses until restarted);
T8B-3 open (planned with N.10); R-3 open (planned in T.8c).
The host's abstract Unix sockets are gone with the network off (`ECONNREFUSED` on every ABI).
The Landlock-only fallback, `contain = "off"` and the node's helpers (graphify, checkpoint git) get no namespace: there UDP and DNS still go out, said by `doctor`, the log, README and PLAN.md R2 (T.6 for those).
Tests `cli/tests/network_off.rs` (eight, seven failed before) and the guard's cancel test with the network off.
Not tested: nested in a container, IPv6 beyond `::1`.**
Network off means off (O12). `--unshare-net` when `[sandbox] network = "off"`: closes UDP and DNS, which Landlock leaks;
Landlock's TCP rules stay inside.
Shrinks T.6.

## Safeguards and abuse tests, written first

Tests first, from `net.sh`: with network off, TCP to the host's 127.0.0.1 and LAN address fails, a UDP datagram to either never arrives at a host listener, a DNS lookup fails (control: the same probe on today's path, where UDP and DNS go out); with network on, nothing changes.

## Invariants

Loopback; node decides

## Podman / Node

No / No

## Depends

T.8a

## Size

**S**

