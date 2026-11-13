#!/usr/bin/env python3
"""A.19 review: a restore racing the stop of `node run`.

`NodeRuntime::handle` refuses a restore once the stop flag is set, and
`NodeRuntime::stop` waits for a restore that has registered its folder
(`restoring`). Between the two a restore that passed the first check has
not registered yet (its audit row, the log read, the checkpoint repository
come first), and `stop` looks once: it finds no turn and no restore,
returns, and `node run` exits 0 about 200 ms later with the restore cut.

The restore here is slow (a sparse file in the folder makes its "before
restore" snapshot take seconds), so the three outcomes are told apart by
the node's exit time and the caller's answer:

  refused   503 "stopping"; the node exits at once
  waited    200; the node exits after the restore (seconds)
  cut       the node exits 0 at once; the caller gets no answer from it;
            the node's audit log holds the restore (it passed the refusal)
  unanswered  the node exits 0 at once without having read the command

    restore_race.py [tries] [size] [fixed lead in ms: the control, e.g. 1500]
"""
import os, random, signal, socket, subprocess, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

TRIES = int(sys.argv[1]) if len(sys.argv) > 1 else 14
SIZE = sys.argv[2] if len(sys.argv) > 2 else "8G"


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def one(i, lead):
    root = scratch("restore%d" % i)
    data, ndata, ws = os.path.join(root, "data"), os.path.join(root, "node"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    write_config(ndata, ws)
    port = free_port()
    procs = []
    try:
        s = Server(data, ["--node-listen", "127.0.0.1:%d" % port])
        procs.append(s)
        fp = s.wait_line("nodes connect to wss://").split("(certificate ")[1].rstrip(")").strip()
        env = dict(os.environ, HARNESS_JOIN_TOKEN=s.post("/api/admin/nodes/join", {})["token"])
        j = subprocess.run([EXE, "--data-dir", ndata, "node", "join", "wss://127.0.0.1:%d" % port,
                            "--fingerprint", fp], env=env, cwd=root, capture_output=True)
        assert j.returncode == 0, j.stderr
        node_id = [l.split('"')[1] for l in open(os.path.join(ndata, "remote.toml")) if l.startswith("node_id")][0]
        n = Proc(ndata, ["node", "run"])
        procs.append(n)
        def made():
            c, v = s.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto", "node_id": node_id})
            return v["session_id"] if c == 200 else None
        sid = wait_for(made, 30, 0.2)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "run: echo one > file.txt"})
        evs = s.wait_event(sid, "turn_ended")
        turn = [e for e in evs if e["kind"] == "turn_started"][0]["payload"]["turn_id"]
        subprocess.run(["truncate", "-s", SIZE, os.path.join(ws, "big")], check=True)
        with open(os.path.join(ws, "file.txt"), "w") as f:
            f.write("two\n")
        ans = {}
        def restore():
            t = time.monotonic()
            ans["a"] = s.call("POST", "/api/sessions/%s/turns/%s/restore" % (sid, turn), {"force": True},
                              timeout=90)
            ans["t"] = time.monotonic() - t
        th = threading.Thread(target=restore)
        th.start()
        time.sleep(lead)
        t0 = time.monotonic()
        n.signal(signal.SIGTERM)
        code = n.exited(100)
        took = time.monotonic() - t0
        th.join()
        try:
            with open(os.path.join(ws, "file.txt")) as f:
                content = f.read().strip()
        except FileNotFoundError:
            content = "(removed: restored)"
        # The node's own record: did the restore get past the refusal
        # (its audit row is written after that check), and what did it log?
        import sqlite3
        db = sqlite3.connect("file:%s?mode=ro" % os.path.join(ndata, "node.db"), uri=True)
        audited = db.execute("SELECT count(*) FROM audit WHERE kind = 'session.restore'").fetchone()[0]
        phases = [json.loads(r[0]).get("phase") for r in db.execute(
            "SELECT payload FROM events WHERE kind = 'checkpoint' ORDER BY seq")]
        db.close()
        c, v = ans["a"]
        kind = ("refused" if c == 503 and "stopping" in str(v) else
                "waited" if c == 200 else
                "cut" if took < 2.0 and audited else
                "unanswered" if took < 2.0 else "other")
        print("  try %2d, SIGTERM %4.1f ms after the request: node exit %r in %4.1f s; caller: %s %s (%.1f s); "
              "file.txt: %s; node's audit rows for the restore: %d; checkpoint events: %s -> %s"
              % (i, lead * 1000, code, took, c, str(v)[:50], ans["t"], content, audited, phases, kind))
        return kind
    finally:
        cleanup(procs, needle=root)
        try:
            os.unlink(os.path.join(ws, "big"))
        except OSError:
            pass


tally = {}
for i in range(TRIES):
    k = one(i, float(sys.argv[3]) / 1000 if len(sys.argv) > 3 else random.uniform(0.0, 0.060))
    tally[k] = tally.get(k, 0) + 1
print("outcomes: %s" % tally)
