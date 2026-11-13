#!/usr/bin/env python3
"""T.8d review: what the person can do about a background command in step 1.

First review (T8D-4): on by default, and a person could neither see nor
stop one. Decided since: step 1 ships the feature off
(`background_per_session = 0`) until step 2 has the strip and the stop.

1. With the defaults: a start is refused with the reason, nothing listens.
2. Where an operator turns it on (`background_per_session = 8`), it is as
   the first review found it, and the README has to say so: an `auto`
   chat's agent starts a listener (127.0.0.1, a free port, an empty folder;
   reached only from this script), the person presses Stop
   (`POST …/cancel`): the listener stays, and only deleting the chat ends
   it.
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
    write_config(data, ws)
    with socket.socket() as k:
        k.bind(("127.0.0.1", 0))
        port = k.getsockname()[1]
    procs = []
    try:
        # 1. the defaults
        d0 = os.path.join(root, "data-default")
        write_config(d0, ws, extra_node="")
        s0 = Server(d0)
        procs.append(s0)
        sid = s0.session(ws)
        s0.post("/api/sessions/%s/prompt" % sid, script([
            bg("cd empty && exec python3 -m http.server %d --bind 127.0.0.1" % port, 3600),
            {"tool": "bash_output", "input": {}}]))
        evs = s0.wait_event(sid, "turn_ended")
        print("with the defaults: %s" % results(evs)[0][:120])
        print("    the list: %s; started %s; the listener answers: %s"
              % (results(evs)[1][:50], bg_account(evs)[0], answers(port)))
        s0.signal(signal.SIGTERM)
        s0.exited(40)
        # 2. turned on by the operator
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, script([
            bg("cd empty && exec python3 -m http.server %d --bind 127.0.0.1" % port, 3600),
            fg("sleep 300", 600)]))
        assert wait_for(lambda: answers(port), 30, 0.2), "the listener never answered"
        evs = s.wait_event(sid, "background_started")
        print("the listener answers from outside its sandbox (another process of this machine): %s" % answers(port))
        c, v = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
        evs = s.wait_event(sid, "turn_ended")
        time.sleep(3.0)
        print("Stop (cancel): %s; the turn ended %s; the listener still answers: %s" % (c, ends(evs), answers(port)))
        tried = []
        for method, path in (("POST", "/api/sessions/%s/background/stop"), ("POST", "/api/sessions/%s/background/bg-1/stop"),
                             ("DELETE", "/api/sessions/%s/background/bg-1"), ("POST", "/api/sessions/%s/stop_background")):
            c, v = s.call(method, path % sid, {})
            tried.append("%s %s -> %s" % (method, path % "<id>", c))
        print("routes a stop might have: %s" % "; ".join(tried))
        c, v = s.call("GET", "/api/sessions/%s" % sid)
        # By field name: the chat's title is its first prompt, which holds the word.
        def fields(x):
            if isinstance(x, dict):
                return set(x) | set().union(*[fields(y) for y in x.values()] or [set()])
            return set()
        print("GET /api/sessions/<id> has a field about background commands: %s (running: %s)"
              % (any("background" in f or "bg" == f for f in fields(v)) if c == 200 else c,
                 (v.get("session") or v).get("running") if c == 200 else "?"))
        time.sleep(1.0)
        print("…the listener still answers: %s" % answers(port))
        c, v = s.call("DELETE", "/api/sessions/%s" % sid)
        gone = wait_for(lambda: not answers(port), 6, 0.2)
        print("delete the chat: %s; the listener is gone: %s" % (c, gone))
        s.signal(signal.SIGTERM)
        s.exited(40)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
