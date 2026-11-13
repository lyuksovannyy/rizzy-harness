#!/usr/bin/env python3
"""A.19 review, claim 4, and its re-check: a routine whose run is cut by a
stop, on the real `serve`.

At `5add735`: the run was finished `interrupted`, counted as a failure, and
five stops paused the routine.

Since `859e59c` the run is put back (`pending`, a new chat) and launched
again at the next start. Two parts:

  stops    one run of a chat routine, cut by five SIGTERMs in a row, then
           left to finish: its status and reason after each start, the
           routine's failure counter, the number of runs and of chats.
  webhook  a run a webhook fired, whose prompt uses the event
           (`{{event.text}}`), cut once: the prompt and the `untrusted`
           mark of the run before the stop and of the run after it.

  storm    the put-back run, then twelve starts each stopped or killed within
           half a second of listening (during the start-up reconcile and
           the relaunch), then left alone: how often the run's turn ran.
           Without `kills` every one of them is a clean SIGTERM.

  window   a put-back run, the next start stopped d ms after it listens.

  launch   a clean stop in the moment a run's row first says `running`.

    routine_stops.py [stops] [webhook] [storm [kills]] [window] [launch]     kills: every fourth start is killed (-9) instead
"""
import os, signal, sqlite3, sys, time, urllib.request, urllib.error
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def row(data, rid):
    db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
    try:
        return db.execute("SELECT consecutive_failures, paused_reason FROM routines WHERE routine_id = ?",
                          (rid,)).fetchone()
    finally:
        db.close()


def runs(user, rid):
    c, v = user.call("GET", "/api/routines/%s/runs?limit=200" % rid)
    return v.get("runs", []) if c == 200 else []


def run_of(user, rid, run_id):
    c, v = user.call("GET", "/api/routines/%s/runs/%s" % (rid, run_id))
    return v if c == 200 else {}


def stops():
    root = scratch("routine")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        c, r = alex.call("POST", "/api/routines", {
            "name": "long", "triggers": [{"kind": "webhook", "config": {}}],
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "slow: 9000"}})
        assert c == 201, (c, r)
        rid = r["routine"]["routine_id"]
        c, v = alex.call("POST", "/api/routines/%s/run" % rid, {})
        assert c == 202, (c, v)
        run_id = v["run_id"]
        chats = []
        for i in range(1, 6):
            def running():
                r = run_of(alex, rid, run_id)
                return r if r.get("status") == "running" and r.get("session_id") not in chats else None
            r = wait_for(running, 30)
            assert r, run_of(alex, rid, run_id)
            chats.append(r["session_id"])
            time.sleep(0.3)
            s.signal(signal.SIGTERM)
            code = s.exited(100)
            pending = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True).execute(
                "SELECT status, reason, session_id FROM routine_runs WHERE run_id = ?", (run_id,)).fetchone()
            s = Server(data)
            procs.append(s)
            alex = alex.on(s)
            print("stop %d: exit %r; the run's row after the exit: %s; routine: consecutive_failures, "
                  "paused_reason = %r; runs listed: %d" % (i, code, pending, row(data, rid), len(runs(alex, rid))))
        def done():
            r = run_of(alex, rid, run_id)
            return r if r.get("status") not in ("pending", "running", "waiting") else None
        r = wait_for(done, 60, 0.2)
        print("left alone after the fifth start: the run is %s (reason %r); runs listed: %d; routine: %r"
              % (r and r.get("status"), r and r.get("reason"), len(runs(alex, rid)), row(data, rid)))
        chats.append(r["session_id"])
        for n, sid in enumerate(chats, 1):
            c, v = alex.call("GET", "/api/sessions/%s/events?limit=5000" % sid)
            evs = v.get("events", []) if c == 200 else []
            print("  chat %d of the run: turns started %d, ends %s"
                  % (n, sum(1 for e in evs if e["kind"] == "turn_started"), ends(evs)))
        print("  distinct chats: %d" % len(set(chats)))
        s.signal(signal.SIGTERM)
        s.exited(100)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))


