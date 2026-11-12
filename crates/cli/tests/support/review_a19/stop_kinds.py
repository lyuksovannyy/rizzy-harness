#!/usr/bin/env python3
"""A.19 review: one stop with a turn of each kind in flight, on `serve`:
a model call (the demo provider streaming slowly), a sub-agent's delegated
turn, a web tool waiting on a server that never answers (this script's own
socket), a turn waiting for an approval, a chat-profile turn (the server's
chat runtime), and a shell command.

After the exit, read from the stores themselves (node.db, chat.db): for
every session, its turns started and how each ended, and who denied each
approval. Then the same after the next start (nothing may be added).
"""
import os, signal, socket, sqlite3, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 982000 + os.getpid() % 900


def turns(data):
    out = {}
    for name in ("node.db", "chat.db"):
        db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, name), uri=True)
        for sid, kind, payload, seq in db.execute(
                "SELECT session_id, kind, payload, seq FROM events WHERE kind IN "
                "('turn_started','turn_ended','approval_resolved','session_created') ORDER BY session_id, seq"):
            p = json.loads(payload)
            s = out.setdefault((name, sid), {"title": "", "started": 0, "ends": [], "approvals": [], "seqs": []})
            if kind == "session_created":
                s["title"] = (p.get("title") or "")[:28]
            elif kind == "turn_started":
                s["started"] += 1
            elif kind == "turn_ended":
                s["ends"].append(p.get("reason"))
            else:
                s["approvals"].append("%s by %s" % (p.get("decision"), p.get("by")))
        for sid, n, top in db.execute("SELECT session_id, count(*), max(seq) FROM events GROUP BY session_id"):
            out[(name, sid)]["contiguous"] = n == top
        db.close()
    return out


def show(t, label):
    print(label)
    for (db, sid), s in sorted(t.items(), key=lambda x: x[1]["title"]):
        print("  %-8s %-30s started %d, ends %s%s, seq contiguous %s"
              % (db, s["title"] or sid[:8], s["started"], s["ends"],
                 ", approvals %s" % s["approvals"] if s["approvals"] else "", s.get("contiguous")))


def main():
    root = scratch("kinds")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    # A web server that accepts and never answers.
    silent = socket.socket()
    silent.bind(("127.0.0.1", 0))
    silent.listen(8)
    held = []
    threading.Thread(target=lambda: [held.append(silent.accept()) for _ in range(8)], daemon=True).start()
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        def start(title, text, body=None, mode="auto"):
            b = {"cwd": ws, "approval_mode": mode, "title": title}
            b.update(body or {})
            c, v = s.call("POST", "/api/sessions", b)
            if c != 200:
                print("  NOT TESTED %s: session: %s %s" % (title, c, v))
                return None
            c2, v2 = s.call("POST", "/api/sessions/%s/prompt" % v["session_id"], {"text": text})
            if c2 != 200:
                print("  NOT TESTED %s: prompt: %s %s" % (title, c2, v2))
            return v["session_id"]
        start("1 model call", "slow: 120000")
        start("2 sub-agent (parent)", "delegate: scout=slow: 120000", {"agent_id": "teamlead"})
        start("3 web tool", "fetch: http://127.0.0.1:%d/never" % silent.getsockname()[1])
        start("4 approval", "run: touch must-not-exist", mode="ask")
        start("5 chat runtime", "slow: 120000", {"profile": "chat", "cwd": ""})
        start("6 shell", "run: trap '' TERM; sleep %d" % N)
        assert wait_for(lambda: sleeps(N)), "the command never started"
        time.sleep(2.0)
        show(turns(data), "before the stop:")
        print("  the silent web server holds %d connection(s)" % len(held))
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(80)
        print("exit status %r, %.1f s after SIGTERM; %s" % (code, time.monotonic() - t0, s.log(1)[-45:]))
        after = turns(data)
        show(after, "in the stores after the exit:")
        s2 = Server(data)
        procs.append(s2)
        time.sleep(1.0)
        s2.signal(signal.SIGTERM)
        s2.exited(30)
        again = turns(data)
        print("the next start added nothing to any log: %s" % (again == after))
        if again != after:
            show(again, "after the next start:")
        print("must-not-exist exists: %s" % os.path.exists(os.path.join(ws, "must-not-exist")))
    finally:
        k = cleanup(procs, needle=root, secs=[N])
        print("cleanup killed: %s" % (k or "nothing"))

main()
