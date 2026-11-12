#!/usr/bin/env python3
"""A.19 review: what each request gets while `serve` stops, and what the
next start finds. One turn's command ignores SIGTERM, so the stop waits
about 2 s (the shell tool's grace) with HTTP still up; in that window, as
the owner: a prompt, a prompt with an uploaded attachment, a new session, a
restore, a cancel, a delete, an approval (a routine's "run now" needs a
user, not the API token: `stop_users_routines.py`).

Prints the answers, the exit status, and after a second start: the ends in
every log, and whether the refused prompt can be "sent again" as the
refusal says.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 976000 + os.getpid() % 900


def main():
    root = scratch("window")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        busy, idle, gone, restorable = (s.session(ws) for _ in range(4))
        asking = s.session(ws, mode="ask")
        # A finished turn with a checkpoint, to restore.
        s.post("/api/sessions/%s/prompt" % restorable, {"text": "run: echo one > file.txt"})
        evs = s.wait_event(restorable, "turn_ended")
        turn = [e for e in evs if e["kind"] == "turn_started"][0]["payload"]["turn_id"]
        # An upload waiting to be sent.
        import urllib.request
        req = urllib.request.Request("http://%s/api/attachments?name=note.txt" % s.addr,
                                     data=b"the note's text", method="POST")
        req.add_header("Authorization", "Bearer " + s.token)
        att = json.loads(urllib.request.urlopen(req).read())["attachment_id"]
        s.post("/api/sessions/%s/prompt" % asking, {"text": "run: touch must-not-exist"})
        evs = s.wait_event(asking, "approval_requested")
        approval = [e for e in evs if e["kind"] == "approval_requested"][0]["payload"]["approval_id"]
        s.post("/api/sessions/%s/prompt" % busy, {"text": "run: trap '' TERM; sleep %d" % N})
        assert wait_for(lambda: sleeps(N)), "the command never started"
        before = len(s.call("GET", "/api/sessions")[1].get("sessions", []))

        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        s.wait_line("stopping the server", where="err")
        time.sleep(0.25)
        def show(what, cv):
            print("  %-34s -> %s %s" % (what, cv[0], str(cv[1])[:150]))
        print("while stopping (%.2f s after SIGTERM):" % (time.monotonic() - t0))
        show("prompt, idle session", s.call("POST", "/api/sessions/%s/prompt" % idle, {"text": "hello"}))
        show("prompt with the attachment", s.call("POST", "/api/sessions/%s/prompt" % idle,
                                                  {"text": "read it", "attachments": [att]}))
        show("new session", s.call("POST", "/api/sessions", {"cwd": ws}))
        show("restore", s.call("POST", "/api/sessions/%s/turns/%s/restore" % (restorable, turn), {}))
        show("approval: allow", s.call("POST", "/api/sessions/%s/approvals/%s" % (asking, approval),
                                       {"decision": "allow"}))
        show("cancel, busy session", s.call("POST", "/api/sessions/%s/cancel" % busy, {}))
        show("delete, another session", s.call("DELETE", "/api/sessions/%s" % gone))
        show("events, busy session", (s.call("GET", "/api/sessions/%s/events" % busy)[0], ""))
        after = len(s.call("GET", "/api/sessions")[1].get("sessions", []))
        print("  sessions listed before/after the refused create and the delete: %d/%d" % (before, after))
        code = s.exited(70)
        print("exit status %r, %.1f s after SIGTERM; %s" % (code, time.monotonic() - t0, s.log(1)[-60:]))
        print("must-not-exist exists: %s" % os.path.exists(os.path.join(ws, "must-not-exist")))

        s2 = Server(data)
        procs.append(s2)
        print("after the next start:")
        for name, sid in (("busy", busy), ("idle", idle), ("asking", asking), ("restorable", restorable)):
            print("  %-10s %s" % (name, kinds(s2.events(sid))[-230:]))
        show("the refused prompt, sent again", s2.call("POST", "/api/sessions/%s/prompt" % idle,
                                                       {"text": "read it", "attachments": [att]}))
        s2.signal(signal.SIGTERM)
        print("the second server stops with %r" % s2.exited(30))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))

main()
