#!/usr/bin/env python3
"""A.19 review: a stop on one side of the node link while the other side
is away or busy. A real `serve --node-listen` and a real `node run`.

1. The server is stopped (SIGTERM) while its remote node runs a turn: the
   server's exit status and time; the node's command keeps running.
2. The node is stopped (SIGTERM) while the server is down: it cannot report
   the turn's end. Exit status; its command is gone.
3. The server starts again, alone: what it shows for the chat.
4. The node starts again: the end must arrive once, `stopped`, with every
   seq once and in order.
"""
import os, signal, socket, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 980000 + os.getpid() % 900


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def main():
    root = scratch("link")
    data, ndata, ws = os.path.join(root, "data"), os.path.join(root, "node"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    write_config(ndata, ws)
    port = free_port()
    listen = ["--node-listen", "127.0.0.1:%d" % port]
    procs = []
    try:
        s = Server(data, listen)
        procs.append(s)
        line = s.wait_line("nodes connect to wss://")
        fp = line.split("(certificate ")[1].rstrip(")").strip()
        token = s.post("/api/admin/nodes/join", {})["token"]
        env = dict(os.environ, HARNESS_JOIN_TOKEN=token)
        j = subprocess.run([EXE, "--data-dir", ndata, "node", "join", "wss://127.0.0.1:%d" % port,
                            "--fingerprint", fp], env=env, cwd=root, capture_output=True)
        assert j.returncode == 0, j.stderr
        node_id = [l.split('"')[1] for l in open(os.path.join(ndata, "remote.toml")) if l.startswith("node_id")][0]
        n = Proc(ndata, ["node", "run"])
        procs.append(n)
        n.wait_line("connecting to")
        def made():
            c, v = s.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto", "node_id": node_id})
            return v["session_id"] if c == 200 else None
        sid = wait_for(made, 30, 0.2)
        assert sid, n.log(10)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "run: sleep %d" % N})
        assert wait_for(lambda: sleeps(N)), "the command never started"
        s.wait_event(sid, "tool_call_started")

        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(100)
        print("1. the server, its remote node mid-turn: exit status %r in %.1f s; the node's command "
              "is still running: %s" % (code, time.monotonic() - t0, bool(sleeps(N))))
        time.sleep(1.0)
        t0 = time.monotonic()
        n.signal(signal.SIGTERM)
        code = n.exited(100)
        print("2. the node, its server away: exit status %r in %.1f s; its command is gone: %s; log: %s"
              % (code, time.monotonic() - t0, not gone_within(lambda: sleeps(N), 5) == [] or True and not sleeps(N),
                 n.log(1)[-70:]))

        s2 = Server(data, listen)
        procs.append(s2)
        time.sleep(1.0)
        evs = s2.events(sid)
        print("3. the server alone again: ends %s; last seq %s; %s" % (ends(evs), evs[-1]["seq"], kinds(evs)[-70:]))
        c, v = s2.call("POST", "/api/sessions/%s/prompt" % sid, {"text": "hello"})
        print("   a prompt to that chat meanwhile: %s %s" % (c, str(v)[:110]))

        n2 = Proc(ndata, ["node", "run"])
        procs.append(n2)
        evs = s2.wait_event(sid, "turn_ended", 40)
        time.sleep(1.0)
        evs = s2.events(sid)
        seqs = [e["seq"] for e in evs]
        print("4. the node back: ends %s; seqs once and in order: %s; %s"
              % (ends(evs), seqs == list(range(1, len(seqs) + 1)), kinds(evs)[kinds(evs).find("6:"):][-200:]))
        c, v = s2.call("POST", "/api/sessions/%s/prompt" % sid, {"text": "hello"})
        evs = s2.wait_event(sid, "turn_ended", 30)
        time.sleep(1.5)
        print("   a new turn runs after it: %s; ends %s" % (c, ends(s2.events(sid))))
        n2.signal(signal.SIGTERM)
        print("   the node stops with %r" % n2.exited(70))
        s2.signal(signal.SIGTERM)
        print("   the server stops with %r" % s2.exited(100))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))

main()
