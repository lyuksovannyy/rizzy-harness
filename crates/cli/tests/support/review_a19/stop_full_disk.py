#!/usr/bin/env python3
"""A.19 review: a stop while the session log cannot be written (a full
disk, stood in for by a file size limit of 0 set on this script's own
server once its turn runs: every write to a file then fails with EFBIG;
SIGXFSZ is ignored, as Python leaves it for its children here).

Does the process exit 0 and say "every turn is closed" with the turn's end
not in the log?
"""
import os, resource, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 981000 + os.getpid() % 900


def main():
    root = scratch("full")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data, restore_signals=False)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "run: sleep %d" % N})
        assert wait_for(lambda: sleeps(N)), "the command never started"
        s.wait_event(sid, "tool_call_started")
        before = kinds(s.events(sid))
        resource.prlimit(s.pid, resource.RLIMIT_FSIZE, (0, 0))
        print("the log before the stop: %s" % before[-120:])
        print("from now on no write to a file by pid %d succeeds" % s.pid)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(80)
        print("exit status %r, %.1f s after SIGTERM; the log's last lines:" % (code, time.monotonic() - t0))
        for l in s.log(5).split("\n"):
            print("    " + l[-260:])
        print("the command is gone: %s" % (not gone_within(lambda: sleeps(N), 5)))
        s2 = Server(data)
        procs.append(s2)
        evs = s2.events(sid)
        print("next start: ends %s; %s" % (ends(evs), kinds(evs)[kinds(evs).find("6:"):][-260:]))
        s2.signal(signal.SIGTERM)
        print("the second server stops with %r" % s2.exited(30))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))

main()
