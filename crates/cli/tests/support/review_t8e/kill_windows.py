#!/usr/bin/env python3
"""T.8e review: the kill windows, with the real guard and bwrap.

    kill_windows.py node  [tries]   SIGKILL the node 0 to 6 ms after its spawn
                                    returned (claim 1; the branch's own test
                                    covers 0 to 1 ms)
    kill_windows.py guard [tries]   SIGKILL the guard alone, the node alive,
                                    0 to 3 ms after bubblewrap exists (the
                                    residual PLAN.md names as "half a
                                    millisecond")
    kill_windows.py term  [tries]   SIGTERM to the guard 0 to 3 ms after its
                                    own start (before it has blocked its
                                    signals, SIGTERM's default action ends it)
    kill_windows.py bwrap [tries]   SIGKILL bubblewrap alone 0 to 3 ms after it
                                    exists: its init is then the guard's to
                                    end; is anything alive when the guard exits?

A survivor is a `sleep <n>` unique to this run still alive 2 s later; each is
killed by pid (with its namespace init) and counted per delay.

Re-check (1e73622): `guard` and `term` still leave namespaces here, because
this script is a node that does not sweep. The fix is in the bash tool, which
sweeps the session when it sees its guard die; that is measured through the
tool in review_t8e_proofs.rs `recheck_a_guard_killed_while_it_starts_bubblewrap`.
"""
import os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

mode = sys.argv[1]
tries = int(sys.argv[2]) if len(sys.argv) > 2 else 200
SECS = 977300 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
argv = guard_argv(root, "s1", ws, f"sleep {SECS}")


def survivors():
    left = gone_within(lambda: sleeps(SECS), 2.0)
    for s in left:
        pp = ppid(s)
        kill_pids([s] + ([pp] if pp and root.encode() in cmdline(pp) else []))
    time.sleep(0.02)
    cleanup(root, [SECS])
    return left


def node_try(us):
    r, w = os.pipe()
    node = os.fork()
    if node == 0:
        os.close(r)
        p, ours = spawn_guard(argv)
        os.write(w, b"x")
        time.sleep(1000)
        os._exit(0)
    os.close(w)
    os.read(r, 1)
    os.close(r)
    busy_wait(us)
    os.kill(node, signal.SIGKILL)
    os.waitpid(node, 0)
    return bool(survivors()), ""


def guard_try(us, what):
    """The node stays alive; the guard (or its bubblewrap) is signalled."""
    p, ours = spawn_guard(argv)
    note = ""
    if what == "term":
        busy_wait(us)
        os.kill(p.pid, signal.SIGTERM)
    else:
        bw = wait_for(lambda: [c for c in children(p.pid) if b"bwrap" in cmdline(c).split(b"\0")[0]],
                      2.0, 0.00002)
        busy_wait(us)
        if what == "guard":
            os.kill(p.pid, signal.SIGKILL)
        elif bw:
            os.kill(bw[0], signal.SIGKILL)
    try:
        p.wait(8)
    except subprocess.TimeoutExpired:
        note = "guard did not exit in 8 s"
    # The instant the guard is gone: is anything of its command alive?
    at_exit = sleeps(SECS) or [x for x in holding(root) if x != p.pid]
    if what == "bwrap" and at_exit:
        note += " ALIVE AT THE GUARD'S EXIT: " + describe(at_exit)
    left = survivors()
    ours.close()
    if what == "term" and p.returncode is not None:
        note += f" guard exit {p.returncode}"
    return bool(left), note


by_delay, notes = {}, {}
span = 6000 if mode == "node" else 3000
steps = 13
for i in range(tries):
    us = (i % steps) * span // (steps - 1)
    hit, note = node_try(us) if mode == "node" else guard_try(us, mode)
    a, b = by_delay.get(us, (0, 0))
    by_delay[us] = (a + hit, b + 1)
    if note.strip():
        notes[note.strip()] = notes.get(note.strip(), 0) + 1
total = sum(a for a, _ in by_delay.values())
print(f"{mode}: {total} of {tries} namespaces left running")
print("  by delay (us: left/tries): " + "  ".join(f"{d}: {a}/{b}" for d, (a, b) in sorted(by_delay.items())))
for n, c in sorted(notes.items()):
    print(f"  {c} x {n[:300]}")
print(f"cleanup: {cleanup(root, [SECS])}")
