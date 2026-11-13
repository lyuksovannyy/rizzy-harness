#!/usr/bin/env python3
"""T.8d: what the person can do about a background command.

First review (T8D-4): on by default, and a person could neither see nor
stop one. Step 1 then shipped the feature OFF (`background_per_session =
0`) because of that. **Step 2 closes T8D-4**: the feature is ON by default
(8 a chat) and the chat's owner has the strip and its Stop — a real HTTP
route (`POST /api/sessions/{id}/background/{bg}/stop`,
`.../background/stop`). This proof now asserts the step-2 world, so it
fails if the route or the default are taken away again. The route's
authorisation, the strip's untrusted-text handling and the kill are
attacked in `step2_authz.py`, `step2_stop.py` and
`web/src/components/backgroundstrip.test.tsx`; the full record is
`docs/security-review/2026-11-T8d-step2-stop.md`.

1. With the defaults a start RUNS (the first review found it refused once
   the feature shipped off): an `auto` chat's agent starts a listener
   (127.0.0.1, a free port, an empty folder; reached only from this
   script), and the fold carries a running background command.
2. The person's stop over the route ends it: `stopped_by_user`, and the
   listener is gone — the turn's own Stop (cancel) still leaves it, by
   design (R11).
"""
import os, signal, socket, sys, time, urllib.request
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def answers(port):
    try:
        with urllib.request.urlopen("http://127.0.0.1:%d/" % port, timeout=2) as r:
            return r.status == 200
    except Exception:
        return False


def main():
    root = scratch("person")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(os.path.join(ws, "empty"), exist_ok=True)
    # The node's defaults: step 2 ships background commands ON. No [limits].
    os.makedirs(data, exist_ok=True)
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write('workspace_roots = ["%s"]\nmax_approval_mode = "auto"\n' % ws)
    with open(os.path.join(data, "config.toml"), "w") as f:
        f.write('[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\n'
                'mock_delay_ms = 1\n\n[[providers.models]]\nid = "mock"\n')
    with socket.socket() as k:
        k.bind(("127.0.0.1", 0))
        port = k.getsockname()[1]
    procs = []
    try:
        s = Server(data)
        procs.append(s)

        # 1. the defaults take a start, and the fold shows it running.
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([
            bg("cd empty && exec python3 -m http.server %d --bind 127.0.0.1" % port, 3600),
            fg("sleep 300", 600)]))
        assert wait_for(lambda: answers(port), 30, 0.2), "the listener never answered"
        s.wait_event(sid, "background_started")
        running = [p["bg_id"] for p in of(s.events(sid), "background_started")
                   if not any(e["bg_id"] == p["bg_id"] for e in of(s.events(sid), "background_ended"))]
        c, v = s.call("GET", "/api/sessions/%s" % sid)
        print("with the defaults the start runs: the listener answers: %s; the fold shows running: %s"
              % (answers(port), running))

        # 2. the turn's own Stop leaves it (R11), then the person's stop
        #    over the route ends it.
        c, v = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
        evs = s.wait_event(sid, "turn_ended")
        time.sleep(2.0)
        print("the turn's Stop (cancel): %s; the turn ended %s; the listener still answers: %s"
              % (c, ends(evs), answers(port)))
        c, v = s.call("POST", "/api/sessions/%s/background/bg-1/stop" % sid, {})
        gone = wait_for(lambda: not answers(port), 6, 0.2)
        reasons = {p["bg_id"]: p["reason"] for p in of(s.wait_event(sid, "background_ended"), "background_ended")}
        print("the owner's Stop over the route: %s %s; the listener is gone: %s; the log's end: %s"
              % (c, v, gone, reasons))
        print("T8D-4 closed by step 2: %s"
              % (c == 200 and gone and reasons.get("bg-1") == "stopped_by_user"))

        # Stop all, and an id that does not exist: the route answers.
        c_all, v_all = s.call("POST", "/api/sessions/%s/background/stop" % sid, {})
        c_bad, v_bad = s.call("POST", "/api/sessions/%s/background/bg-9/stop" % sid, {})
        print("Stop all now (nothing running): %s %s; a stop of bg-9: %s %s"
              % (c_all, v_all, c_bad, v_bad))
        s.signal(signal.SIGTERM)
        s.exited(40)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
