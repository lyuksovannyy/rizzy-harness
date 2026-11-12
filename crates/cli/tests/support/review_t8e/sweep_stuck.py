#!/usr/bin/env python3
"""T.8e review, T8E-4: what the start-up sweep says about a leftover it
cannot end.

A namespace without a guard (the stand-in for the doubly unlucky kill, as in
`cli/tests/guard.rs` `orphan`: the node's bubblewrap line without
`--die-with-parent`, started by a shell that exits) whose command is stuck in
a FUSE read the kernel will not interrupt (`support/fuse_stall.py`). Then a
real node is started on that data directory (`serve`, loopback, its sweep
runs before it serves) and its log is printed, with the state of the reader
at that moment. The node is stopped again, the read released.
"""
import os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
mnt = scratch("mnt")
stall = os.path.join(mnt, "stall")
STALL = os.path.join(HERE, "..", "fuse_stall.py")

fuse = subprocess.Popen([sys.executable, "-I", STALL, mnt], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, text=True)
try:
    assert fuse.stdout.readline().strip() == "mounted", "FUSE does not mount here"
    argv = guard_argv(root, "s1", ws, f"cat {stall}", die_with_parent=False)
    bw = argv[argv.index("--") + 1:]
    subprocess.run(["sh", "-c", '"$@" </dev/null >/dev/null 2>&1 &', "sh"] + bw, check=True)
    assert fuse.stdout.readline().strip() == "read", "the reader never read"
    cat = [p for p in pids() if cmdline(p) == b"cat\0" + stall.encode() + b"\0"]
    marked = holding(root)
    print(f"left by a dead node and a dead guard: {describe(marked)}")
    print(f"its command: {describe(cat)}")
    log = os.path.join(SCRATCH, "node.log")
    for start in (1, 2):
        with open(log, "w") as f:
            n = subprocess.Popen([EXE, "--data-dir", data, "serve", "--bind", "127.0.0.1:47918"],
                                 stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                                 env={**os.environ, "NO_COLOR": "1"})
        time.sleep(6)
        n.send_signal(signal.SIGINT)
        try:
            n.wait(10)
        except subprocess.TimeoutExpired:
            n.kill(); n.wait()
        said = [l.rstrip() for l in open(log) if "previous run" in l]
        print(f"node start {start} logged: {[l[l.find('shell containment'):] for l in said] or 'nothing about a leftover'}")
        print(f"  the reader after that start: {describe(cat)}; still marked: {describe(holding(root)) or 'nothing'}")
    fuse.stdin.close()
    fuse.wait()
    left = gone_within(lambda: [p for p in cat if alive(p)], 3.0)
    print(f"the read released: reader {'gone' if not left else describe(left)}")
finally:
    try:
        fuse.stdin.close()
    except OSError:
        pass
    fuse.wait()
    subprocess.run(["fusermount3", "-u", "-z", "--", mnt], stderr=subprocess.DEVNULL)
    k = cleanup(root)
    with open("/proc/self/mountinfo") as f:
        mine = [l.split()[4] for l in f if SCRATCH in l]
    print(f"cleanup: killed {k}; mounts of this run left: {mine}")