def webhook():
    root = scratch("webhook")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws, delay_ms=400)   # the demo provider's 20 words take 8 s
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        c, r = alex.call("POST", "/api/routines", {
            "name": "inbox", "triggers": [{"kind": "webhook", "config": {}}],
            "spec": {"profile": "chat", "agent_id": "general",
                     "prompt": "slow: twenty words. Handle this ticket: {{event.text}}"}})
        assert c == 201, (c, r)
        rid = r["routine"]["routine_id"]
        tid = r["routine"]["triggers"][0]["trigger_id"]
        secret = r["secrets"][tid]
        req = urllib.request.Request("http://%s/api/hooks/%s" % (s.addr, tid),
                                     data=b"TICKET-4711 refund the customer", method="POST")
        req.add_header("Authorization", "Bearer " + secret)
        req.add_header("Content-Type", "text/plain")
        run_id = json.loads(urllib.request.urlopen(req).read())["run_id"]
        r = wait_for(lambda: (lambda x: x if x.get("status") == "running" else None)(run_of(alex, rid, run_id)), 30)
        assert r, run_of(alex, rid, run_id)
        first = r["session_id"]
        print("before the stop: status %s, untrusted %s, prompt %r" % (r["status"], r.get("untrusted"), r.get("prompt")))
        time.sleep(1.0)
        s.signal(signal.SIGTERM)
        print("exit status %r" % s.exited(100))
        s = Server(data)
        procs.append(s)
        alex = alex.on(s)
        def done():
            r = run_of(alex, rid, run_id)
            return r if r.get("status") not in ("pending", "running", "waiting") else None
        r = wait_for(done, 90, 0.3)
        r = r or run_of(alex, rid, run_id)
        print("after the next start: status %s, reason %r, untrusted %s, prompt %r"
              % (r.get("status"), r.get("reason"), r.get("untrusted"), r.get("prompt")))
        print("the stored event of the run is still on its row: %r" % (str(r.get("event"))[:80],))
        if r.get("session_id") and r["session_id"] != first:
            c, v = alex.call("GET", "/api/sessions/%s/events?limit=5000" % r["session_id"])
            um = [e["payload"].get("text") for e in v.get("events", []) if e["kind"] == "user_message"]
            print("what the new chat's turn was asked: %r; ends %s" % (um, ends(v.get("events", []))))
        print("runs listed: %d" % len(runs(alex, rid)))
        s.signal(signal.SIGTERM)
        s.exited(100)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))


KILLS = "kills" in sys.argv


def launch():
    """A clean stop in the moment a run's row first says `running` (its
    chat is made, its prompt is on the way): what the next start makes of
    the run."""
    tally = {}
    for i in range(8):
        root = scratch("launch%d" % i)
        data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
        os.makedirs(ws, exist_ok=True)
        write_config(data, ws)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            alex = Browser.claim(s, "alex")
            c, r = alex.call("POST", "/api/routines", {
                "name": "l", "triggers": [{"kind": "webhook", "config": {}}],
                "spec": {"profile": "chat", "agent_id": "general", "prompt": "slow: 3000"}})
            rid = r["routine"]["routine_id"]
            db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
            run_id = alex.call("POST", "/api/routines/%s/run" % rid, {})[1]["run_id"]
            by = time.monotonic() + 20
            while time.monotonic() < by:
                st = db.execute("SELECT status FROM routine_runs WHERE run_id = ?", (run_id,)).fetchone()
                if st and st[0] == "running":
                    break
            s.signal(signal.SIGTERM)
            code = s.exited(100)
            st = db.execute("SELECT status, reason, session_id IS NOT NULL FROM routine_runs WHERE run_id = ?",
                            (run_id,)).fetchone()
            db.close()
            rest = "%s%s%s" % (st[0], "+chat" if st[2] else "", " (put back)" if st[1] and "stopped while" in st[1] else "")
            s = Server(data)
            procs.append(s)
            alex = alex.on(s)
            def done():
                r = run_of(alex, rid, run_id)
                return r if r.get("status") not in ("pending", "running", "waiting") else None
            r = wait_for(done, 30, 0.2) or run_of(alex, rid, run_id)
            time.sleep(0.3)
            key = "exit %r; at rest: %s; after the next start: %s (%s); failures %r" % (
                code, rest, r.get("status"), r.get("reason") or "", row(data, rid)[0])
            tally[key] = tally.get(key, 0) + 1
            s.signal(signal.SIGTERM)
            s.exited(100)
        finally:
            cleanup(procs, needle=root)
    print("SIGTERM in the moment the run's row first says `running`, 8 tries:")
    for k, n in sorted(tally.items()):
        print("  %d: %s" % (n, k))


