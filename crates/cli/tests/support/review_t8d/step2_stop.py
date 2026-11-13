#!/usr/bin/env python3
"""T.8d step 2 review: does the person's stop end everything, once, and
what survives it. The real binary with the node's defaults (background
commands on, 8 a chat), its own node, bubblewrap; the API token plays the
person (the route is the same; who may call it is `step2_authz.py`).

    step2_stop.py escapers | races | first_wait | stuck | all
"""
import os, signal, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from step2_common import *

BASE = 979100
ONE, ALL = "/api/sessions/%s/background/%s/stop", "/api/sessions/%s/background/stop"


def turns(s, sid):
    return len(ends(s.events(sid)))


def run(s, sid, calls, wait=True):
    n = turns(s, sid)
    s.post("/api/sessions/%s/prompt" % sid, script(calls))
    if wait:
        assert wait_for(lambda: turns(s, sid) > n, 90, 0.05), "the turn did not end: %s" % kinds(s.events(sid))


def account(s, sid, want=None, secs=10):
    if want is not None:
        wait_for(lambda: sum(len(v) for v in bg_account(s.events(sid))[1].values()) >= want, secs, 0.05)
    return bg_account(s.events(sid))


def escapers(s, ws, marks):
    n = BASE
    marks += [n + i for i in range(8)]
    cmd = ("setsid sleep %d & nohup sleep %d >/dev/null 2>&1 & ( setsid sleep %d & ) ; "
           "sh -c 'trap \"\" TERM HUP INT; exec sleep %d' & "
           "( ( setsid nohup sh -c 'trap \"\" TERM; sleep %d' >/dev/null 2>&1 & ) & ) ; "
           "python3 -c 'import os,time\nif os.fork()==0:\n os.setsid()\n if os.fork()==0:\n  os.execvp(\"sleep\",[\"sleep\",\"%d\"])\n os._exit(0)\ntime.sleep(1)' ; "
           "sleep %d" % (n, n + 1, n + 2, n + 3, n + 4, n + 5, n + 6))
    alive = lambda k=7: [p for i in range(k) for p in sleeps(n + i)]
    sid = s.session(ws)
    run(s, sid, [bg(cmd, 3600), bg("sleep %d" % (n + 7), 3600)])
    assert wait_for(lambda: len(alive()) == 7, 10), "not all of them run: %s" % describe(alive())
    before = len(alive())
    t0 = time.monotonic()
    c, v = s.call("POST", ONE % (sid, "bg-1"), {})
    left = gone_within(alive, 8)
    took = time.monotonic() - t0
    st, en = account(s, sid, 1)
    e = [p for p in of(s.events(sid), "background_ended")][0]
    print("escapers (setsid, nohup, double fork, a daemon of its own session, TERM ignored): %d processes; Stop bg-1: %s %s"
          % (before, c, v))
    print("    left after %.2f s: %d %s; the end: reason %s, confirmed %s; the other command (bg-2) still runs: %s"
          % (took, len(left), describe(left), e["reason"], e["confirmed"], bool(sleeps(n + 7))))
    c, v = s.call("POST", ONE % (sid, "bg-1"), {})
    print("    the same stop again: %s %s" % (c, v))
    c, v = s.call("POST", ALL % sid, {})
    left = gone_within(lambda: sleeps(n + 7), 8)
    print("    Stop all: %s %s; left %d; log %s" % (c, v, len(left), account(s, sid, 2)))
    # Eight at once (the default cap), each with escapers; one Stop all.
    sid = s.session(ws)
    n2 = BASE + 20
    marks += [n2 + i for i in range(40)]
    run(s, sid, [bg("setsid sleep %d & ( setsid nohup sleep %d >/dev/null 2>&1 & ) ; sh -c 'trap \"\" TERM; exec sleep %d' & sleep %d"
                    % (n2 + 4 * i, n2 + 4 * i + 1, n2 + 4 * i + 2, n2 + 4 * i + 3), 3600) for i in range(9)])
    alive8 = lambda: [p for i in range(36) for p in sleeps(n2 + i)]
    res = results(s.events(sid))
    print("nine starts with the defaults: %d run (%d processes); the ninth: %s" % (len(account(s, sid)[0]), len(alive8()), short(res[-1], 150)))
    t0 = time.monotonic()
    c, v = s.call("POST", ALL % sid, {})
    left = gone_within(alive8, 10)
    st, en = account(s, sid, 8)
    print("    Stop all: %s %s; left after %.2f s: %d; ends %s"
          % (c, v, time.monotonic() - t0, len(left), sorted((k, r) for k, rs in en.items() for r in rs)))


