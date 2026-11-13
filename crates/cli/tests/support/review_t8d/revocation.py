#!/usr/bin/env python3
"""T.8d review: the revocation sweep, end to end through the real master
and its node (the builder's tests send `session.stop_background` to a node
by hand; nothing ran the master's side).

A member starts background commands (the T.8a escapers) in an `auto` chat,
with no turn running afterwards. Then an admin takes the access away, in
three ways, each on a fresh member:

    disable the account | make them a plain member again | delete the account

(The server's own node takes agentic chats from server admins only, so the
member is made an admin first; the sweep is the same one for every way of
losing access.)

Expected for each: `background_ended { reason: access_revoked }` and
nothing of the command left. Also: what another member is told about the
owner's chat (events, and the list), beside an id that does not exist.
"""
import os, signal, sys, time, uuid
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

BASE = 978800


def escapers(n):
    return ("setsid sleep %d & nohup sleep %d >/dev/null 2>&1 & ( setsid sleep %d & ) ; "
            "sh -c 'trap \"\" TERM; exec sleep %d' & sleep %d" % (n, n + 1, n + 2, n + 3, n + 4))


def alive_of(n):
    return [p for i in range(5) for p in sleeps(n + i)]


def main():
    root = scratch("revoke")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs, marks = [], []
    try:
        s = Server(data)
        procs.append(s)
        admin = Browser.claim(s, "alex")
        other = admin.member("olga")
        c, users = admin.call("GET", "/api/users")
        ways = [("disable the account", lambda uid: admin.call("PATCH", "/api/users/%s" % uid, {"disabled": True})),
                ("make them a member again", lambda uid: admin.call("PATCH", "/api/users/%s" % uid, {"role": "member"})),
                ("delete the account", lambda uid: admin.call("DELETE", "/api/users/%s" % uid))]
        for k, (name, revoke) in enumerate(ways):
            n = BASE + 10 * k
            marks.append(n)
            who = "bob%d" % k
            bob = admin.member(who)
            c, users = admin.call("GET", "/api/users")
            ulist = users["users"] if isinstance(users, dict) else users
            uid = [u for u in ulist if u["username"] == who][0]["user_id"]
            c, v = admin.call("PATCH", "/api/users/%s" % uid, {"role": "admin"})
            assert c == 200, (c, v)
            c, v = bob.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
            if c != 200:
                print("%s: the member could not make an agentic chat: %s %s" % (name, c, v))
                continue
            sid = v["session_id"]
            c, v = bob.call("POST", "/api/sessions/%s/prompt" % sid, script([bg(escapers(n), 3600)]))
            assert c == 200, (c, v)
            s.wait_event(sid, "turn_ended", secs=60)
            assert len(alive_of(n)) == 5, "the background command is not running"
            if k == 0:
                # What another member learns of it.
                nothing = str(uuid.uuid4())
                for what, path in (("events", "/api/sessions/%s/events"), ("session", "/api/sessions/%s")):
                    a, b = other.call("GET", path % sid), other.call("GET", path % nothing)
                    print("another member, %s: the owner's chat %s %s | no such chat %s %s%s"
                          % (what, a[0], str(a[1])[:50], b[0], str(b[1])[:50], "" if a == b else "  DIFFERENT"))
                c, lst = other.call("GET", "/api/sessions")
                print("another member's list of chats names it: %s" % (sid in json.dumps(lst)))
            t0 = time.monotonic()
            c, v = revoke(uid)
            left = gone_within(lambda: alive_of(n), 8.0)
            took = time.monotonic() - t0
            time.sleep(0.5)
            try:
                started, ended = bg_account(s.events(sid))
            except RuntimeError as e:
                started, ended = "?", "the chat is gone (%s)" % str(e)[:60]
            print("%s (%s): processes left %d after %.1f s; started %s, ends %s"
                  % (name, c, len(left), took, started, ended))
        # Re-check: the two moments the first review's N-1 was about. No
        # count decides any more: the stop goes to every chat of a blocked
        # owner, and closes its table.
        for k, (name, wait_for_turn) in enumerate([
                ("disable 1 s into a start's first wait", False),
                ("disable while a foreground step runs after a start", True)], start=3):
            n = BASE + 10 * k
            marks.append(n)
            who = "bob%d" % k
            bob = admin.member(who)
            c, users = admin.call("GET", "/api/users")
            ulist = users["users"] if isinstance(users, dict) else users
            uid = [u for u in ulist if u["username"] == who][0]["user_id"]
            c, v = admin.call("PATCH", "/api/users/%s" % uid, {"role": "admin"})
            assert c == 200, (c, v)
            c, v = bob.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
            assert c == 200, (c, v)
            sid = v["session_id"]
            c, v = bob.call("POST", "/api/sessions/%s/prompt" % sid,
                            script([bg(escapers(n), 3600), fg("sleep %d" % (n + 5), 300), bg("sleep %d" % (n + 6), 3600)]))
            assert c == 200, (c, v)
            assert wait_for(lambda: len(alive_of(n)) == 5), "never started"
            if wait_for_turn:
                assert wait_for(lambda: sleeps(n + 5), 20), "the foreground step never started"
                time.sleep(0.3)
            else:
                time.sleep(0.7)
            c, v = admin.call("PATCH", "/api/users/%s" % uid, {"disabled": True})
            left = gone_within(lambda: alive_of(n) + sleeps(n + 5) + sleeps(n + 6), 8.0)
            time.sleep(4.0)  # were the turn to go on, its next start would be running by now
            late = alive_of(n) + sleeps(n + 5) + sleeps(n + 6)
            evs = s.events(sid)
            started, ended = bg_account(evs)
            print("%s (%s): processes left %d, 4 s later %d; started %s, ends %s, turn %s"
                  % (name, c, len(left), len(late), started, ended, ends(evs)))
        s.signal(signal.SIGTERM)
        s.exited(40)
    finally:
        secs = [m + i for m in marks for i in range(7)]
        k = cleanup(procs, needle=root, secs=secs)
        print("cleanup killed: %s" % (k or "nothing"))

main()
