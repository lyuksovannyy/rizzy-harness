#!/usr/bin/env python3
"""A.19 re-check, N-7: which end a stop records.

  cancel-first   the person cancels the turn (its command ignores SIGTERM,
                 so it takes 2 s to close), then SIGTERM 0.2 s later
  stop-first     SIGTERM, then the person's cancel 0.2 s later
  completed      the turn's command and the model's last message are done;
                 the after-turn checkpoint is running (a 15 GB sparse file
                 keeps it busy for about 20 s) when SIGTERM comes
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 984000 + os.getpid() % 900


def case(name, i):
    root = scratch("ends%d" % i)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        if name == "completed":
            s.post("/api/sessions/%s/prompt" % sid, {"text": "run: truncate -s 15G big; echo made"})
            # The model's answer after the tool, then the checkpoint.
            def answered():
                evs = s.events(sid)
                return [e for e in evs if e["kind"] == "assistant_message"][1:] and not ends(evs)
            assert wait_for(answered, 30, 0.02), kinds(s.events(sid))
            time.sleep(1.0)
            before = kinds(s.events(sid))[-60:]
            s.signal(signal.SIGTERM)
        else:
            s.post("/api/sessions/%s/prompt" % sid, {"text": "run: trap '' TERM; sleep %d" % (N + i)})
            assert wait_for(lambda: sleeps(N + i)), "the command never started"
            before = ""
            if name == "cancel-first":
                c, _ = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
                time.sleep(0.2)
                s.signal(signal.SIGTERM)
            else:
                s.signal(signal.SIGTERM)
                time.sleep(0.2)
                c, _ = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
        t0 = time.monotonic()
        code = s.exited(120)
        took = time.monotonic() - t0
        s2 = Server(data)
        procs.append(s2)
        evs = s2.events(sid)
        print("%-12s exit %r in %.1f s; the turn's end: %s%s"
              % (name, code, took, ends(evs), ("; the log when the signal came: …" + before) if before else ""))
        s2.signal(signal.SIGTERM)
        s2.exited(120)
    finally:
        cleanup(procs, needle=root, secs=[N + i])
        try:
            os.unlink(os.path.join(ws, "big"))
        except OSError:
            pass


for i, name in enumerate(["cancel-first", "stop-first", "completed"]):
    case(name, i)
