#!/usr/bin/env python3
"""T.8d step 2 review: the stop when the node or the master is away, slow,
killed or restarted; what the log (and so the strip) shows then, and
whether an id is ever given twice. A real `serve --node-listen` and a real
`node run` with the defaults, on this machine.

    step2_restart.py remote | embedded | all
"""
import os, signal, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from step2_common import *

BASE = 979300
ONE, ALL = "/api/sessions/%s/background/%s/stop", "/api/sessions/%s/background/stop"


def turns(s, sid):
    return len(ends(s.events(sid)))


def run(s, sid, calls, wait=True):
    n = turns(s, sid)
    s.post("/api/sessions/%s/prompt" % sid, script(calls))
    if wait:
        assert wait_for(lambda: turns(s, sid) > n, 90, 0.05), "the turn did not end: %s" % kinds(s.events(sid))


def acct(s, sid, want=None, secs=15):
    if want is not None:
        wait_for(lambda: sum(len(v) for v in bg_account(s.events(sid))[1].values()) >= want, secs, 0.05)
    return bg_account(s.events(sid))


def esc(n):
    return "setsid sleep %d & ( setsid nohup sleep %d >/dev/null 2>&1 & ) ; sleep %d" % (n, n + 1, n + 2)


def remote(root, procs, marks):
    data, ndata, ws = os.path.join(root, "data"), os.path.join(root, "node"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    config(ndata, ws)
    port = free_port()
    listen = ["--node-listen", "127.0.0.1:%d" % port]
    s = Server(data, listen)
    procs.append(s)
    n, node_id = remote_node(s, port, ndata, root)
    procs.append(n)
    assert wait_for(lambda: connected(s, node_id), 30, 0.1), n.log(10)
    m = BASE
    marks += [m + i for i in range(30)]
    al = lambda a, k=3: [p for i in range(k) for p in sleeps(a + i)]
    sid = s.session(ws, node=node_id)
    run(s, sid, [bg(esc(m), 3600)])
    assert len(al(m)) == 3

    # 1. The node does not answer (stopped with SIGSTOP: its link is up).
    n.signal(signal.SIGSTOP)
    out = []
    def one():
        t0 = time.monotonic()
        c, v = s.call("POST", ONE % (sid, "bg-1"), {}, timeout=60)
        out.append((c, short(v, 60), time.monotonic() - t0))
    rss0, fds0 = rss_kib(s.pid), len(os.listdir("/proc/%d/fd" % s.pid))
    ts = [threading.Thread(target=one) for _ in range(300)]
    t0 = time.monotonic()
    for t in ts:
        t.start()
    time.sleep(5)
    rss1, fds1 = rss_kib(s.pid), len(os.listdir("/proc/%d/fd" % s.pid))
    c, v = s.call("GET", "/api/sessions/%s" % sid, timeout=10)
    for t in ts:
        t.join()
    took = time.monotonic() - t0
    tally = {}
    for c2, v2, d in out:
        k = "%s %s after %d s" % (c2, v2, round(d))
        tally[k] = tally.get(k, 0) + 1
    print("1. the node frozen (SIGSTOP), 300 stops of bg-1 at once: %s" % tally)
    print("   all answered within %.1f s; the server meanwhile: RSS %d -> %d KiB, descriptors %d -> %d, another request of the same user: %s"
          % (took, rss0, rss1, fds0, fds1, c))
    n.signal(signal.SIGCONT)
    left = gone_within(lambda: al(m), 10)
    st, en = acct(s, sid, 1)
    print("   the node goes on (SIGCONT): the stops it had queued are carried out: left %d; ends %s; server RSS %d KiB, descriptors %d"
          % (len(left), en, rss_kib(s.pid), len(os.listdir("/proc/%d/fd" % s.pid))))

    # 2. The node is killed.
    run(s, sid, [bg(esc(m + 3), 3600)])
    assert len(al(m + 3)) == 3
    n.signal(signal.SIGKILL)
    n.exited(10)
    left = gone_within(lambda: al(m + 3), 5)
    wait_for(lambda: not connected(s, node_id), 10, 0.1)
    a = s.call("POST", ONE % (sid, "bg-2"), {})
    b = s.call("POST", ALL % sid, {})
    st, en = acct(s, sid)
    print("2. the node killed (-9): its command's processes left after it: %d" % len(left))
    print("   the log still shows bg-2 running: %s; Stop: %s %s" % ("bg-2" not in en, a[0], a[1]))
    print("   Stop all: %s %s" % (b[0], short(b[1], 90)))
    n = Proc(ndata, ["node", "run"])
    procs.append(n)
    assert wait_for(lambda: connected(s, node_id), 40, 0.1), n.log(10)
    st, en = acct(s, sid, 2)
    a = s.call("POST", ONE % (sid, "bg-2"), {})
    print("   the node is back: ends %s; Stop bg-2 now: %s %s" % (en, a[0], a[1]))
    run(s, sid, [bg(esc(m + 6), 3600)])
    st, en = acct(s, sid)
    print("   a new start after the restart is %s (ids so far %s)" % (st[-1], st))

    # 3. The master is killed and started again; the node and its command stay.
    #    A turn is running, and starts another command while the master is away.
    run(s, sid, [fg("sleep 6", 30), bg(esc(m + 9), 3600)], wait=False)
    time.sleep(1.5)
    st_before = acct(s, sid)[0]
    s.signal(signal.SIGKILL)
    s.exited(10)
    assert wait_for(lambda: len(al(m + 9)) == 3, 30), "the turn did not go on without the master"
    time.sleep(4.0)
    print("3. the master killed (-9) mid-turn: the node's commands run on: bg-3 %s, and one started while the master was away: %s"
          % (len(al(m + 6)) == 3, len(al(m + 9)) == 3))
    s = Server(data, listen)
    procs.append(s)
    assert wait_for(lambda: connected(s, node_id), 60, 0.1), n.log(10)
    wait_for(lambda: len(acct(s, sid)[0]) > len(st_before), 20, 0.1)
    st, en = acct(s, sid)
    print("   the master is back: the log had %s before, now started %s, ends %s" % (st_before, st, en))
    a = s.call("POST", ALL % sid, {})
    left = gone_within(lambda: al(m + 6) + al(m + 9), 10)
    st, en = acct(s, sid, 4)
    print("   Stop all: %s %s; left %d; ends %s" % (a[0], a[1], len(left), en))

    # 4. The node is stopped properly (SIGTERM) with one running.
    run(s, sid, [bg(esc(m + 12), 3600)])
    n.signal(signal.SIGTERM)
    code = n.exited(60)
    left = gone_within(lambda: al(m + 12), 5)
    time.sleep(1.0)
    a = s.call("POST", ONE % (sid, "bg-5"), {})
    st, en = acct(s, sid)
    print("4. the node stopped (SIGTERM, exit %r): left %d; ends %s; Stop bg-5 now: %s %s" % (code, len(left), en.get("bg-5"), a[0], short(a[1], 120)))
    s.signal(signal.SIGTERM)
    print("   the server's exit status: %r" % s.exited(60))


def embedded(root, procs, marks):
    data, ws = os.path.join(root, "edata"), os.path.join(root, "ews")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    m = BASE + 50
    marks += [m + i for i in range(12)]
    al = lambda a, k=3: [p for i in range(k) for p in sleeps(a + i)]
    s = Server(data)
    procs.append(s)
    sid = s.session(ws)
    run(s, sid, [bg(esc(m), 3600), bg("sleep 5", 30)])
    time.sleep(4.0)
    st, en = acct(s, sid, 1)
    print("5. the server with its own node: started %s, ends %s" % (st, en))
    s.signal(signal.SIGKILL)
    s.exited(10)
    left = gone_within(lambda: al(m), 5)
    s = Server(data)
    procs.append(s)
    st, en = acct(s, sid, 2)
    a = s.call("POST", ONE % (sid, "bg-1"), {})
    print("   killed (-9): processes left %d; started again: ends %s; Stop bg-1: %s %s" % (len(left), en, a[0], a[1]))
    run(s, sid, [bg(esc(m + 3), 3600)])
    st, en = acct(s, sid)
    a = s.call("POST", ONE % (sid, st[-1]), {})
    left = gone_within(lambda: al(m + 3), 8)
    print("   a new start is %s (never an id again: %s); its Stop: %s %s; left %d"
          % (st[-1], len(set(st)) == len(st), a[0], a[1], len(left)))
    s.signal(signal.SIGTERM)
    print("   the server's exit status: %r" % s.exited(60))


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    root = scratch("restart")
    procs, marks = [], []
    try:
        if which in ("remote", "all"):
            remote(root, procs, marks)
        if which in ("embedded", "all"):
            embedded(root, procs, marks)
    finally:
        for p in procs:
            try:
                os.kill(p.pid, signal.SIGCONT)
            except OSError:
                pass
        k = cleanup(procs, needle=root, secs=sorted(set(marks)))
        print("cleanup killed: %s" % (k or "nothing"))

main()
