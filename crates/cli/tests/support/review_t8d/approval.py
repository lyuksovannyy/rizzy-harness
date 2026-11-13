#!/usr/bin/env python3
"""T.8d review: the approval path of a background start, in `ask`.

1. "Allow for this turn" given to a harmless foreground call does not let
   the same command through as a background start: a second card, whose
   reason says in words that it keeps running, until when, and that it can
   accept connections (the network is on).
2. A denied background start runs nothing.
3. `bash_output` and `bash_stop` ask for nothing in `ask` (read-only risk).
4. One click, one command (the first review's N-2: one "for this turn"
   covered every identical start of the turn). Fixed means: the card of a
   background start does not offer "for this turn", and an answer that
   says `allow_session` anyway covers that one call: the identical start
   asks again.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 978700


def pending(s, sid, seen):
    def f():
        for p in of(s.events(sid), "approval_requested"):
            if p["approval_id"] not in seen:
                return p
    p = wait_for(f, 20)
    assert p, "no approval card: %s" % kinds(s.events(sid))
    seen.add(p["approval_id"])
    return p


def answer(s, sid, p, decision):
    s.post("/api/sessions/%s/approvals/%s" % (sid, p["approval_id"]), {"decision": decision})


def main():
    root = scratch("approval")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws, mode="ask")
        seen = set()
        cmd = "sleep %d" % N
        s.post("/api/sessions/%s/prompt" % sid, script([
            fg(cmd, 5),
            {"tool": "bash", "input": {"timeout_secs": 99999, "command": cmd, "background": True}},
            {"tool": "bash_output", "input": {}},
        ]))
        p = pending(s, sid, seen)
        print("card 1 (foreground): input %s; reason: %s" % (json.dumps(p["input"]), p["reason"]))
        answer(s, sid, p, "allow_session")
        p = pending(s, sid, seen)
        print("card 2 (the same command, background): input %s" % json.dumps(p["input"]))
        print("    reason: %s" % p["reason"])
        answer(s, sid, p, "deny")
        evs = s.wait_event(sid, "turn_ended")
        time.sleep(0.3)
        print("denied: background_started %s; processes %d; cards in the turn: %d; bash_output asked: %s"
              % (bg_account(evs)[0], len(sleeps(N)), len(of(evs, "approval_requested")),
                 any(p["tool"] != "bash" for p in of(evs, "approval_requested"))))
        # 4. one click, one command
        sid2 = s.session(ws, mode="ask")
        seen = set()
        start = bg("sleep %d" % (N + 1), 600)
        s.post("/api/sessions/%s/prompt" % sid2, script([start, start, {"tool": "bash_stop", "input": {"id": "bg-1"}}]))
        p1 = pending(s, sid2, seen)
        answer(s, sid2, p1, "allow_session")
        p2 = pending(s, sid2, seen)
        running = len(sleeps(N + 1))
        answer(s, sid2, p2, "deny")
        evs = s.wait_event(sid2, "turn_ended", secs=60)
        print("two identical background starts, the first answered allow_session: cards %d (\"for this turn\" offered: %s); "
              "running when the second card came: %d; started %s, ends %s"
              % (len(of(evs, "approval_requested")), [p1.get("turn_allow"), p2.get("turn_allow")], running,
                 bg_account(evs)[0], bg_account(evs)[1]))
        print("N-2 fixed: %s" % (len(of(evs, "approval_requested")) == 2 and bg_account(evs)[0] == ["bg-1"]
                                 and not p1.get("turn_allow") and not p2.get("turn_allow")))
        s.signal(signal.SIGTERM)
        s.exited(40)
    finally:
        k = cleanup(procs, needle=root, secs=[N, N + 1])
        print("cleanup killed: %s" % (k or "nothing"))

main()
