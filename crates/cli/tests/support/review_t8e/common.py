"""T.8e review: what the proof scripts share.

Every script plays the node with the real `rizzy-harness sandbox-guard`, the
real `bwrap` and the node's own command line (`tools/src/contain.rs`
`Contain::args`, `tools/src/sandbox.rs` `wrapper`). Scratch files go under
`<worktree>/target/review_t8e/` (never /tmp). A process is only ever found by
a marker root or a `sleep <n>` unique to the run, and killed by that pid.
"""
import os, signal, socket, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
WORKTREE = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
EXE = os.environ.get("RIZZY", os.path.join(WORKTREE, "target", "debug", "rizzy-harness"))
BWRAP = os.environ.get("BWRAP", "/usr/bin/bwrap")
SCRATCH = os.path.join(WORKTREE, "target", "review_t8e", "run-%d" % os.getpid())


def scratch(*parts):
    p = os.path.join(SCRATCH, *parts)
    os.makedirs(p, exist_ok=True)
    return p


def guard_argv(root, session, cwd, script, die_with_parent=True):
    """The node's command line for `bash -c <script>` in `cwd`."""
    bw = [BWRAP]
    if die_with_parent:
        bw.append("--die-with-parent")
    bw += ["--new-session", "--unshare-pid", "--dev-bind", "/", "/",
           "--proc", "/proc", "--chdir", cwd, "--"]
    inner = [EXE, "sandbox-exec", "--mark", root, "--mark-session", session,
             "--best-effort", "--rw", "system", "/", "--", "bash", "-c", script]
    return [EXE, "sandbox-guard", "--mark", root, "--mark-session", session, "--"] + bw + inner


def spawn_guard(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL):
    """Spawn a guard as `contain::spawn` does: its stdin one end of a socket
    pair, the other end (close-on-exec) kept here; its own process group; cwd
    `/`. Returns (Popen, our end)."""
    ours, theirs = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    if "--node" not in argv:
        # Since the fixes (1e73622): the guard is told its node's pid and
        # takes any other parent as the kill order. The node is whoever
        # spawns it: this process, now.
        at = argv.index("sandbox-guard") + 1
        argv = argv[:at] + ["--node", str(os.getpid())] + argv[at:]
    p = subprocess.Popen(argv, stdin=theirs.fileno(), stdout=stdout, stderr=stderr,
                         cwd="/", process_group=0, close_fds=False)
    theirs.close()
    return p, ours


def stat_fields(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().rsplit(")", 1)[1].split()
    except OSError:
        return None


def state(pid):
    f = stat_fields(pid)
    return f[0] if f else None


def alive(pid):
    s = state(pid)
    return s is not None and s not in "ZXx"


def ppid(pid):
    f = stat_fields(pid)
    return int(f[1]) if f else None


def cmdline(pid):
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            return f.read()
    except OSError:
        return b""


def pids():
    return [int(p) for p in os.listdir("/proc") if p.isdigit()]


def sleeps(n):
    want = b"sleep\0%d\0" % n
    return [p for p in pids() if cmdline(p) == want and alive(p)]


def holding(needle):
    """Live processes whose command line holds `needle` (a run's marker root)."""
    n = needle.encode()
    me = os.getpid()
    return [p for p in pids() if p != me and n in cmdline(p) and alive(p)]


def children(pid):
    try:
        with open(f"/proc/{pid}/task/{pid}/children") as f:
            return [int(x) for x in f.read().split()]
    except OSError:
        return []


def describe(ps):
    return "; ".join("%d[%s] %s" % (p, state(p), cmdline(p).replace(b"\0", b" ").decode(errors="replace")[:110])
                     for p in ps)


def wait_for(cond, secs=10.0, step=0.005):
    by = time.monotonic() + secs
    while time.monotonic() < by:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


def gone_within(fn, secs):
    """Wait up to `secs` for fn() to be empty; what it still returns."""
    by = time.monotonic() + secs
    left = fn()
    while left and time.monotonic() < by:
        time.sleep(0.01)
        left = fn()
    return left


def kill_pids(ps, sig=signal.SIGKILL):
    for p in ps:
        try:
            os.kill(p, sig)
        except OSError:
            pass


def cleanup(root, secs=()):
    """SIGKILL what this run left: processes carrying its marker root, its
    sleeps and the namespace inits above them. Returns what it had to kill."""
    killed = []
    for _ in range(5):
        left = set(holding(root))
        for n in secs:
            for s in sleeps(n):
                left.add(s)
                pp = ppid(s)
                if pp and pp > 1 and root.encode() in cmdline(pp):
                    left.add(pp)
        if not left:
            break
        killed += sorted(left)
        kill_pids(left)
        time.sleep(0.1)
    return killed


def busy_wait(us):
    end = time.perf_counter() + us / 1e6
    while time.perf_counter() < end:
        pass


def my_fuse_mounts():
    with open("/proc/self/mountinfo") as f:
        return [l.split()[4] for l in f if "t8e-review" in l]
