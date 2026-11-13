#!/usr/bin/env python3
"""T.8d review: the ring's bound on the node's memory while commands run:
eight floods at once in one chat (the default cap), for 20 s: binary,
one endless line, invalid UTF-8, stderr, short lines. The node's RSS is
sampled every second. Then each is read and stopped, and the events are
read back.

Each flood starts 3.3 s into its command, after its start's call returned,
so nothing of it goes through the call's live stream (`tool_stream_bytes`,
1 MiB a call, stored as events: a foreground call's cost, not measured
here), and the script asks the server for nothing while it samples: a
first version polled the chat's events meanwhile and measured the size of
its own answers.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

FLOODS = [
    "cat /dev/urandom",
    "tr '\\0' a </dev/zero",
    "while :; do printf '\\377\\376\\200\\301'; done",
    "cat /dev/urandom >&2",
    "yes",
    "yes \"$(head -c 100000 /dev/zero | tr '\\0' b)\"",
    "cat /dev/zero",
    "while :; do printf '\\033[2J\\033]0;title\\007\\r\\b'; done",
]


def main():
    root = scratch("flood")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([fg("true")]))
        s.wait_event(sid, "turn_ended")
        rss0 = rss_kib(s.pid)
        s.post("/api/sessions/%s/prompt" % sid, script([bg("sleep 3.3; " + c, 600) for c in FLOODS]))
        peak, t0, series = rss0, time.monotonic(), []
        for _ in range(8 * 3 + 5 + 20):
            r = rss_kib(s.pid)
            series.append(r // 1024)
            peak = max(peak, r)
            time.sleep(1.0)
        print("the node's RSS each second, MiB: %s" % series)
        started, _ = bg_account(s.events(sid))
        print("floods running: %d for %.0f s; the node's RSS %d KiB before, peak %d KiB (+%.1f MiB; 8 rings of 256 KiB are 2 MiB)"
              % (len(started), time.monotonic() - t0, rss0, peak, (peak - rss0) / 1024.0))
        calls = [{"tool": "bash_output", "input": {}}]
        calls += [{"tool": "bash_output", "input": {"id": i}} for i in started]
        calls += [{"tool": "bash_stop", "input": {"id": i}} for i in started]
        s.post("/api/sessions/%s/prompt" % sid, script(calls))
        time.sleep(17 * 0.6 + 8 * 2.5 + 5)
        after = rss_kib(s.pid)
        evs = wait_for(lambda: (lambda e: e if len(ends(e)) == 3 else None)(s.events(sid)), 120, 2.0)
        res = results(evs)
        print("bash_output (list), first line: %s" % res[len(FLOODS) + 1].split("\n")[0][:160])
        sizes = [len(r) for r in res[len(FLOODS) + 2:len(FLOODS) + 2 + len(started)]]
        print("bash_output results, characters each (the cap is 30000): %s" % sizes)
        _, ended = bg_account(evs)
        tails = [len(p["tail"].encode()) for p in of(evs, "background_ended")]
        print("ends: %s" % ended)
        print("tails in the events, bytes each (4096 of output; invalid bytes become U+FFFD, 3 bytes each): %s; the node's RSS after the reads and stops %d KiB" % (tails, after))
        s.signal(signal.SIGTERM)
        print("stop: %r" % s.exited(40))
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
