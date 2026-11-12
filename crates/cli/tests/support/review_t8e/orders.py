#!/usr/bin/env python3
"""T.8e review: the orders in which the guard, bubblewrap and the namespace's
init can die, and what the command inherits; the real guard and bwrap, this
script as the node. Each case prints what it saw; "HELD" or "BROKEN".

  A  bubblewrap dies before its init (the init without a death signal, the
     stand-in for the window before it set one): is the init the guard's
     child (subreaper), and is anything alive when the guard exits?
  B  the init is SIGKILLed from outside: does the guard exit, with what?
  C  the graceful stop: a command that exits on SIGTERM; the report and time.
  D  the channel closes without the TERM byte (a dropped call): no grace.
  E  SIGTERM, SIGINT, SIGHUP to the guard: the command's own TERM trap never
     runs (the kill has no grace), and what the node is told.
  F  what the command inherits: blocked and ignored signals, descriptors.
"""
import os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977400 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
ok = True


def verdict(name, held, text):
    global ok
    ok &= held
    print(f"{name}: {'HELD' if held else 'BROKEN'}: {text}")


def started(script, **kw):
    p, ours = spawn_guard(guard_argv(root, "s1", ws, script, **kw), stdout=subprocess.PIPE)
    return p, ours


def nothing_left(p):
    return not (sleeps(SECS) or [x for x in holding(root) if x != p.pid])


# A
p, ours = started(f"sleep {SECS}", die_with_parent=False)
wait_for(lambda: sleeps(SECS))
bw = children(p.pid)[0]
init = children(bw)[0]
os.kill(p.pid, signal.SIGSTOP)
os.kill(bw, signal.SIGKILL)
adopted = wait_for(lambda: ppid(init) == p.pid, 2.0)
alive_then = bool(sleeps(SECS))
os.kill(p.pid, signal.SIGCONT)
code = p.wait(10)
clean = nothing_left(p)
verdict("A", bool(adopted) and alive_then and clean,
        f"bubblewrap killed; its init {init} became the guard's child: {bool(adopted)} "
        f"(command alive then: {alive_then}); guard exit {code}; alive at its exit: {not clean}; "
        f"channel: {ours.recv(64)!r}")
ours.close()

# B
p, ours = started(f"sleep {SECS}")
wait_for(lambda: sleeps(SECS))
init = children(children(p.pid)[0])[0]
os.kill(init, signal.SIGKILL)
code = p.wait(10)
clean = nothing_left(p)
verdict("B", clean, f"init SIGKILLed; guard exit {code}; alive at its exit: {not clean}; "
                    f"channel: {ours.recv(64)!r}")
ours.close()

# C
p, ours = started(f"trap 'echo got-TERM; exit 7' TERM; sleep {SECS} & wait")
wait_for(lambda: sleeps(SECS))
t = time.monotonic()
ours.send(b"T")
ours.shutdown(socket.SHUT_WR)
line = ours.recv(64)
took = time.monotonic() - t
code = p.wait(10)
out = p.stdout.read()
clean = nothing_left(p)
verdict("C", clean and b"got-TERM" in out and line.startswith(b"ended"),
        f"TERM byte then half-close: report {line!r} after {took*1000:.0f} ms, guard exit {code}, "
        f"output {out!r}, alive at its exit: {not clean}")
ours.close()

# D
p, ours = started(f"trap 'echo got-TERM; exit 7' TERM; sleep {SECS} & wait")
wait_for(lambda: sleeps(SECS))
t = time.monotonic()
ours.close()
code = p.wait(10)
took = time.monotonic() - t
out = p.stdout.read()
verdict("D", nothing_left(p) and b"got-TERM" not in out,
        f"channel closed: guard exit {code} after {took*1000:.0f} ms, output {out!r} (no grace, as documented)")

# E
for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    p, ours = started(f"trap 'echo got-TERM; exit 7' TERM INT HUP; sleep {SECS} & wait")
    wait_for(lambda: sleeps(SECS))
    os.kill(p.pid, sig)
    code = p.wait(10)
    out = p.stdout.read()
    verdict(f"E {sig.name}", nothing_left(p),
            f"guard exit {code}, output {out!r}, channel {ours.recv(64)!r}: the node sees an "
            f"exit code, not a stop; the command had no grace")
    ours.close()

# F
p, ours = started("grep -E '^Sig(Blk|Ign|Cgt)' /proc/self/status; ls -l /proc/self/fd | "
                  "awk '{print $9, $10, $11}'; echo pid=$$ ppid=$PPID")
out = p.stdout.read().decode()
code = p.wait(10)
blk = [l for l in out.splitlines() if l.startswith("SigBlk")]
ign = [l for l in out.splitlines() if l.startswith("SigIgn")]
verdict("F", blk == ["SigBlk:\t0000000000000000"] and "socket" not in out,
        f"exit {code}; " + " | ".join(out.split("\n")))
ours.close()

print(f"cleanup: {cleanup(root, [SECS])}")
sys.exit(0 if ok else 1)
