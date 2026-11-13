#!/usr/bin/env python3
"""T.8d re-check, T8D-1: the memory of a real node process (`serve`), not of
a test process, after many background commands that ended.

node.toml: `background_per_session = 2`, `background_per_node = 2`,
`background_output_kb = 4096` (the first review's `finding_1`). One turn
makes 12 starts; each prints 4.5 MB 3.3 s into its command, after its
start's call returned, so nothing of it goes through the call's live
stream or into an event, and ends 0.4 s later. The script asks the server
for nothing while they run. The node's RSS is read before, each second,
and after.

    kept_rss.py          one chat, 12 starts
    kept_rss.py chats    6 chats one after another, 6 starts each (36)

On the reviewed code (`c4c852c`) the first form kept 12 rings of 4 MiB.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

LIMITS = "\n[limits]\nbackground_per_session = 2\nbackground_per_node = 2\nbackground_output_kb = 4096\n"
CMD = "sleep 3.3; head -c 4500000 /dev/zero | tr '\\0' x; sleep 0.4"


def main(how):
    chats, each = (6, 6) if how == "chats" else (1, 12)
    root = scratch("kept-" + how)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws, extra_node=LIMITS)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sids = [s.session(ws) for _ in range(chats)]
        for sid in sids:  # warm up: every chat has run a command
            s.post("/api/sessions/%s/prompt" % sid, script([fg("true")]))
            s.wait_event(sid, "turn_ended")
        time.sleep(1.0)
        rss0, series = rss_kib(s.pid), []
        for sid in sids:
            s.post("/api/sessions/%s/prompt" % sid, script([bg(CMD, 30) for _ in range(each)]))
            for _ in range(each * 3 + 6):
                series.append(rss_kib(s.pid) // 1024)
                time.sleep(1.0)
        rss1 = rss_kib(s.pid)
        started = ended = kept = 0
        for sid in sids:
            st, en = bg_account(s.events(sid))
            started += len(st)
            ended += sum(len(v) for v in en.values())
            calls = [{"tool": "bash_output", "input": {"id": i}} for i in st]
            s.post("/api/sessions/%s/prompt" % sid, script(calls))
            evs = wait_for(lambda: (lambda e: e if len(ends(e)) == 3 else None)(s.events(sid)), 60, 0.5)
            kept += len([r for r in results(evs)[-len(st):] if "no longer kept" not in r])
        print("node.toml: background_per_session = 2, background_per_node = 2, background_output_kb = 4096")
        print("chats %d, starts %d, ends %d, running now 0" % (chats, started, ended))
        print("the node's RSS each second, MiB: %s" % series)
        print("the node's RSS: %d KiB before, %d KiB after the last end: %+.1f MiB (peak %d MiB)"
              % (rss0, rss1, (rss1 - rss0) / 1024.0, max(series)))
        print("ended commands whose output can still be read: %d of %d (the node's count is 2)" % (kept, started))
        s.signal(signal.SIGTERM)
        print("stop: %r" % s.exited(40))
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main(sys.argv[1] if len(sys.argv) > 1 else "one")
