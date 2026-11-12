#!/usr/bin/env python3
"""T.8e re-check, R-2 (b): the session text in `<data>/sandbox-leftovers`
comes from the command line of the process the sweep killed
(`--mark-session <text>`), and is written as it is. A process that carries
the mark of its own accord (any process of a contained command can: the mark
is in `/proc/1/cmdline` there) chooses that text. With a line break in it,
it writes further `pid start` lines, and the next start SIGKILLs whatever
they name.

    leftovers_inject.py

The marked process is a reader stuck in a FUSE read (`support/fuse_stall.py`;
on a slow disk a long `fsync` is the same state), so the sweep cannot see it
end and records it. The victim is an ordinary `sleep` of this script. Two
real node starts (`serve`, loopback) on a scratch data directory.
"""
import os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977800 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
mnt = scratch("mnt")
stall = os.path.join(mnt, "stall")
STALL = os.path.join(HERE, "..", "fuse_stall.py")
leftovers = os.path.join(data, "sandbox-leftovers")


def start_node():
    log = os.path.join(SCRATCH, "node.log")
    with open(log, "w") as f:
        n = subprocess.Popen([EXE, "--data-dir", data, "serve", "--bind", "127.0.0.1:47921"],
                             stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                             env={**os.environ, "NO_COLOR": "1"})
    time.sleep(6)
    n.send_signal(signal.SIGINT)
    try:
        n.wait(10)
    except subprocess.TimeoutExpired:
        n.kill(); n.wait()
    return [l[l.find("shell containment"):].rstrip()[:150] for l in open(log) if "previous run" in l]


fuse = subprocess.Popen([sys.executable, "-I", STALL, mnt], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, text=True)
victim = reader = None
try:
    assert fuse.stdout.readline().strip() == "mounted", "FUSE does not mount here"
    victim = subprocess.Popen(["sleep", str(SECS)])
    vstart = stat_fields(victim.pid)[19]
    session = f"s1\n{victim.pid} {vstart} injected"
    reader = subprocess.Popen([sys.executable, "-I", "-c", "import sys; open(sys.argv[1], 'rb').read()",
                               stall, "--mark", root, "--mark-session", session])
    assert fuse.stdout.readline().strip() == "read", "the reader never read"
    print(f"victim: pid {victim.pid} start {vstart} (an unmarked `sleep`)")
    print(f"the marked reader: pid {reader.pid}, its --mark-session is {session!r}")
    print(f"node start 1 logged: {start_node()}")
    print(f"  {leftovers} now holds: {open(leftovers).read()!r}")
    print(f"  the victim: {'alive' if victim.poll() is None else 'killed'}")
    print(f"node start 2 logged: {start_node()}")
    code = victim.poll()
    print(f"  the victim: {'KILLED, status ' + str(code) if code is not None else 'alive'}")
finally:
    try:
        fuse.stdin.close()
    except OSError:
        pass
    fuse.wait()
    subprocess.run(["fusermount3", "-u", "-z", "--", mnt], stderr=subprocess.DEVNULL)
    for p in (victim, reader):
        if p is not None:
            if p.poll() is None:
                p.kill()
            p.wait()
    with open("/proc/self/mountinfo") as f:
        mine = [l.split()[4] for l in f if SCRATCH in l]
    print(f"cleanup: sleeps left {sleeps(SECS)}; mounts of this run left: {mine}")
