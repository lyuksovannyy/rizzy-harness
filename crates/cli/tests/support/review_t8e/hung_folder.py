#!/usr/bin/env python3
"""T.8e review: the guard when the session folder stops answering after the
node built the command (so bubblewrap's `--chdir`, not the node, meets it).

The folder is `fuse_hang.py`'s `d`, stalled before the spawn. This script, as
the node, orders the graceful stop 1 s later and prints the guard's reports;
then the node "dies" (the channel closes), the folder is released, and what
is left is counted. Real guard, real bwrap.
"""
import os, select, signal, socket, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
mnt = scratch("mnt")
fuse = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "fuse_hang.py"), mnt],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
try:
    assert fuse.stdout.readline().strip() == "mounted"
    fuse.stdin.write("stall\n"); fuse.stdin.flush()
    assert fuse.stdout.readline().strip() == "stalling"
    p, ours = spawn_guard(guard_argv(root, "s1", os.path.join(mnt, "d"), "echo started"),
                          stdout=subprocess.PIPE)
    assert fuse.stdout.readline().strip() == "held"
    time.sleep(1)
    print(f"1 s after the spawn: {describe(holding(root))}")
    t = time.monotonic()
    ours.send(b"T")
    ours.shutdown(socket.SHUT_WR)
    r, _, _ = select.select([ours], [], [], 8)
    print(f"the guard's report {time.monotonic() - t:.2f} s after the order: "
          f"{ours.recv(64) if r else 'none in 8 s'}")
    print(f"then: {describe(holding(root))}")
    ours.close()
    fuse.stdin.close(); fuse.wait()
    left = gone_within(lambda: holding(root), 3.0)
    print(f"the folder released: guard exit {p.wait(5)}, left: {describe(left) or 'nothing'}, "
          f"output {p.stdout.read()!r}")
finally:
    try:
        fuse.stdin.close()
    except OSError:
        pass
    fuse.wait()
    subprocess.run(["fusermount3", "-u", "-z", "--", mnt], stderr=subprocess.DEVNULL)
    print(f"cleanup: killed {cleanup(root)}; FUSE mounts of this review left: {my_fuse_mounts()}")