def window():
    """A put-back run, the next start, and a second clean stop d ms after
    that start listens: what the run's row holds at rest, and after one
    more start."""
    print("a put-back run; the next start is stopped (SIGTERM, exit 0) d ms after it listens:")
    for d in (0, 10, 20, 30, 40, 60, 80, 100, 150, 250, 400):
        root = scratch("window%d" % d)
        data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
        os.makedirs(ws, exist_ok=True)
        write_config(data, ws)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            alex = Browser.claim(s, "alex")
            c, r = alex.call("POST", "/api/routines", {
                "name": "w", "triggers": [{"kind": "webhook", "config": {}}],
                "spec": {"profile": "chat", "agent_id": "general", "prompt": "slow: 3000"}})
            rid = r["routine"]["routine_id"]
            run_id = alex.call("POST", "/api/routines/%s/run" % rid, {})[1]["run_id"]
            assert wait_for(lambda: run_of(alex, rid, run_id).get("status") == "running", 30)
            time.sleep(0.5)
            s.signal(signal.SIGTERM)
            s.exited(100)
            def at_rest():
                db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
                st = db.execute("SELECT status, reason, session_id IS NOT NULL FROM routine_runs WHERE run_id = ?",
                                (run_id,)).fetchone()
                db.close()
                return "%s%s (%s)" % (st[0], "+chat" if st[2] else "", (st[1] or "")[:44])
            first = at_rest()
            s = Server(data)
            procs.append(s)
            time.sleep(d / 1000.0)
            s.signal(signal.SIGTERM)
            code = s.exited(100)
            second = at_rest()
            s = Server(data)
            procs.append(s)
            alex = alex.on(s)
            def done():
                r = run_of(alex, rid, run_id)
                return r if r.get("status") not in ("pending", "running", "waiting") else None
            r = wait_for(done, 30, 0.2) or run_of(alex, rid, run_id)
            print("  d = %3d ms: exit %r; at rest: %s; after one more start: %s (%s); failures %r"
                  % (d, code, second, r.get("status"), (r.get("reason") or "")[:44], row(data, rid)[0]))
            s.signal(signal.SIGTERM)
            s.exited(100)
        finally:
            cleanup(procs, needle=root)


def storm():
    import random
    root = scratch("storm")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        c, r = alex.call("POST", "/api/routines", {
            "name": "storm", "triggers": [{"kind": "webhook", "config": {}}],
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "slow: 4000"}})
        assert c == 201, (c, r)
        rid = r["routine"]["routine_id"]
        c, v = alex.call("POST", "/api/routines/%s/run" % rid, {})
        run_id = v["run_id"]
        assert wait_for(lambda: run_of(alex, rid, run_id).get("status") == "running", 30)
        s.signal(signal.SIGTERM)
        s.exited(100)
        how, trail, said = [], [], []
        for i in range(12):
            s = Server(data)
            procs.append(s)
            time.sleep(random.uniform(0.0, 0.5))
            if KILLS and i % 4 == 3:
                s.kill()     # kill -9 of this script's own server
                how.append("kill")
            else:
                s.signal(signal.SIGTERM)
                how.append("stop:%r" % s.exited(100))
            db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
            st = db.execute("SELECT status, reason, session_id IS NOT NULL FROM routine_runs WHERE run_id = ?",
                            (run_id,)).fetchone()
            db.close()
            trail.append("%s%s" % (st[0], "+chat" if st[2] else ""))
            if st[0] not in ("pending", "running") and not said:
                said.append(1)
                print("after start %d (%s) the run's row is: %s" % (i + 1, how[-1], st[:2]))
        s = Server(data)
        procs.append(s)
        alex = alex.on(s)
        def done():
            r = run_of(alex, rid, run_id)
            return r if r.get("status") not in ("pending", "running", "waiting") else None
        r = wait_for(done, 60, 0.2) or run_of(alex, rid, run_id)
        print("twelve starts, each ended 0 to 0.5 s after it listened (%s)" % " ".join(how))
        print("the run's row after each: %s" % " ".join(trail))
        print("then left alone: the run is %s (reason %r); runs listed: %d; routine: %r"
              % (r.get("status"), r.get("reason"), len(runs(alex, rid)), row(data, rid)))
        s.signal(signal.SIGTERM)
        s.exited(100)
        db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "chat.db"), uri=True)
        tally = {}
        for (sid,) in db.execute("SELECT DISTINCT session_id FROM events"):
            started = db.execute("SELECT count(*) FROM events WHERE session_id = ? AND kind = 'turn_started'", (sid,)).fetchone()[0]
            e = [json.loads(p)["reason"] for (p,) in db.execute(
                "SELECT payload FROM events WHERE session_id = ? AND kind = 'turn_ended' ORDER BY seq", (sid,))]
            key = "turns started %d, ends %s" % (started, e)
            tally[key] = tally.get(key, 0) + 1
        db.close()
        print("the chats the run left in the chat store:")
        for k, n in sorted(tally.items()):
            print("  %d chat(s): %s" % (n, k))
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))


which = [a for a in sys.argv[1:] if a != "kills"] or ["stops", "webhook", "storm"]
if "stops" in which:
    stops()
if "webhook" in which:
    webhook()
if "storm" in which:
    storm()
if "window" in which:
    window()
if "launch" in which:
    launch()
