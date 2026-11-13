#!/usr/bin/env python3
"""T.8d review: the first 3 s of a background start, when the command is
in the call and not yet in the table. A cancel, a delete of the chat and a
stop of the node in that window; then cancels timed around the hand-over
itself (2.90 s to 3.10 s after the start).

The rule checked every time: the command's processes are alive only if the
log holds a `background_started` for it without a `background_ended`.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

BASE = 978400


def escapers(n):
    """The T.8a escapers under one marker range, then the command itself."""
    return ("setsid sleep %d & nohup sleep %d >/dev/null 2>&1 & ( setsid sleep %d & ) ; "
            "sh -c 'trap \"\" TERM; exec sleep %d' & sleep %d" % (n, n + 1, n + 2, n + 3, n + 4))


def alive_of(n):
    return [p for i in range(5) for p in sleeps(n + i)]


def main():
    root = scratch("first")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs, marks = [], []
    try:
        s = Server(data)
        procs.append(s)
        # 1. cancel at 1 s
        n = BASE; marks.append(n)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([bg(escapers(n), 600)]))
        assert wait_for(lambda: len(alive_of(n)) == 5), "never started"
        time.sleep(0.7)
        s.post("/api/sessions/%s/cancel" % sid, {})
        evs = s.wait_event(sid, "turn_ended")
        left = gone_within(lambda: alive_of(n), 3.0)
        print("cancel in the first wait: started %s, turn %s, processes left %d; result: %s"
              % (bg_account(evs)[0], ends(evs), len(left), results(evs)[0][-150:].replace("\n", " ")))
        # 2. delete at 1 s
        n = BASE + 10; marks.append(n)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([bg(escapers(n), 600)]))
        assert wait_for(lambda: len(alive_of(n)) == 5), "never started"
        time.sleep(0.7)
        t0 = time.monotonic()
        c, v = s.call("DELETE", "/api/sessions/%s" % sid)
        left = gone_within(lambda: alive_of(n), 3.0)
        print("delete in the first wait: %s %s after %.1f s, processes left %d"
              % (c, v, time.monotonic() - t0, len(left)))
        # 3. cancels around the hand-over
        tally = {}
        broken = 0
        for k, off in enumerate([2.90, 2.94, 2.97, 2.99, 3.00, 3.01, 3.03, 3.06, 3.10, 2.98, 3.00, 3.02]):
            n = BASE + 100 + 10 * k; marks.append(n)
            sid = s.session(ws)
            s.post("/api/sessions/%s/prompt" % sid, script([bg("sleep %d" % n, 600), fg("sleep %d" % (n + 1), 100)]))
            assert wait_for(lambda: sleeps(n)), "never started"
            # The call's clock started a little before the process appeared.
            t_seen = time.monotonic()
            time.sleep(max(0.0, off - 0.06))
            s.post("/api/sessions/%s/cancel" % sid, {})
            evs = s.wait_event(sid, "turn_ended")
            time.sleep(2.6)  # a cancelled command's stop: grace and kill
            evs = s.events(sid)
            started, ended = bg_account(evs)
            running = bool(sleeps(n))
            case = "adopted, runs on" if (started and not ended and running) else \
                   "not adopted, gone" if (not started and not running) else "BROKEN"
            broken += case == "BROKEN"
            tally[case] = tally.get(case, 0) + 1
            if case == "BROKEN":
                print("  cancel %.2f s after the start: started %s ended %s alive %s; %s"
                      % (off, started, ended, running, kinds(evs)[-200:]))
            if started and not ended:
                # A cancelled turn leaves it; the chat's deletion must end it.
                s.call("DELETE", "/api/sessions/%s" % sid)
                if gone_within(lambda: sleeps(n), 4.0):
                    broken += 1
                    print("  an adopted command outlived its chat's deletion")
        print("cancels 2.90 to 3.10 s after the start: %s; broken: %d" % (tally, broken))
        # 4. the node's stop at 1 s
        n = BASE + 20; marks.append(n)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([bg(escapers(n), 600)]))
        assert wait_for(lambda: len(alive_of(n)) == 5), "never started"
        time.sleep(0.7)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(60)
        left = gone_within(lambda: alive_of(n), 2.0)
        print("SIGTERM in the first wait: exit %r after %.1f s, processes left %d" % (code, time.monotonic() - t0, len(left)))
        s2 = Server(data)
        procs.append(s2)
        evs = s2.events(sid)
        print("  next start: started %s ends %s turns %s" % (bg_account(evs)[0], bg_account(evs)[1], ends(evs)))
        s2.signal(signal.SIGTERM)
        s2.exited(40)
    finally:
        secs = [m + i for m in marks for i in range(5)]
        k = cleanup(procs, needle=root, secs=secs)
        print("cleanup killed: %s" % (k or "nothing"))

main()
