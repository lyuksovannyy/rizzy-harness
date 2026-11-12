#!/usr/bin/env python3
"""T.8a review, T8A-2: how long after a spawn does killing the node, or
bubblewrap's process group, leave the namespace running?

    kill_window.py node  <delay-us>...   SIGKILL to the spawning process (the
                                         node) <delay> after its spawn returned
    kill_window.py group <delay-us>...   SIGKILL to bubblewrap's process group
                                         (KillGroupOnDrop, kill_on_drop)

Each delay is tried REPS times (default 40). The command line is the node's
(`--die-with-parent --new-session --unshare-pid --dev-bind / / --proc /proc`)
running `sleep <n>`; survivors are found by that command line 1.5 s later and
killed by pid, with the bwrap init that is their parent. Writes no file.
"""
import os, signal, subprocess, sys, time
REPS = int(os.environ.get("REPS", "40"))
D = os.path.dirname(os.path.abspath(__file__))
SECS = 976700 + (os.getpid() % 90)

def sleeps():
    want = b"sleep\0%d\0" % SECS
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                if open(f"/proc/{p}/cmdline", "rb").read() == want:
                    st = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[0]
                    if st not in "ZXx":
                        out.append(int(p))
            except OSError:
                pass
    return out

def bwrap_argv():
    return ["bwrap", "--die-with-parent", "--new-session", "--unshare-pid",
            "--dev-bind", "/", "/", "--proc", "/proc", "--chdir", D, "--",
            "sleep", str(SECS)]

def busy_wait(us):
    end = time.perf_counter() + us / 1e6
    while time.perf_counter() < end:
        pass

def one(mode, us):
    if mode == "group":
        p = subprocess.Popen(bwrap_argv(), process_group=0,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
        busy_wait(us)
        try: os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError: pass
        p.wait()
    else:
        r, w = os.pipe()
        node = os.fork()
        if node == 0:
            os.close(r)
            p = subprocess.Popen(bwrap_argv(), process_group=0,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
            os.write(w, b"x")
            time.sleep(1000)
            os._exit(0)
        os.close(w)
        os.read(r, 1); os.close(r)
        busy_wait(us)
        os.kill(node, signal.SIGKILL)
        os.waitpid(node, 0)
    # anything still alive 1.5 s later survived its killer
    time.sleep(0.05)
    by = time.time() + 1.5
    left = sleeps()
    while left and time.time() < by:
        time.sleep(0.05); left = sleeps()
    for pid in left:
        # the survivor: kill the sleep itself and its namespace init (its parent)
        try:
            ppid = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[1])
        except OSError:
            ppid = 0
        for q in (pid, ppid):
            if q > 1:
                try:
                    if q == pid or b"bwrap" in open(f"/proc/{q}/cmdline", "rb").read():
                        os.kill(q, signal.SIGKILL)
                except OSError: pass
    return bool(left)

mode = sys.argv[1]
for us in map(int, sys.argv[2:]):
    n = sum(one(mode, us) for _ in range(REPS))
    print(f"{mode}: kill {us:>6} us after spawn: {n}/{REPS} namespaces survived", flush=True)
