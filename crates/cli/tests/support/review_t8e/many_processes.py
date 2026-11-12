#!/usr/bin/env python3
"""T.8e review: how long the guard's confirmation takes when a command holds
many processes that ignore SIGTERM (claim 2's bounds: 2 s grace, "still
alive" 2 s after the kill, the node's 5 s, the agent loop's 5 s).

    many_processes.py <count>...

The command starts <count> `sleep`s with SIGTERM ignored; this script, as the
node, sends the TERM byte and half-closes the channel, and prints each report
of the guard with its time since the order. Real guard, real bwrap.
"""
import os, select, signal, socket, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977500 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
for count in [int(a) for a in sys.argv[1:]]:
    script = (f"trap '' TERM; i=0; while [ $i -lt {count} ]; do sleep {SECS} & i=$((i+1)); done; "
              f"echo ready; wait")
    p, ours = spawn_guard(guard_argv(root, "s1", ws, script), stdout=subprocess.PIPE)
    assert p.stdout.readline().strip() == b"ready"
    n = len(sleeps(SECS))
    t = time.monotonic()
    ours.send(b"T")
    ours.shutdown(socket.SHUT_WR)
    said = []
    while True:
        r, _, _ = select.select([ours], [], [], 30)
        got = ours.recv(256) if r else b""
        if not got:
            break
        said.append((round(time.monotonic() - t, 3), got.decode().strip()))
    code = p.wait(30)
    print(f"{n} processes ignoring SIGTERM: the guard said {said}, exited {code} "
          f"{time.monotonic() - t:.3f} s after the order; left: {len(sleeps(SECS))}")
    ours.close()
print(f"cleanup: {cleanup(root, [SECS])}")
