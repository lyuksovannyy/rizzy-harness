#!/usr/bin/env python3
"""T.8d re-check: the node stops (SIGTERM) with ends waiting, the two ways
an end can wait since A.19's stop and this branch are one program:

  a  a command ended by itself (exit 7) while a long foreground step of
     its turn runs: its end is queued for a step boundary that the stop
     takes away
  b  a command that ended and was recorded, its output still kept
  c  one still running

Expected: exit 0 inside the bound; a's end is its own (`exited`, code 7,
its output), not `node_stopping` and not left for the next start; c's is
`node_stopping`; nothing is recorded twice by the next start.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

A, C = 978901, 978902


def main():
    root = scratch("queued")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        a, b, c = (s.session(ws) for _ in range(3))
        s.post("/api/sessions/%s/prompt" % b, script([bg("sleep 3.3; echo done-b", 60)]))
        s.post("/api/sessions/%s/prompt" % c, script([bg("sleep %d" % C, 3600)]))
        s.post("/api/sessions/%s/prompt" % a, script([bg("sleep 4; echo done-a; exit 7", 60), fg("sleep %d" % A, 300)]))
        s.wait_event(b, "background_ended")
        s.wait_event(c, "turn_ended")
        assert wait_for(lambda: sleeps(A)), "a's foreground step never started"
        time.sleep(3.0)
        print("before the stop: a %s, b %s, c %s" % tuple(bg_account(s.events(x))[1] for x in (a, b, c)))
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(100)
        print("SIGTERM: exit %r after %.1f s; left: %d" % (code, time.monotonic() - t0, len(sleeps(A) + sleeps(C))))
        s2 = Server(data)
        procs.append(s2)
        time.sleep(1.0)
        for name, sid in (("a", a), ("b", b), ("c", c)):
            evs = s2.events(sid)
            print("  %s: started %s; ends %s; turns %s" % (name, bg_account(evs)[0],
                  [(p["bg_id"], p["reason"], p.get("exit_code"), p["tail"].strip()) for p in of(evs, "background_ended")], ends(evs)))
        s2.signal(signal.SIGTERM)
        s2.exited(40)
    finally:
        k = cleanup(procs, needle=root, secs=[A, C])
        print("cleanup killed: %s" % (k or "nothing"))

main()
