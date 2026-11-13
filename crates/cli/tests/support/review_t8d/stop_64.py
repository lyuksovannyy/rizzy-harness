#!/usr/bin/env python3
"""T.8d review: the node's stop and its death with the node's cap of
background commands running (64 by default: 8 chats of 8), every one
ignoring SIGTERM.

    stop_64.py term   SIGTERM: how long, the exit status, what is recorded
    stop_64.py kill   kill -9: how long until nothing is left, what the next
                      start records

Also: what the 65th start is told, and what 64 cost beside the node.
"""
import os, signal, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

BASE = 978500
CHATS, EACH = 8, 8


def main(how):
    root = scratch("stop64-" + how)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    marks = [BASE + i for i in range(CHATS * EACH + 2)]
    try:
        s = Server(data)
        procs.append(s)
        rss0, n0 = rss_kib(s.pid), nprocs()
        sids = [s.session(ws) for _ in range(CHATS)]
        for c, sid in enumerate(sids):
            calls = [bg("trap '' TERM; sleep %d" % (BASE + c * EACH + i), 3600) for i in range(EACH)]
            s.post("/api/sessions/%s/prompt" % sid, script(calls))
        for sid in sids:
            s.wait_event(sid, "turn_ended", secs=120)
        running = [m for m in marks[:CHATS * EACH] if sleeps(m)]
        print("running: %d of %d; the node's RSS %d -> %d KiB; processes on the machine +%d"
              % (len(running), CHATS * EACH, rss0, rss_kib(s.pid), nprocs() - n0))
        extra = s.session(ws)
        s.post("/api/sessions/%s/prompt" % extra, script([bg("sleep %d" % marks[-1], 600)]))
        evs = s.wait_event(extra, "turn_ended")
        print("the 65th start, in a ninth chat: %s" % results(evs)[0][:230])
        full = sids[0]
        s.post("/api/sessions/%s/prompt" % full, script([bg("sleep %d" % marks[-2], 600)]))
        time.sleep(0.2)
        evs = s.wait_event(full, "turn_ended")
        evs = wait_for(lambda: (lambda e: e if len(ends(e)) == 2 else None)(s.events(full)), 20)
        print("a ninth start in a full chat: %s" % results(evs)[-1][:230])
        t0 = time.monotonic()
        if how == "term":
            s.signal(signal.SIGTERM)
            code = s.exited(90)
            took = time.monotonic() - t0
            left = gone_within(lambda: [m for m in marks if sleeps(m)], 2.0)
            print("SIGTERM with 64 that ignore SIGTERM: exit %r after %.1f s; commands left: %d" % (code, took, len(left)))
            for l in s.log(4).split("\n"):
                print("    " + l[-200:])
        else:
            s.signal(signal.SIGKILL)
            s.exited(10)
            left = gone_within(lambda: [m for m in marks if sleeps(m)], 5.0)
            print("kill -9 with 64 running: nothing left after %.2f s: %s" % (time.monotonic() - t0, not left))
        s2 = Server(data)
        procs.append(s2)
        time.sleep(1.5)
        tally, wrong = {}, 0
        for sid in sids:
            started, ended = bg_account(s2.events(sid))
            for i in started:
                r = ended.get(i, [])
                wrong += len(r) != 1
                for x in r:
                    tally[x] = tally.get(x, 0) + 1
        conf = {}
        for sid in sids:
            for p in of(s2.events(sid), "background_ended"):
                conf[p["confirmed"]] = conf.get(p["confirmed"], 0) + 1
        print("next start: ends by reason %s, by the guard's word %s; commands without exactly one end: %d"
              % (tally, conf, wrong))
        s2.signal(signal.SIGTERM)
        s2.exited(40)
    finally:
        k = cleanup(procs, needle=root, secs=marks)
        print("cleanup killed: %s" % (k or "nothing"))

main(sys.argv[1] if len(sys.argv) > 1 else "term")
