#!/usr/bin/env python3
"""A.19 re-check, claim 4 on a remote node: a routine's run on `node run`,
cut by the node's stop (the master stays up), then the master is stopped
too, as on a machine or a fleet that is restarted in order: node first,
server second.

The put-back relaunches the run at once (the master is not the one
stopping); the node is still stopping or gone, so the launch falls to
`retry_later`, which keeps the retry in memory and writes its own reason
over the put-back's. The master's stop then loses the retry, and the next
start finds a `pending` run without the put-back reason.

    routine_remote.py [both|node]     node: only the node is restarted
"""
import os, signal, socket, sqlite3, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

MODE = sys.argv[1] if len(sys.argv) > 1 else "both"


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def db_row(data, run_id, rid):
    db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
    try:
        return (db.execute("SELECT status, attempt, reason FROM routine_runs WHERE run_id = ?", (run_id,)).fetchone(),
                db.execute("SELECT consecutive_failures, paused_reason FROM routines WHERE routine_id = ?",
                           (rid,)).fetchone())
    finally:
        db.close()


def main():
    root = scratch("remote")
    data, ndata, ws = os.path.join(root, "data"), os.path.join(root, "node"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    write_config(ndata, ws)
    port = free_port()
    listen = ["--node-listen", "127.0.0.1:%d" % port, "--no-local-node"]
    procs = []
    try:
        s = Server(data, listen)
        procs.append(s)
        fp = s.wait_line("nodes connect to wss://").split("(certificate ")[1].rstrip(")").strip()
        env = dict(os.environ, HARNESS_JOIN_TOKEN=s.post("/api/admin/nodes/join", {})["token"])
        j = subprocess.run([EXE, "--data-dir", ndata, "node", "join", "wss://127.0.0.1:%d" % port,
                            "--fingerprint", fp], env=env, cwd=root, capture_output=True)
        assert j.returncode == 0, j.stderr
        node_id = [l.split('"')[1] for l in open(os.path.join(ndata, "remote.toml")) if l.startswith("node_id")][0]
        n = Proc(ndata, ["node", "run"])
        procs.append(n)
        alex = Browser.claim(s, "alex")
        def linked():
            c, v = alex.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "ask", "node_id": node_id})
            return c == 200
        assert wait_for(linked, 30, 0.3), "the node never linked"
        spec = {"profile": "agentic", "agent_id": "coder", "prompt": "slow: 30000",
                "node_id": node_id, "cwd": ws}
        def made():
            c, r = alex.call("POST", "/api/routines", {
                "name": "remote", "triggers": [{"kind": "webhook", "config": {}}], "spec": spec})
            return r if c == 201 else None
        r = wait_for(made, 20, 0.5)
        if not r:
            print("NOT TESTED: the routine was not accepted: %s" % (alex.call("POST", "/api/routines", {
                "name": "remote", "triggers": [{"kind": "webhook", "config": {}}], "spec": spec}),))
            return
        rid = r["routine"]["routine_id"]
        c, v = alex.call("POST", "/api/routines/%s/run" % rid, {})
        assert c == 202, (c, v)
        run_id = v["run_id"]
        def running():
            c, v = alex.call("GET", "/api/routines/%s/runs/%s" % (rid, run_id))
            return v if c == 200 and v.get("status") == "running" else None
        r = wait_for(running, 30)
        assert r, alex.call("GET", "/api/routines/%s/runs/%s" % (rid, run_id))
        time.sleep(0.5)
        print("the run is running on the remote node: %s" % (db_row(data, run_id, rid),))
        n.signal(signal.SIGTERM)
        print("the node is stopped: exit status %r" % n.exited(100))
        time.sleep(1.5)
        print("the master's row for the run now: %s" % (db_row(data, run_id, rid),))
        if MODE == "both":
            s.signal(signal.SIGTERM)
            print("the master is stopped: exit status %r" % s.exited(100))
            print("the row at rest: %s" % (db_row(data, run_id, rid),))
            s = Server(data, listen)
            procs.append(s)
            alex = alex.on(s)
        n2 = Proc(ndata, ["node", "run"])
        procs.append(n2)
        time.sleep(4.0)
        print("both are back: %s" % (db_row(data, run_id, rid),))
        if MODE != "both":
            def settled():
                row = db_row(data, run_id, rid)[0]
                return row if row[0] in ("running", "completed", "failed", "interrupted") else None
            print("the node alone was restarted; waiting for the 1 min retry: %s" % (wait_for(settled, 90, 0.5),))
            print("then: %s" % (db_row(data, run_id, rid),))
        n2.signal(signal.SIGTERM)
        n2.exited(100)
        s.signal(signal.SIGTERM)
        s.exited(100)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
