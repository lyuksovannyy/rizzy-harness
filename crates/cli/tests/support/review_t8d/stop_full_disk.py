#!/usr/bin/env python3
"""T.8d review: the node stops while the log cannot be written (a full
disk, stood in for by a file size limit of 0 set on this script's own
server: every write to a file then fails with EFBIG), with two background
commands running and no turn.

First review (T8D-6): the stop waited its whole bound (80.1 s) for a write
nobody tried again, exited 0 and said "every turn is closed". Fixed means:
the ends are tried again, the stop does not wait out its bound, exits 1
and names the chat; the next start records each end once.
"""
import os, resource, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 978600


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
        s.post("/api/sessions/%s/prompt" % sid, script([bg("sleep %d" % N, 3600), bg("sleep %d" % (N + 1), 3600)]))
        s.wait_event(sid, "turn_ended", secs=60)
        assert sleeps(N) and sleeps(N + 1)
        resource.prlimit(s.pid, resource.RLIMIT_FSIZE, (0, 0))
        print("two background commands run, no turn; from now on no write to a file by pid %d succeeds" % s.pid)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(120)
        took = time.monotonic() - t0
        gone = wait_for(lambda: not (sleeps(N) or sleeps(N + 1)), 3)
        print("exit status %r, %.1f s after SIGTERM; the commands are gone: %s" % (code, took, gone))
        tries = len([l for l in s.log().split("\n") if "could not record a background command's end" in l])
        print("tries to record the two ends that the log refused: %d" % tries)
        fixed = code == 1 and took < 20 and any(sid in l for l in s.log().split("\n")[-3:])
        print("T8D-6 fixed (exit 1, well inside the bound, the chat named at the end): %s" % fixed)
        named = [l for l in s.log().split("\n") if sid in l]
        print("log lines that name the chat: %d" % len(named))
        for l in named[:3]:
            print("    " + l[-230:])
        print("the log's last lines:")
        for l in s.log(6).split("\n"):
            print("    " + l[-230:])
        s2 = Server(data)
        procs.append(s2)
        time.sleep(1.0)
        started, ended = bg_account(s2.events(sid))
        print("next start: started %s; ends %s" % (started, ended))
        s2.signal(signal.SIGTERM)
        print("the second server stops with %r" % s2.exited(40))
    finally:
        k = cleanup(procs, needle=root, secs=[N, N + 1])
        print("cleanup killed: %s" % (k or "nothing"))

main()
