#!/usr/bin/env python3
"""T.8d review: exactly one end, through the node, when the command's own
exit, its timeout (5 s) and the agent's `bash_stop` fall together.

Each turn: a background start with `timeout_secs: 5` whose command sleeps
`d` seconds, a foreground `sleep w`, then `bash_stop`. A first turn
measures how long after the start's call the `bash_stop` call begins on
this machine; `w` is then chosen so the stop lands 5 s after the start,
within a tenth either way, and `d` is 5 s (the exit joins the race) or 60 s
(the timeout and the stop alone). Four chats at once. Every start must have
exactly one end, and what `bash_stop` says must agree with the event.
"""
import os, signal, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

JITTER = [-0.10, -0.07, -0.05, -0.03, -0.02, -0.01, 0.0, 0.01, 0.02, 0.03, 0.05, 0.08]


def one(s, sid, j, d, w):
    s.post("/api/sessions/%s/prompt" % sid, script([
        bg("sleep %s" % d, 5), fg("sleep %.2f" % w),
        {"tool": "bash_stop", "input": {"id": "bg-%d" % j}}]))
    wait_for(lambda: len(ends(s.events(sid))) == j, 40, 0.1)
    time.sleep(1.5)
    calls = [e for e in s.events(sid) if e["kind"] == "tool_call_started"][-3:]
    return (calls[2]["ts_ms"] - calls[0]["ts_ms"]) / 1000.0


def main():
    root = scratch("races")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        cal = s.session(ws)
        gap = one(s, cal, 1, 60, 1.0)
        base = 1.0 + (5.0 - gap)
        print("a stop after `sleep 1.0` begins %.2f s after the start's call here; aiming with sleep %.2f" % (gap, base))
        sids = [s.session(ws) for _ in range(4)]
        gaps = []

        def chat(i, sid):
            for j, jit in enumerate(JITTER[i::4] + JITTER[i::4]):
                d = "4.9%d" % (5 + i) if j % 2 == 0 else "60"
                gaps.append(one(s, sid, j + 1, d, base + jit))
        ts = [threading.Thread(target=chat, args=(i, sid)) for i, sid in enumerate(sids)]
        for t in ts:
            t.start()
        for t in ts:
            t.join()
        time.sleep(3.0)
        tally, wrong, n, said = {}, 0, 0, {}
        for sid in sids:
            evs = s.events(sid)
            started, ended = bg_account(evs)
            stops = [r for r in results(evs) if r.startswith("[bg-") or "bg-" in r[:40] and "stop" in r]
            for i in started:
                n += 1
                r = ended.get(i, [])
                wrong += len(r) != 1
                tally[tuple(r)] = tally.get(tuple(r), 0) + 1
            wrong += len([i for i in ended if i not in started])
            for p, r in zip(of(evs, "background_ended"), sorted(stops, key=lambda r: int(r.split("bg-")[1].split()[0].rstrip(":")))):
                pass
            for r in stops:
                k = "already ended" if "had already ended" in r else "stopped by this call" if "was stopped by the agent" in r else \
                    "the timeout's end" if "reached its timeout" in r else "the exit" if "exited" in r else r[:60]
                said[k] = said.get(k, 0) + 1
        print("the stop began %.2f to %.2f s after the start" % (min(gaps), max(gaps)))
        print("starts: %d; ends per start by reason: %s; starts without exactly one end: %d" % (n, tally, wrong))
        print("what bash_stop said: %s" % said)
        s.signal(signal.SIGTERM)
        s.exited(40)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
