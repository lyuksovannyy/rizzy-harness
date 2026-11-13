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

At `5add735` the stop's wait was 60 s too: exit 1 and `interrupted`. Since
`859e59c` it is 80 s (5 s + the snapshot's 60 s + 15 s).

    stop_bound.py [size, default 60G] [repo]     repo: the folder is a git repository
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 979000 + os.getpid() % 900
SIZE = sys.argv[1] if len(sys.argv) > 1 else "60G"
REPO = "repo" in sys.argv[2:]


def main():
    root = scratch("bound")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    if REPO:
        # The session's folder is a git repository of the user's (one commit).
        import subprocess
        for a in (["init", "-q"], ["commit", "-q", "--allow-empty", "-m", "first"]):
            subprocess.run(["git", "-c", "user.name=review", "-c", "user.email=review@example.invalid"] + a,
                           cwd=ws, check=True)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        text = "run: truncate -s %s big; sleep %d" % (SIZE, N)
        s.post("/api/sessions/%s/prompt" % sid, {"text": text})
        assert wait_for(lambda: sleeps(N)), "the command never started:\n" + s.log(10)
        print("the command made a sparse %s file and sleeps" % SIZE)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        assert not gone_within(lambda: sleeps(N), 10), "the command was not ended"
        print("%.1f s after SIGTERM the command is gone" % (time.monotonic() - t0))
        code = s.exited(200)
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
        for d, _, fs in os.walk(root):
            if "big" in fs:
                try:
                    os.unlink(os.path.join(d, "big"))
                except OSError:
                    pass

main()
