#!/usr/bin/env python3
"""RN-2: a routine whose node is away, before and after a restart of the server.

L-g made `Master::create_session` answer `Rejected("node … does not offer the
sandbox mode (its hello did not name it)")` whenever the node's capabilities
are not in memory. They are in memory only from a hello made to THIS server
process, so a node that is simply away since the server started is "a node
that did not announce the mode". The routine runner retries `NodeOffline` and
blocks (and pauses the routine) on `Rejected`.

    before  the node said hello to this process and left: the run is retried.
    after   the server restarted while the node is away: the same run.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from n10a_common import *


def routine(alex, name, node_id, ws):
    spec = {"profile": "agentic", "agent_id": "coder", "prompt": "hello", "node_id": node_id, "cwd": ws}
    trig = [{"kind": "schedule", "config": {"type": "interval", "every_minutes": 600}}]
    c, v = alex.call("POST", "/api/routines", {"name": name, "spec": spec, "triggers": trig})
    assert c in (200, 201), (c, v)
    return v["routine"]["routine_id"]


def run_and_report(alex, rid, label):
    c, v = alex.call("POST", "/api/routines/%s/run" % rid, {})
    if c != 202:
        print("  %s: Run now: %s %s" % (label, c, short(v, 200)))
        return
    run = v["run_id"]
    time.sleep(3)
    c, v = alex.call("GET", "/api/routines/%s/runs/%s" % (rid, run))
    c2, d = alex.call("GET", "/api/routines/%s" % rid)
    r = d.get("routine", d) if isinstance(d, dict) else {}
    print("  %s: run status=%s attempt=%s reason=%r" % (label, v.get("status"), v.get("attempt"), v.get("reason")))
    print("  %s  the routine: paused_reason=%r" % (" " * len(label), r.get("paused_reason")))


def main():
    root = scratch("routine")
    data, ndata, ws = os.path.join(root, "data"), os.path.join(root, "node"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    config(ndata, ws)
    port = free_port()
    listen = ["--node-listen", "127.0.0.1:%d" % port]
    procs = []
    try:
        s = Server(data, listen)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        n, node_id = remote_node(s, port, ndata, root)
        procs.append(n)
        assert wait_for(lambda: connected(s, node_id), 40, 0.1), n.log(10)
        print("the remote node %s announced: %s" % (node_id, modes(alex).get(node_id)))
        a, b = routine(alex, "Before", node_id, ws), routine(alex, "After", node_id, ws)
        n.kill()
        assert wait_for(lambda: not connected(s, node_id), 20, 0.1)
        print("1. the node is away (it said hello to this server process, then ended):")
        run_and_report(alex, a, "before")
        c, v = alex.call("POST", "/api/sessions", {"cwd": ws, "node_id": node_id})
        print("  a person's new chat there: %s %s" % (c, short(v, 160)))

        s.signal(signal.SIGTERM)
        if s.exited(30) is None:
            s.kill()
        s2 = Server(data, listen)
        procs.append(s2)
        alex2 = alex.on(s2)
        print("2. the server restarted; the node is still away (no hello to this process):")
        print("  the node list: %s" % [(x["node_id"], x.get("connected")) for x in s2.call("GET", "/api/nodes")[1]["nodes"]])
        run_and_report(alex2, b, "after ")
        c, v = alex2.call("POST", "/api/sessions", {"cwd": ws, "node_id": node_id})
        print("  a person's new chat there: %s %s" % (c, short(v, 160)))
    finally:
        left = cleanup(procs, root)
        if left:
            print("  (cleanup ended %d leftover processes of this run)" % len(left))


if __name__ == "__main__":
    main()
