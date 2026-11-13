#!/usr/bin/env python3
"""T.8d review: is every background command accounted for by exactly one
`background_ended` after `kill -9` of the node and a start, whatever state
it was in at the kill?

  a  ended by itself while its chat was idle, its end recorded before the kill
  b  running at the kill
  c  ended by itself (exit 7) while a turn's long foreground step ran: its
     end is queued in the node's memory for the next step boundary
  d  a start in its first 3 s at the kill (no `background_started` yet)

Then a clean stop and a third start: nothing may be recorded twice.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

B, C, D = 978301, 978302, 978303


def show(s, sids, when):
    print(when)
    bad = 0
    for name, sid in sids:
        evs = s.events(sid)
        started, ended = bg_account(evs)
        for i in set(started) | set(ended):
            n = len(ended.get(i, []))
            bad += (i not in started) or n != 1
        print("  %s: started %s; ends %s; turns ended %s" % (name, started, ended, ends(evs)))
    return bad


def main():
    root = scratch("restart")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        a, b, c, d = (s.session(ws) for _ in range(4))
        sids = [("a", a), ("b", b), ("c", c), ("d", d)]
        s.post("/api/sessions/%s/prompt" % a, script([bg("sleep 3.3; echo done-a", 60)]))
        s.post("/api/sessions/%s/prompt" % b, script([bg("echo b; sleep %d" % B, 3600)]))
        s.post("/api/sessions/%s/prompt" % c, script([bg("sleep 4; echo done-c; exit 7", 60),
                                                      fg("sleep %d" % C, 300)]))
        s.wait_event(a, "background_ended")
        s.wait_event(b, "turn_ended")
        assert wait_for(lambda: sleeps(C)), "c's foreground step never started"
        time.sleep(3.0)  # c's background command has exited by now (4 s after its start)
        s.post("/api/sessions/%s/prompt" % d, script([bg("sleep %d" % D, 3600)]))
        assert wait_for(lambda: sleeps(D)), "d's command never started"
        time.sleep(0.5)
        show(s, sids, "before the kill:")
        t0 = time.monotonic()
        s.signal(signal.SIGKILL)
        s.exited(10)
        left = gone_within(lambda: sleeps(B) + sleeps(C) + sleeps(D), 2.0)
        print("kill -9: the commands' processes gone %.2f s later: %s" % (time.monotonic() - t0, not left))
        s2 = Server(data)
        procs.append(s2)
        time.sleep(1.0)
        bad = show(s2, sids, "after the next start:")
        s2.signal(signal.SIGTERM)
        print("clean stop: exit %r" % s2.exited(40))
        s3 = Server(data)
        procs.append(s3)
        time.sleep(1.0)
        bad += show(s3, sids, "after a third start:")
        print("commands without exactly one end: %d" % bad)
        s3.signal(signal.SIGTERM)
        s3.exited(40)
    finally:
        k = cleanup(procs, needle=root, secs=[B, C, D])
        print("cleanup killed: %s" % (k or "nothing"))

main()
