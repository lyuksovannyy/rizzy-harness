#!/usr/bin/env python3
"""A.19 review: what a stop does to a running command that resists it, on
the real `serve` with the real bubblewrap, and what is left after exit.

  term      the command ignores SIGTERM
  detach    the command started a child in its own session and group
  many      three sessions, each with a command that ignores SIGTERM
  stuck     the command waits in a read the kernel cannot interrupt once it
            is killed (`../fuse_stall.py`)
  off       `[sandbox] contain = "off"` and a child in its own session

For each: seconds from SIGTERM to exit, the exit status, the processes of
the turn alive after the exit, and what the next start finds in the log.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 975000 + os.getpid() % 900


def case(name, script, secs, sessions=1, node_extra="", stall=False, hold=0.0):
    root = scratch(name)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws, node_extra)
    st = Stall(os.path.join(ws, "mnt")) if stall else None
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sids = [s.session(ws) for _ in range(sessions)]
        for i, sid in enumerate(sids):
            s.post("/api/sessions/%s/prompt" % sid, {"text": "run: " + script % {"n": secs + i}})
        if stall:
            assert st.expect("read"), "no read reached the mount"
        else:
            assert wait_for(lambda: all(sleeps(secs + i) for i in range(sessions))), \
                "the command never started:\n" + s.log(20)
        time.sleep(hold)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(75)
        took = time.monotonic() - t0
        left = holding(root) + [p for i in range(sessions) for p in sleeps(secs + i)]
        left = [p for p in left if p != (st.p.pid if st else -1)]
        print("[%s] exit status %r %.1f s after SIGTERM; last log line: %s"
              % (name, code, took, s.log(1)[-150:]))
        print("[%s] alive right after the exit: %s" % (name, describe(left) or "nothing"))
        left2 = gone_within(lambda: [p for p in left if alive(p)], 5)
        print("[%s] alive 5 s later: %s" % (name, describe(left2) or "nothing"))
        if st:
            st.release()
            left3 = gone_within(lambda: [p for p in left if alive(p)], 5)
            print("[%s] alive after the mount was released: %s" % (name, describe(left3) or "nothing"))
        s2 = Server(data)
        procs.append(s2)
        for sid in sids:
            evs = s2.events(sid)
            fin = [e["payload"].get("output", "")[-170:].replace("\n", " | ")
                   for e in evs if e["kind"] == "tool_call_finished"]
            print("[%s] next start: ends %s; tool result: %s" % (name, ends(evs), fin))
        for l in s2.log().split("\n"):
            if "sweep" in l or "ended a command" in l or "left" in l:
                print("[%s] next start's log: %s" % (name, l[-200:]))
        s2.signal(signal.SIGTERM)
        s2.exited(30)
    finally:
        if st:
            st.release()
        k = cleanup(procs, needle=root, secs=[secs + i for i in range(sessions)])
        print("[%s] cleanup killed: %s%s" % (name, k or "nothing",
                                             "; mount left: %s" % st.mounted() if st else ""))


which = sys.argv[1:] or ["term", "detach", "many", "stuck", "off"]
if "term" in which:
    case("term", "trap '' TERM; sleep %(n)d", N)
if "detach" in which:
    case("detach", "setsid sleep %(n)d & sleep 1; echo up; wait", N + 10, hold=1.5)
if "many" in which:
    case("many", "trap '' TERM; sleep %(n)d", N + 20, sessions=3)
if "stuck" in which:
    case("stuck", "cat mnt/stall; sleep %(n)d", N + 30, stall=True)
if "off" in which:
    case("off", "setsid sleep %(n)d & sleep 1; echo up; wait", N + 40,
         node_extra='\n[sandbox]\ncontain = "off"\n', hold=1.5)
