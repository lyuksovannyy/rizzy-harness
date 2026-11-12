#!/usr/bin/env python3
"""A.19 review, A19-1: after "stopped: every turn is closed" the process
does not exit while one of the node's own blocking threads is stuck, it has
already released the data directory's lock, and it no longer reacts to
SIGTERM or SIGINT.

The node's `read` tool reads a file on a mount that does not answer (the
guard tests' FUSE server, `../fuse_stall.py`: a network mount whose server
went away). The stop cancels the turn; the agent loop abandons the tool
after its 5 s grace and the turn ends `stopped`. Then `async_main` returns
(the lock file is closed: the lock is released) and the tokio runtime is
dropped, which waits for the stuck blocking thread for ever.

    stop_hang_blocking.py [serve|node]
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

def main():
    root = scratch("hang")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    stall = Stall(os.path.join(ws, "mnt"))
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "read: mnt/stall"})
        assert stall.expect("read"), "the read never reached the mount"
        print("the node's read tool is waiting in a read of %s/stall" % stall.mnt)
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        line = s.wait_line("stopped: every turn is closed", secs=70, where="err")
        t_closed = time.monotonic() - t0
        print("%.1f s after SIGTERM the log says: %s" % (t_closed, line.split(" INFO ")[-1].strip()))
        st = s.exited(15)
        print("exit status 15 s later: %r (None: still running), process state %r"
              % (st, state(s.pid)))
        print("the data directory's lock can be taken by someone else now: %s" % lock_free(data))
        c, v = s.call("GET", "/api/sessions/%s/events" % sid, timeout=3)
        print("its HTTP port answers: %s" % ("yes (%s)" % c if c else "no (%s)" % v[:60]))
        for name, sig in (("SIGTERM", signal.SIGTERM), ("SIGINT", signal.SIGINT)):
            s.signal(sig)
            print("after one more %s: exit status %r" % (name, s.exited(3)))
        # A second start on the same data directory, beside the first.
        s2 = None
        try:
            s2 = Server(data)
            procs.append(s2)
            evs = s2.events(sid)
            print("a second `serve` on the same data directory started (pid %d) while pid %d "
                  "is alive [%s]; the session's log: %s" % (s2.pid, s.pid, state(s.pid), kinds(evs)[-160:]))
            s2.signal(signal.SIGTERM)
            print("the second one stops with exit status %r" % s2.exited(30))
        except RuntimeError as e:
            print("a second `serve` was refused: %s" % str(e)[-300:])
        stall.release()
        print("the mount is released: the first process now ends with exit status %r"
              % s.exited(15))
    finally:
        stall.release()
        left = cleanup(procs, needle=root)
        print("cleanup: killed %s; mount left: %s" % (left or "nothing", stall.mounted()))

main()