def races(s, ws, marks):
    # 1. Many stops of one command at once, and Stop all with them.
    n = BASE + 100
    marks += [n, n + 1]
    sid = s.session(ws)
    run(s, sid, [bg("sleep %d" % n, 3600), bg("sleep %d" % (n + 1), 3600)])
    out = []
    def one(path):
        out.append(s.call("POST", path, {}))
    ts = [threading.Thread(target=one, args=(ONE % (sid, "bg-1"),)) for _ in range(24)]
    ts += [threading.Thread(target=one, args=(ALL % sid,)) for _ in range(8)]
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    left = gone_within(lambda: sleeps(n) + sleeps(n + 1), 8)
    st, en = account(s, sid, 2)
    time.sleep(1.0)
    st, en = account(s, sid)
    codes = {}
    for c, v in out:
        k = "%s %s" % (c, "already ended" if isinstance(v, dict) and v.get("ended") else "stopping" if isinstance(v, dict) and "stopping" in v else short(v, 40))
        codes[k] = codes.get(k, 0) + 1
    print("24 stops of bg-1 and 8 Stop all at once: answers %s; processes left %d; ends %s" % (codes, len(left), en))
    # 2. A stop aimed at the command's own end, and at its timeout.
    tally, bad = {}, 0
    sid = s.session(ws)
    k = 0
    for trial in range(16):
        k += 1
        own = trial % 2 == 0
        # Ends by itself 4.0 s after its start, or at its 5 s timeout.
        cmd = "sleep 4" if own else "sleep %d" % (n + 2)
        marks.append(n + 2)
        t_end = 4.0 if own else 5.0
        nturn = turns(s, sid)
        tmo = 30 if own else 5
        s.post("/api/sessions/%s/prompt" % sid, script([bg(cmd, tmo)]))
        # The start's event comes after its first wait (3 s) and carries the
        # deadline on this machine's clock: the command began at
        # `ends_by_ms - timeout`. Aim around its end from there.
        def started():
            for p in of(s.events(sid), "background_started"):
                if p["bg_id"] == "bg-%d" % k:
                    return p
        p = wait_for(started, 20, 0.01)
        if p:
            aim = (p["ends_by_ms"] - tmo * 1000) / 1000.0 + t_end + (trial // 2) * 0.03 - 0.03
            while time.time() < aim:
                time.sleep(0.001)
        c, v = s.call("POST", ONE % (sid, "bg-%d" % k), {})
        wait_for(lambda: turns(s, sid) > nturn, 60, 0.05)
        wait_for(lambda: len(bg_account(s.events(sid))[1].get("bg-%d" % k, [])) >= 1, 10, 0.05)
        time.sleep(0.4)
        st, en = bg_account(s.events(sid))
        rs = en.get("bg-%d" % k, [])
        if "bg-%d" % k not in st:
            # It ended inside its first wait: no id was given.
            k -= 1
            key = ("no id (ended in its first wait)", c)
        else:
            key = (tuple(rs), c, "already ended" if isinstance(v, dict) and v.get("ended") else "stopping" if c == 200 else short(v, 50))
            if len(rs) != 1:
                bad += 1
        tally[key] = tally.get(key, 0) + 1
    print("16 stops aimed at the command's own exit or its timeout: %s; starts without exactly one end: %d" % (tally, bad))
    print("    left: %d" % len(gone_within(lambda: sleeps(n + 2), 8)))


def first_wait(s, ws, marks):
    """Stop all while the still-running turn starts another command."""
    n = BASE + 200
    marks += [n + i for i in range(6)]
    sid = s.session(ws)
    run(s, sid, [bg("sleep %d" % n, 3600), fg("sleep 1"), bg("sleep %d" % (n + 1), 3600), fg("sleep 4", 30),
                 bg("sleep %d" % (n + 2), 3600), fg("sleep 2", 30)], wait=False)
    assert wait_for(lambda: sleeps(n + 1), 30), "the second start never began"
    time.sleep(1.0)  # one second into its first wait (3 s)
    st0, _ = bg_account(s.events(sid))
    c, v = s.call("POST", ALL % sid, {})
    print("Stop all 1 s into the turn's second start (the log shows started %s): %s %s" % (st0, c, v))
    time.sleep(1.0)
    print("    1 s later: the first command runs: %s; the one that was starting runs: %s" % (bool(sleeps(n)), bool(sleeps(n + 1))))
    assert wait_for(lambda: turns(s, sid) >= 1, 60, 0.05)
    st, en = account(s, sid)
    print("    when the turn ended: started %s, ends %s; running: second %s, third (started after the Stop all) %s"
          % (st, en, bool(sleeps(n + 1)), bool(sleeps(n + 2))))
    notes = [p for p in of(s.events(sid), "tool_call_finished")]
    c, v = s.call("POST", ALL % sid, {})
    left = gone_within(lambda: [p for i in range(3) for p in sleeps(n + i)], 8)
    print("    a second Stop all, the turn over: %s %s; left %d; ends %s" % (c, v, len(left), account(s, sid, 3)[1]))
    # The panic order the note gives: the turn's Stop, then Stop all.
    sid = s.session(ws)
    run(s, sid, [bg("sleep %d" % (n + 3), 3600), fg("sleep 1"), bg("sleep %d" % (n + 4), 3600), fg("sleep 30", 60),
                 bg("sleep %d" % (n + 5), 3600)], wait=False)
    assert wait_for(lambda: sleeps(n + 4), 30)
    time.sleep(1.0)
    c1, v1 = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
    c2, v2 = s.call("POST", ALL % sid, {})
    assert wait_for(lambda: turns(s, sid) >= 1, 60, 0.05)
    left = gone_within(lambda: [p for i in range(3, 6) for p in sleeps(n + i)], 8)
    time.sleep(3.5)
    late = [p for i in range(3, 6) for p in sleeps(n + i)]
    print("the turn's Stop and then Stop all, 1 s into a start: cancel %s, Stop all %s %s; turn %s; left %d, 3.5 s later %d; started %s ends %s"
          % (c1, c2, v2, ends(s.events(sid)), len(left), len(late), *account(s, sid)))
    # The other order: Stop all first, then the turn's Stop.
    sid = s.session(ws)
    m = BASE + 210
    marks += [m + i for i in range(3)]
    run(s, sid, [bg("sleep %d" % m, 3600), fg("sleep 1"), bg("sleep %d" % (m + 1), 3600), fg("sleep 30", 60)], wait=False)
    assert wait_for(lambda: sleeps(m + 1), 30)
    time.sleep(1.0)
    c2, v2 = s.call("POST", ALL % sid, {})
    c1, v1 = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
    assert wait_for(lambda: turns(s, sid) >= 1, 60, 0.05)
    left = gone_within(lambda: [p for i in range(3) for p in sleeps(m + i)], 8)
    print("Stop all and then the turn's Stop, 1 s into a start: Stop all %s %s, cancel %s; turn %s; left %d; started %s ends %s"
          % (c2, v2, c1, ends(s.events(sid)), len(left), *account(s, sid, 1)))


def stuck(s, ws, marks):
    """A command in a read the kernel does not interrupt (a mount that
    stopped answering: `fuse_stall.py`, as T.8e's test holds it)."""
    mnt = os.path.join(ws, "mnt")
    try:
        st = Stall(mnt)
    except RuntimeError as e:
        print("stuck: not run (%s)" % e)
        return
    try:
        sid = s.session(ws)
        run(s, sid, [bg("exec cat mnt/stall", 3600)])
        if not st.expect("read", 15):
            print("stuck: the command's read never arrived (the mount is not in its view?): %s" % short(results(s.events(sid))[-1], 200))
            return
        cats = lambda: [p for p in pids() if cmdline(p) == b"cat\0mnt/stall\0" and alive(p)]
        print("stuck: a command in an uninterruptible read: %s" % describe(cats()))
        t0 = time.monotonic()
        c, v = s.call("POST", ONE % (sid, "bg-1"), {})
        stt, en = account(s, sid, 1, secs=30)
        took = time.monotonic() - t0
        e = (of(s.events(sid), "background_ended") or [{}])[0]
        print("    Stop: %s %s; the end after %.1f s: reason %s, confirmed %s; the process: %s"
              % (c, v, took, e.get("reason"), e.get("confirmed"), describe(cats()) or "gone"))
        c, v = s.call("POST", ONE % (sid, "bg-1"), {})
        print("    the stop again: %s %s" % (c, v))
        c, v = s.call("POST", ALL % sid, {})
        print("    Stop all: %s %s; the process: %s" % (c, v, "alive" if cats() else "gone"))
        run(s, sid, [{"tool": "bash_output", "input": {}}])
        print("    what the agent's list says: %s" % short(results(s.events(sid))[-1], 200))
    finally:
        st.release()
        left = gone_within(lambda: [p for p in pids() if cmdline(p) == b"cat\0mnt/stall\0" and alive(p)], 8)
        print("    the mount released: the process is %s; still mounted: %s" % ("alive" if left else "gone", st.mounted()))


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    root = scratch("stop")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    procs, marks = [], []
    try:
        s = Server(data)
        procs.append(s)
        for name, f in (("escapers", escapers), ("races", races), ("first_wait", first_wait), ("stuck", stuck)):
            if which in (name, "all"):
                f(s, ws, marks)
        s.signal(signal.SIGTERM)
        print("the server's exit status: %r" % s.exited(60))
    finally:
        k = cleanup(procs, needle=root, secs=sorted(set(marks)))
        print("cleanup killed: %s" % (k or "nothing"))

main()
