#!/usr/bin/env python3
"""A.19 review, claim 4: a routine whose run is cut by a stop.

A chat routine's turn is running (the demo provider's `slow:`) when `serve`
gets SIGTERM. The turn ends `stopped`; the run is finished `interrupted`,
which counts as a failure (`runner.rs` `counted`), and five failures in a
row pause the routine (`MAX_FAILURES`). Five stops, each during a run:
prints the run's status and the routine's counter after each.
"""
import os, signal, sqlite3, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def row(data, rid):
    db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
    try:
        return db.execute("SELECT consecutive_failures, paused_reason FROM routines WHERE routine_id = ?",
                          (rid,)).fetchone()
    finally:
        db.close()


def main():
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
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "slow: 60000"}})
        assert c == 201, (c, r)
        rid = r["routine"]["routine_id"]
        for i in range(1, 7):
            c, v = alex.call("POST", "/api/routines/%s/run" % rid, {})
            if c != 202:
                print("stop %d: run now -> %s %s" % (i, c, v))
                break
            run_id = v["run_id"]
            def running():
                c, v = alex.call("GET", "/api/routines/%s/runs" % rid)
                return [r for r in v.get("runs", []) if r["run_id"] == run_id and r["status"] == "running"]
            assert wait_for(running, 30), alex.call("GET", "/api/routines/%s/runs" % rid)
            time.sleep(0.3)
            s.signal(signal.SIGTERM)
            code = s.exited(70)
            s = Server(data)
            procs.append(s)
            alex = alex.on(s)
            time.sleep(1.0)
            c, v = alex.call("GET", "/api/routines/%s/runs" % rid)
            run = [r for r in v["runs"] if r["run_id"] == run_id][0]
            print("stop %d: exit %r; the run: %s, %r; routine: consecutive_failures, paused_reason = %r"
                  % (i, code, run["status"], run.get("reason"), row(data, rid)))
        s.signal(signal.SIGTERM)
        s.exited(30)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
