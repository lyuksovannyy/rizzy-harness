#!/usr/bin/env python3
"""A.19 review: signals at awkward moments, on the real `serve`.

  second   two turns: one closes at once, one's command ignores SIGTERM;
           SIGTERM, then SIGINT 0.5 s later. Exit status, what is alive
           after, and what the next start makes of each log.
  twice    SIGTERM twice back to back (the same microsecond), ten times.
  hup,quit SIGHUP / SIGQUIT with a turn running.
  startup  SIGTERM a given time after the process was started, on a data
           directory that holds a turn left open by a kill -9.

    signals.py [second] [twice] [hup] [quit] [startup]
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 978000 + os.getpid() % 900


def fresh(name):
    root = scratch(name)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    return root, data, ws


def busy(s, ws, script, n):
    sid = s.session(ws)
    s.post("/api/sessions/%s/prompt" % sid, {"text": "run: " + script % n})
    assert wait_for(lambda: sleeps(n)), "the command never started"
    return sid


def after(data, sids, procs, tag):
    s2 = Server(data)
    procs.append(s2)
    for name, sid in sids:
        evs = s2.events(sid)
        print("[%s] next start, %-6s: ends %s; last: %s" % (tag, name, ends(evs), kinds(evs)[-110:]))
    s2.signal(signal.SIGTERM)
    c = s2.exited(30)
    s3 = Server(data)
    procs.append(s3)
    for name, sid in sids:
        print("[%s] third start, %-6s: ends %s" % (tag, name, ends(s3.events(sid))))
    s3.signal(signal.SIGTERM)
    s3.exited(30)


def second():
    root, data, ws = fresh("second")
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        quick = busy(s, ws, "sleep %d", N)
        slow = busy(s, ws, "trap '' TERM; sleep %d", N + 1)
        s.signal(signal.SIGTERM)
        time.sleep(0.5)
        t0 = time.monotonic()
        s.signal(signal.SIGINT)
        code = s.exited(10)
        took = time.monotonic() - t0
        left = holding(root) + sleeps(N) + sleeps(N + 1)
        print("[second] SIGTERM, then SIGINT 0.5 s later: exit status %r %.3f s after the second; "
              "alive right after: %d process(es)" % (code, took, len(left)))
        left = gone_within(lambda: holding(root) + sleeps(N) + sleeps(N + 1), 5)
        print("[second] alive 5 s later: %s" % (describe(left) or "nothing"))
        print("[second] log: %s" % " / ".join(l.split("rizzy_harness: ")[-1][:95] for l in s.log(3).split("\n")))
        after(data, (("quick", quick), ("slow", slow)), procs, "second")
    finally:
        print("[second] cleanup killed: %s" % (cleanup(procs, root, [N, N + 1]) or "nothing"))


def twice():
    out = []
    for i in range(10):
        root, data, ws = fresh("twice%d" % i)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            busy(s, ws, "trap '' TERM; sleep %d", N + 10 + i)
            t0 = time.monotonic()
            os.kill(s.pid, signal.SIGTERM)
            os.kill(s.pid, signal.SIGTERM)
            code = s.exited(20)
            out.append("%r in %.2f s" % (code, time.monotonic() - t0))
        finally:
            cleanup(procs, root, [N + 10 + i])
    print("[twice] SIGTERM twice back to back, exit status and time, ten runs: %s" % ", ".join(out))


def other(name, sig):
    root, data, ws = fresh(name)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = busy(s, ws, "trap '' TERM HUP; sleep %d", N + 30)
        s.signal(sig)
        code = s.exited(10)
        left = holding(root) + sleeps(N + 30)
        print("[%s] exit status %r (negative: died of the signal); alive right after: %d; log: %s"
              % (name, code, len(left), s.log(1)[-80:]))
        left = gone_within(lambda: holding(root) + sleeps(N + 30), 6)
        print("[%s] alive 6 s later: %s" % (name, describe(left) or "nothing"))
        after(data, (("turn", sid),), procs, name)
    finally:
        print("[%s] cleanup killed: %s" % (name, cleanup(procs, root, [N + 30]) or "nothing"))


def startup():
    root, data, ws = fresh("startup")
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = busy(s, ws, "sleep %d", N + 40)
        s.kill()  # kill -9 of this script's own server: the turn stays open
        gone_within(lambda: holding(root) + sleeps(N + 40), 6)
        rows = []
        for ms in (0, 2, 5, 10, 20, 40, 80, 120, 160, 200, 250, 300, 400, 500, 700, 1000):
            p = Proc(data, ["serve", "--bind", "127.0.0.1:0"])
            procs.append(p)
            time.sleep(ms / 1000.0)
            t0 = time.monotonic()
            p.signal(signal.SIGTERM)
            code = p.exited(70)
            took = time.monotonic() - t0
            listening = any("listening on" in l for _, l in p.out)
            said = "stop" if "stopping the server" in p.log() else "-"
            rows.append("%d ms: %r in %.2f s%s%s" % (ms, code, took, " listening" if listening else "",
                                                     " " + said if said != "-" else ""))
            if code is None:
                print("[startup] at %d ms the process did not exit in 70 s:\n%s" % (ms, p.log(8)))
                p.kill()
        print("[startup] SIGTERM n ms after exec -> exit status (negative: died of the signal):")
        for r in rows:
            print("    " + r)
        s2 = Server(data)
        procs.append(s2)
        evs = s2.events(sid)
        print("[startup] the full start after them: ends %s; %s" % (ends(evs), kinds(evs)[kinds(evs).find("6:"):]))
        s2.signal(signal.SIGTERM)
        print("[startup] and stops with %r" % s2.exited(30))
    finally:
        print("[startup] cleanup killed: %s" % (cleanup(procs, root, [N + 40]) or "nothing"))


which = sys.argv[1:] or ["second", "twice", "hup", "quit", "startup"]
if "second" in which:
    second()
if "twice" in which:
    twice()
if "hup" in which:
    other("hup", signal.SIGHUP)
if "quit" in which:
    other("quit", signal.SIGQUIT)
if "startup" in which:
    startup()
