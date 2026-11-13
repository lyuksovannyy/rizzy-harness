#!/usr/bin/env python3
"""A.19 review, A19-1 and its re-check: a stop while one of the node's own
blocking threads is stuck.

The node's `read` tool reads a file on a mount that does not answer (the
guard tests' FUSE server, `../fuse_stall.py`: a network mount whose server
went away). The stop cancels the turn; the agent loop abandons the tool
after its 5 s grace and the turn ends `stopped`.

At `5add735` (the first review): the process then never exited, had
released the data directory's lock, and ignored SIGTERM and SIGINT; a
second `serve` started beside it.

Since `859e59c` `main` holds the lock outside the runtime, gives the
runtime 2 s and calls exit. A thread in a FUSE read whose request the
server holds cannot be killed by the kernel, so the process stays (its
leader a zombie) until the read returns. This prints, for that state: the
process's state, whether the lock can be taken, what a second `serve` and
`doctor` say, what the lock file's hint holds, whether a file the tool
would have written appears, and the exit status once the mount is released.
"""
import os, signal, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def threads(pid):
    try:
        return sorted("%s[%s]" % (t, open("/proc/%d/task/%s/stat" % (pid, t)).read().rsplit(")", 1)[1].split()[0])
                      for t in os.listdir("/proc/%d/task" % pid))
    except OSError:
        return []


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
        print("the node's read tool is waiting in a read of <run>/hang/ws/mnt/stall")
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        line = s.wait_line("stopped: every turn is closed", secs=100, where="err")
        print("%.1f s after SIGTERM the log says: %s" % (time.monotonic() - t0, line.split(" INFO ")[-1].strip()))
        st = s.exited(8)
        print("exit status 8 s later: %r (None: not reaped), process state %r, threads %s"
              % (st, state(s.pid), threads(s.pid)))
        print("the data directory's lock can be taken by someone else now: %s" % lock_free(data))
        with open(os.path.join(data, "lock")) as f:
            print("the lock file's hint: %r (the process: pid %d)" % (f.read().strip(), s.pid))
        c, v = s.call("GET", "/api/sessions/%s/events" % sid, timeout=3)
        print("its HTTP port answers: %s" % ("yes (%s)" % c if c else "no (%s)" % v[:60]))
        for name, sig in (("SIGTERM", signal.SIGTERM), ("SIGINT", signal.SIGINT), ("SIGKILL", signal.SIGKILL)):
            s.signal(sig)
            print("after one more %s: exit status %r, state %r" % (name, s.exited(2), state(s.pid)))
        # A second start on the same data directory.
        s2 = None
        try:
            s2 = Server(data)
            procs.append(s2)
            print("A SECOND `serve` STARTED on the same data directory (pid %d) while pid %d exists [%s]"
                  % (s2.pid, s.pid, state(s.pid)))
            s2.signal(signal.SIGTERM)
            s2.exited(100)
        except RuntimeError as e:
            print("a second `serve` is refused: %s" % str(e).split("\n")[-1].replace(root, "<run>/hang")[-200:])
        d = subprocess.run([EXE, "--data-dir", data, "doctor"], capture_output=True, cwd=root, timeout=60)
        for l in (d.stdout + d.stderr).decode().split("\n"):
            if "lock" in l:
                print("doctor: %s" % l.strip().replace(root, "<run>/hang"))
        stall.release()
        code = s.exited(15)
        print("the mount is released: the process is reaped with exit status %r; the lock is free: %s"
              % (code, lock_free(data)))
        s3 = Server(data)
        procs.append(s3)
        evs = s3.events(sid)
        print("the next start: ends %s; %s" % (ends(evs), kinds(evs)[-90:]))
        s3.signal(signal.SIGTERM)
        print("and stops with %r" % s3.exited(100))
    finally:
        stall.release()
        left = cleanup(procs, needle=root)
        print("cleanup: killed %s; mount left: %s" % (left or "nothing", stall.mounted()))

main()
