#!/usr/bin/env python3
"""A.19 re-check, N-7: a stop while a sub-agent is being started.

`NodeRuntime::stop` marks every running turn as the stop's, then cancels
them. A sub-agent that passed `run_child`'s refusal before the flag was set
and has not claimed its turn yet (its folder is being made: a git worktree)
is not running, so it is not marked; its parent is cancelled, and the
child's token is the parent's. When the child then claims its turn the token
is already cancelled, and `mark_stopped` leaves it alone.

SIGTERM d ms after the parent's `delegate` call starts; then, from node.db,
how each session's turn ended.
"""
import os, signal, sqlite3, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def ends_by_title(data):
    db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "node.db"), uri=True)
    out = []
    for (sid,) in db.execute("SELECT DISTINCT session_id FROM events"):
        title = json.loads(db.execute("SELECT payload FROM events WHERE session_id = ? AND seq = 1", (sid,)).fetchone()[0]).get("title", "")
        e = [json.loads(p)["reason"] for (p,) in db.execute(
            "SELECT payload FROM events WHERE session_id = ? AND kind = 'turn_ended' ORDER BY seq", (sid,))]
        started = db.execute("SELECT count(*) FROM events WHERE session_id = ? AND kind = 'turn_started'", (sid,)).fetchone()[0]
        out.append(("child" if title.startswith("@") else "parent", started, e))
    db.close()
    return sorted(out)


def main():
    tally = {}
    for i, d in enumerate([0, 0, 5, 5, 10, 10, 20, 20, 40, 40, 80, 80, 150, 150, 300, 300]):
        root = scratch("child%d" % i)
        data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
        os.makedirs(ws, exist_ok=True)
        write_config(data, ws)
        for a in (["init", "-q"], ["commit", "-q", "--allow-empty", "-m", "first"]):
            subprocess.run(["git", "-c", "user.name=review", "-c", "user.email=review@example.invalid"] + a,
                           cwd=ws, check=True)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            c, v = s.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto", "agent_id": "teamlead"})
            assert c == 200, (c, v)
            sid = v["session_id"]
            db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "node.db"), uri=True)
            s.post("/api/sessions/%s/prompt" % sid, {"text": "delegate: coder=slow: 60000"})
            by = time.monotonic() + 20
            while time.monotonic() < by:
                if db.execute("SELECT 1 FROM events WHERE session_id = ? AND kind = 'tool_call_started'", (sid,)).fetchone():
                    break
            db.close()
            time.sleep(d / 1000.0)
            s.signal(signal.SIGTERM)
            code = s.exited(100)
            got = ends_by_title(data)
            key = "; ".join("%s: started %d, ends %s" % g for g in got)
            tally.setdefault(key, []).append("%d ms (exit %r)" % (d, code))
        finally:
            cleanup(procs, needle=root)
    print("SIGTERM d ms after the parent's `delegate` call started:")
    for k, ds in sorted(tally.items()):
        print("  %s\n      at %s" % (k, ", ".join(ds)))

main()
