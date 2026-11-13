#!/usr/bin/env python3
"""T.8d step 2 review: who may call the person's stop, and what the route
tells anyone else. The real binary, its own node, bubblewrap.

A team workspace: alex (server admin, the workspace's owner), wanda (a
workspace admin), olga (a member), bob (a member with the agentic grant;
`[nodes] local_node_users = "granted"` lets him use the server's own node).
Bob's chat in that workspace runs `sleep 979001` in the background.
"""
import os, signal, statistics, sys, time, uuid
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from step2_common import *

M = 979001


def main():
    root = scratch("authz")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws, cfg_extra='\n[nodes]\nlocal_node_users = "granted"\n')
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        bob, bob_id = granted(alex, "bob")
        olga, olga_id = granted(alex, "olga")
        wanda, wanda_id = granted(alex, "wanda")
        c, w = alex.call("POST", "/api/workspaces", {"name": "Team"})
        assert c == 200, (c, w)
        wid = w.get("workspace_id") or w.get("id")
        for name, role in (("bob", "member"), ("olga", "member"), ("wanda", "admin")):
            c, v = alex.call("POST", "/api/workspaces/%s/members" % wid, {"username": name, "role": role})
            assert c == 200, (name, c, v)
        c, v = bob.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto", "workspace_id": wid})
        assert c == 200, (c, v)
        sid = v["session_id"]
        c, v = bob.call("POST", "/api/sessions/%s/prompt" % sid, script([bg("sleep %d" % M, 3600), bg("sleep %d" % (M + 1), 3600)]))
        assert c == 200, (c, v)
        s.wait_event(sid, "turn_ended", secs=60)
        assert sleeps(M) and sleeps(M + 1), "the commands do not run"
        nothing = str(uuid.uuid4())
        one, allp = "/api/sessions/%s/background/bg-1/stop", "/api/sessions/%s/background/stop"

        print("== who is refused, and with what (bob's chat | a chat id that does not exist) ==")
        who = [("olga, a member of the same workspace", page(olga)),
               ("wanda, a workspace admin", page(wanda)),
               ("alex, server admin and the workspace's owner", page(alex)),
               ("signed out (no cookie, the page's Origin)", {"Origin": "http://" + s.addr}),
               ("bob's cookie without its login key", page(bob, x_harness_login=None)),
               ("bob's cookie with olga's login key", page(bob, x_harness_login=olga.key)),
               ("bob's key with olga's cookie", page(olga, x_harness_login=bob.key)),
               ("bob, no Origin", page(bob, Origin=None)),
               ("bob, Origin of another port of this host", page(bob, Origin="http://127.0.0.1:%d" % free_port())),
               ("bob, Origin https:// of the same host", page(bob, Origin="https://" + s.addr)),
               ("bob, Origin null", page(bob, Origin="null")),
               ("bob, Origin http://evil.example", page(bob, Origin="http://evil.example")),
               ("bob, Host evil.example:80", page(bob, Host="evil.example")),
               ("a wrong API token", {"Authorization": "Bearer " + "0" * 64}),
               ("a wrong API token beside bob's cookie and key", dict(page(bob), Authorization="Bearer " + "0" * 64))]
        for name, h in who:
            for path in (one, allp):
                a = raw(s.addr, "POST", path % sid, h)
                b = raw(s.addr, "POST", path % nothing, h)
                same = (a[0], a[1]) == (b[0], b[1])
                print("  %-52s %-4s %s %s | %s %s%s" % (name, "one" if path is one else "all", a[0], short(a[1], 46),
                                                         b[0], short(b[1], 30), "" if same else "   <-- DIFFERENT"))
        print("  the commands still run: %s" % (bool(sleeps(M)) and bool(sleeps(M + 1))))

        print("== preflight and what a cross-origin page gets ==")
        other = "http://127.0.0.1:%d" % free_port()
        a = raw(s.addr, "OPTIONS", one % sid, {"Origin": other, "Access-Control-Request-Method": "POST",
                                                "Access-Control-Request-Headers": "x-harness-login"})
        print("  OPTIONS from %s: %s %r; Access-Control-* headers: %s"
              % (other, a[0], a[1], [k for k in a[2] if k.lower().startswith("access-control")]))
        a = raw(s.addr, "OPTIONS", one % sid, {"Origin": "http://" + s.addr, "Access-Control-Request-Method": "POST"})
        print("  OPTIONS from the page's own origin: %s; Access-Control-* headers: %s"
              % (a[0], [k for k in a[2] if k.lower().startswith("access-control")]))
        for m in ("GET", "DELETE", "PUT", "PATCH", "HEAD"):
            a = raw(s.addr, m, one % sid, page(bob))
            print("  %s by bob: %s %s" % (m, a[0], short(a[1], 60)))

        print("== timing: another member asks for bob's chat, and for a chat that does not exist (400 each, alternating) ==")
        ta, tb = [], []
        h = page(olga)
        for i in range(400):
            ta.append(raw(s.addr, "POST", one % sid, h)[3])
            tb.append(raw(s.addr, "POST", one % str(uuid.uuid4()), h)[3])
        def q(xs, p):
            xs = sorted(xs)
            return xs[int(len(xs) * p)] * 1000
        print("  bob's chat:    median %.2f ms, p10 %.2f, p90 %.2f" % (q(ta, .5), q(ta, .1), q(ta, .9)))
        print("  no such chat:  median %.2f ms, p10 %.2f, p90 %.2f" % (q(tb, .5), q(tb, .1), q(tb, .9)))

        print("== the ids in the path, by bob (the owner) ==")
        ids = ["bg-01", "bg-0", "bg-+1", "bg--1", "bg-1%20", "%62g-1x", "BG-1", "bg-1%00", "bg-%EF%BC%91", "bg-1e0",
               "bg-0x1", "bg-999999999", "bg-1000000000", "bg-" + "9" * 5000, "..%2F..%2Fcancel", "bg-1%2Fstop", "bg-3"]
        for i in ids:
            a = raw(s.addr, "POST", "/api/sessions/%s/background/%s/stop" % (sid, i), page(bob))
            print("  %-22s %s %s" % (short(i, 20), a[0], short(a[1], 120)))
        print("  the same odd ids by olga (not the owner):")
        for i in ["bg-+1", "bg-" + "9" * 5000, "bg-3"]:
            a = raw(s.addr, "POST", "/api/sessions/%s/background/%s/stop" % (sid, i), page(olga))
            print("  %-22s %s %s" % (short(i, 20), a[0], short(a[1], 60)))
        print("  the chat id written other ways, by bob then by olga (bg-3: nothing is stopped):")
        forms = [sid.upper(), sid.replace("-", ""), "{%s}" % sid, "urn:uuid:" + sid, sid + "%20", sid + "/", "%2e%2e/" + sid, sid[:-1]]
        for f in forms:
            a = raw(s.addr, "POST", "/api/sessions/%s/background/bg-3/stop" % f, page(bob))
            b = raw(s.addr, "POST", "/api/sessions/%s/background/bg-3/stop" % f, page(olga))
            print("  %-48s bob %s %s | olga %s %s" % (short(f, 46), a[0], short(a[1], 50), b[0], short(b[1], 30)))
        print("  the commands still run: %s" % (bool(sleeps(M)) and bool(sleeps(M + 1))))

        print("== the owner, and the machine owner's API token ==")
        a = raw(s.addr, "POST", one % sid, page(bob))
        gone = not gone_within(lambda: sleeps(M), 5)
        print("  bob stops bg-1: %s %s; its process is gone: %s; bg-2 runs: %s" % (a[0], a[1], gone, bool(sleeps(M + 1))))
        a = raw(s.addr, "POST", allp % sid, {"Authorization": "Bearer " + s.token})
        gone = not gone_within(lambda: sleeps(M + 1), 5)
        print("  the API token, Stop all: %s %s; bg-2 is gone: %s" % (a[0], a[1], gone))
        wait_for(lambda: len(bg_account(s.events(sid))[1]) == 2, 10, 0.05)
        print("  the log: %s" % (bg_account(s.events(sid)),))

        def turns(i):
            return len(ends(s.events(i)))

        def run(who, i, calls):
            n = turns(i)
            c, v = who.call("POST", "/api/sessions/%s/prompt" % i, script(calls))
            assert c == 200, (c, v)
            assert wait_for(lambda: turns(i) > n, 60, 0.05), "the turn did not end"

        print("== bob loses access with a command running and no turn; then tries to stop ==")
        # 1. removed from the workspace: the chat goes back to his Personal
        # workspace, where he may still run agentic chats.
        run(bob, sid, [bg("sleep %d" % (M + 2), 3600)])
        c, v = alex.call("DELETE", "/api/workspaces/%s/members/%s" % (wid, bob_id))
        time.sleep(3.0)
        a = raw(s.addr, "POST", "/api/sessions/%s/background/bg-3/stop" % sid, page(bob))
        left = gone_within(lambda: sleeps(M + 2), 8)
        wait_for(lambda: "bg-3" in bg_account(s.events(sid))[1], 10, 0.05)
        print("  removed from the workspace (%s): 3 s later the command runs: %s (the chat is his Personal one again, where he may run it)"
              % (c, a[0] == 200 and "bg-3" in a[1]))
        print("    bob's stop of bg-3: %s %s; processes left %d; log %s"
              % (a[0], short(a[1], 80), len(left), bg_account(s.events(sid))[1].get("bg-3")))
        # 2. the agentic grant taken away: the sweep stops it; the stop still answers.
        run(bob, sid, [bg("sleep %d" % (M + 5), 3600)])
        c, v = alex.call("PATCH", "/api/users/%s" % bob_id, {"can_agentic": False})
        left = gone_within(lambda: sleeps(M + 5), 8)
        wait_for(lambda: "bg-4" in bg_account(s.events(sid))[1], 10, 0.05)
        a = raw(s.addr, "POST", "/api/sessions/%s/background/bg-4/stop" % sid, page(bob))
        b = raw(s.addr, "POST", allp % sid, page(bob))
        print("  the agentic grant taken away (%s): the sweep left %d process(es); log %s"
              % (c, len(left), bg_account(s.events(sid))[1].get("bg-4")))
        print("    bob's stop of bg-4 afterwards: %s %s" % (a[0], short(a[1], 170)))
        print("    bob's Stop all afterwards:     %s %s" % (b[0], short(b[1], 100)))
        c, v = bob.call("POST", "/api/sessions/%s/prompt" % sid, script([bg("sleep %d" % (M + 5), 3600)]))
        print("    bob's prompt afterwards: %s %s" % (c, short(v, 80)))
        c, v = alex.call("PATCH", "/api/users/%s" % bob_id, {"can_agentic": True})
        # 3. the chat deleted
        c, v = bob.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        sid2 = v["session_id"]
        run(bob, sid2, [bg("sleep %d" % (M + 3), 3600)])
        c, v = bob.call("DELETE", "/api/sessions/%s" % sid2)
        left = gone_within(lambda: sleeps(M + 3), 8)
        a = raw(s.addr, "POST", one % sid2, page(bob))
        b = raw(s.addr, "POST", allp % sid2, page(bob))
        print("  the chat deleted (%s): %d process(es) left; a stop of it by bob: %s %s | all: %s %s"
              % (c, len(left), a[0], a[1], b[0], b[1]))
        # 4. the account disabled
        c, v = bob.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        sid3 = v["session_id"]
        run(bob, sid3, [bg("sleep %d" % (M + 4), 3600)])
        c, v = alex.call("PATCH", "/api/users/%s" % bob_id, {"disabled": True})
        left = gone_within(lambda: sleeps(M + 4), 8)
        wait_for(lambda: bg_account(s.events(sid3))[1], 10, 0.05)
        a = raw(s.addr, "POST", one % sid3, page(bob))
        print("  the account disabled (%s): the sweep left %d process(es); bob's stop afterwards: %s %s; log %s"
              % (c, len(left), a[0], a[1], bg_account(s.events(sid3))[1]))

        print("== the server's log lines about the stops (the new info line) ==")
        lines = [l for _, l in s.err if "stops background" in l]
        print("  %d line(s); the first two:" % len(lines))
        for l in lines[:2]:
            print("    " + l[-170:])
        s.signal(signal.SIGTERM)
        print("  the server's exit status: %r" % s.exited(40))
    finally:
        k = cleanup(procs, needle=root, secs=[M + i for i in range(6)])
        print("cleanup killed: %s" % (k or "nothing"))

main()
