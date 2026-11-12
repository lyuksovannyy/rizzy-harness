#!/usr/bin/env python3
"""T.8e re-check, R-2: `<data>/sandbox-leftovers` is a list of `pid start
session` that every node start SIGKILLs (`contain::sweep`), with one check:
a process with that pid and that start time (clock ticks since boot) exists.
It does not ask whether the entry is from this boot, whether the process is
this user's, or whether it ever carried the node's mark.

    leftovers_file.py

The victim is an ordinary `sleep` of this script (no mark, no namespace, a
child of this script). Its pid and start time are written to the file, as a
previous boot's start would have left them for a process that happens to
have the same two numbers now; then a real node is started on that data
directory (`serve`, loopback). Also shown: the file is read through a
symbolic link.
"""
import os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977600 + os.getpid() % 50
data = scratch("data")
elsewhere = scratch("elsewhere")


def start_node(port):
    log = os.path.join(SCRATCH, "node.log")
    with open(log, "w") as f:
        n = subprocess.Popen([EXE, "--data-dir", data, "serve", "--bind", f"127.0.0.1:{port}"],
                             stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                             env={**os.environ, "NO_COLOR": "1"})
    time.sleep(6)
    n.send_signal(signal.SIGINT)
    try:
        n.wait(10)
    except subprocess.TimeoutExpired:
        n.kill(); n.wait()
    return [l[l.find("shell containment"):].rstrip() for l in open(log) if "previous run" in l]


for via_link in (False, True):
    victim = subprocess.Popen(["sleep", str(SECS)])
    start = stat_fields(victim.pid)[19]
    entry = f"{victim.pid} {start} some-session\n"
    f = os.path.join(data, "sandbox-leftovers")
    if via_link:
        target = os.path.join(elsewhere, "not-the-nodes-file")
        open(target, "w").write(entry)
        os.symlink(target, f)
    else:
        open(f, "w").write(entry)
    print(f"victim: pid {victim.pid}, started at tick {start}, command line "
          f"{cmdline(victim.pid).replace(b'\\0', b' ').decode().strip()!r}, parent this script; "
          f"the file {'is a link to ' + target if via_link else 'holds'}: {entry.strip()!r}")
    said = start_node(47919)
    code = victim.poll()
    print(f"  the node logged: {said or 'nothing'}")
    print(f"  the victim: {'KILLED, status ' + str(code) if code is not None else 'alive'}")
    if code is None:
        victim.kill()
    victim.wait()
    if os.path.lexists(f):
        os.remove(f)
print(f"cleanup: sleeps left {sleeps(SECS)}")
