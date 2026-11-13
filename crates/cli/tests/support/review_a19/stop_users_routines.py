#!/usr/bin/env python3
"""A.19 review: two users and a routine while `serve` stops.

1. What another signed-in user gets for the owner's running chat, before the
   stop and during it, beside what they get for an id that does not exist
   (chats are private to their owner: the two must not differ, and no answer
   may say "stopping" for a chat that is not theirs).
2. A routine's "run now" during the stop: what the caller is told and what
   the next start makes of the run (claim 4: not failed for good by a stop;
   what was due fires at the next start). At `5add735` it was taken (202)
   and ended `interrupted` at the next start; since `859e59c` it is answered
   503 and no run is recorded.
"""
import os, signal, sys, time, uuid
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 977000 + os.getpid() % 900


def probe(user, sid, turn):
    return [
        ("prompt", user.call("POST", "/api/sessions/%s/prompt" % sid, {"text": "hello"})),
        ("cancel", user.call("POST", "/api/sessions/%s/cancel" % sid, {})),
        ("approve", user.call("POST", "/api/sessions/%s/approvals/x" % sid, {"decision": "allow"})),
        ("restore", user.call("POST", "/api/sessions/%s/turns/%s/restore" % (sid, turn), {})),
        ("events", user.call("GET", "/api/sessions/%s/events" % sid)),
        ("delete", user.call("DELETE", "/api/sessions/%s" % sid)),
    ]


def main():
    root = scratch("users")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        bob = alex.member("bob")
        c, v = alex.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        assert c == 200, (c, v)
        busy = v["session_id"]
        nothing = str(uuid.uuid4())
        c, r = alex.call("POST", "/api/routines", {
            "name": "review", "triggers": [{"kind": "webhook", "config": {}}],
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "say hello"}})
        assert c == 201, (c, r)
        rid = r["routine"]["routine_id"]
        c, v = alex.call("POST", "/api/sessions/%s/prompt" % busy,
                         {"text": "run: trap '' TERM; sleep %d" % N})
        assert c == 200, (c, v)
        assert wait_for(lambda: sleeps(N)), "the command never started"
        calm = (probe(bob, busy, "t"), probe(bob, nothing, "t"))
        s.signal(signal.SIGTERM)
        s.wait_line("stopping the server", where="err")
        time.sleep(0.2)
        stopping = (probe(bob, busy, "t"), probe(bob, nothing, "t"))
        run = alex.call("POST", "/api/routines/%s/run" % rid, {})
        own = alex.call("POST", "/api/sessions/%s/prompt" % busy, {"text": "more"})
        code = s.exited(70)
        print("another user, the owner's running chat | an id that does not exist:")
        differ = 0
        for when, (theirs, none) in (("before the stop", calm), ("while stopping", stopping)):
            for (what, a), (_, b) in zip(theirs, none):
                same = a == b
                differ += not same
                print("  %-15s %-8s %s %-34s | %s %-34s %s"
                      % (when, what, a[0], str(a[1])[:34], b[0], str(b[1])[:34], "" if same else "DIFFERENT"))
        print("answers that differ: %d" % differ)
        print("the owner's own prompt to the busy chat while stopping: %s %s" % (own[0], str(own[1])[:90]))
        print("routine, run now, while stopping: %s %s" % run)
        print("exit status %r" % code)

        s2 = Server(data)
        procs.append(s2)
        alex2 = alex.on(s2)
        time.sleep(1.5)
        c, v = alex2.call("GET", "/api/routines/%s/runs" % rid)
        print("after the next start, the routine's runs: %s" % (c if c != 200 else ""))
        for run in (v.get("runs") or []) if c == 200 else []:
            print("  status %s, attempt %s, reason %r" % (run.get("status"), run.get("attempt"), run.get("reason")))
        c, v = alex2.call("GET", "/api/routines/%s" % rid)
        rr = v.get("routine", v) if c == 200 else {}
        print("  the routine: %s" % {k: rr.get(k) for k in ("failures", "paused_reason", "enabled", "last_status")})
        time.sleep(65)
        c, v = alex2.call("GET", "/api/routines/%s/runs" % rid)
        print("65 s later (past the 1 min retry): %s"
              % [(r.get("status"), r.get("reason")) for r in (v.get("runs") or [])])
        s2.signal(signal.SIGTERM)
        print("the second server stops with %r" % s2.exited(30))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))

main()
