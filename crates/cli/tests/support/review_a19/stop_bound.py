#!/usr/bin/env python3
"""A.19 review: the 60 s bound, reached. No test of the branch reaches it.

The turn's own command makes its folder slow to snapshot (one sparse file:
no disk space, `truncate -s <size>`), then sleeps. The stop cancels the turn
and the command is killed at once; what then keeps the turn open is the
node's own after-turn checkpoint (`node/src/git.rs` `SNAPSHOT_TIMEOUT` =
60 s, not cancelled by a cancel), which is as long as the stop's whole wait
(`cli/src/stop.rs` `STOP_WAIT` = 60 s).

Prints the exit status and time, what of the run is still alive after the
exit and for how long, whether the lock is free meanwhile, and what the
next start makes of the turn.

    stop_bound.py [size, default 60G]
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 979000 + os.getpid() % 900
SIZE = sys.argv[1] if len(sys.argv) > 1 else "60G"


def main():
    root = scratch("bound")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "run: truncate -s %s big; sleep %d" % (SIZE, N)})
        assert wait_for(lambda: sleeps(N)), "the command never started:\n" + s.log(10)
        print("the command made a sparse %s file (%d bytes on disk) and sleeps"
              % (SIZE, os.stat(os.path.join(ws, "big")).st_blocks * 512))
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        assert not gone_within(lambda: sleeps(N), 10), "the command was not ended"
        print("%.1f s after SIGTERM the command is gone" % (time.monotonic() - t0))
        code = s.exited(100)
        took = time.monotonic() - t0
        print("exit status %r, %.1f s after SIGTERM" % (code, took))
        for l in s.log(3).split("\n"):
            print("    " + l[-300:])
        left = holding(root)
        print("alive right after the exit: %s" % (describe(left) or "nothing"))
        print("the data directory's lock is free: %s" % lock_free(data))
        t1 = time.monotonic()
        still = gone_within(lambda: holding(root), 150)
        print("they were gone %.0f s after the exit%s" % (time.monotonic() - t1,
              "" if not still else " (NOT gone: %s)" % describe(still)))
        s2 = Server(data)
        procs.append(s2)
        evs = s2.events(sid)
        print("next start: ends %s; %s" % (ends(evs), kinds(evs)[kinds(evs).find("6:"):][-260:]))
        s2.signal(signal.SIGTERM)
        print("the second server stops with %r" % s2.exited(100))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))
        try:
            os.unlink(os.path.join(ws, "big"))
        except OSError:
            pass

main()
