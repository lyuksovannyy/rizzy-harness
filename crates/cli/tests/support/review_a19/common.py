"""A.19 review: what the proof scripts share.

Every script starts the real binary (`serve`, `node run`) on its own data
directory under `<target>/review_a19/run-<pid>/` (never /tmp, never the
default data directory), on a port the kernel picks (`--bind 127.0.0.1:0`),
with the built-in demo provider. A signal goes only to a pid this script
started. A leftover is found by this run's folder in its command line or by
a `sleep <n>` unique to the run, and killed by that pid.

    RIZZY=<binary>   the binary under test (default: <target>/review_a19/rizzy-harness
                     when it exists, a copy of the branch's build made for the review,
                     since the shared target's binary is rebuilt by other branches;
                     else <target>/debug/rizzy-harness)
    A19_TARGET=<dir> the target directory (default: CARGO_TARGET_DIR, else <worktree>/target)
"""
import json, os, re, signal, subprocess, sys, threading, time, urllib.request, urllib.error

HERE = os.path.dirname(os.path.abspath(__file__))
WORKTREE = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
TARGET = os.environ.get("A19_TARGET", os.environ.get("CARGO_TARGET_DIR", os.path.join(WORKTREE, "target")))
_COPY = os.path.join(TARGET, "review_a19", "rizzy-harness")
EXE = os.environ.get("RIZZY", _COPY if os.path.exists(_COPY) else os.path.join(TARGET, "debug", "rizzy-harness"))
SCRATCH = os.path.join(TARGET, "review_a19", "run-%d" % os.getpid())
FUSE_STALL = os.path.join(HERE, "..", "fuse_stall.py")


def scratch(*parts):
    p = os.path.join(SCRATCH, *parts)
    os.makedirs(p, exist_ok=True)
    return os.path.realpath(p)


def write_config(data, ws, extra_node="", delay_ms=1):
    os.makedirs(data, exist_ok=True)
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write('workspace_roots = ["%s"]\nmax_approval_mode = "auto"\n%s' % (ws, extra_node))
    with open(os.path.join(data, "config.toml"), "w") as f:
        f.write('[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\n'
                'mock_delay_ms = %d\n\n[[providers.models]]\nid = "mock"\n' % delay_ms)


class Proc:
    """A process of the binary this script started; stderr and stdout kept."""

    def __init__(self, data, args, env=None, preexec=None, cwd=None, restore_signals=True):
        e = dict(os.environ)
        e.update({"HARNESS_LOG": "info", "NO_COLOR": "1"})
        e.pop("HARNESS_JOIN_TOKEN", None)
        e.update(env or {})
        self.data = data
        self.p = subprocess.Popen([EXE, "--data-dir", data] + args, stdin=subprocess.DEVNULL,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=e,
                                  cwd=cwd or os.path.dirname(data), preexec_fn=preexec,
                                  restore_signals=restore_signals)
        self.pid = self.p.pid
        self.out, self.err = [], []
        self._t = [threading.Thread(target=self._pump, args=(self.p.stdout, self.out), daemon=True),
                   threading.Thread(target=self._pump, args=(self.p.stderr, self.err), daemon=True)]
        for t in self._t:
            t.start()

    @staticmethod
    def _pump(f, into):
        for line in iter(f.readline, b""):
            into.append((time.monotonic(), line.decode(errors="replace").rstrip("\n")))

    def wait_line(self, needle, secs=60.0, where="out"):
        by = time.monotonic() + secs
        while time.monotonic() < by:
            for _, l in list(getattr(self, where)):
                if needle in l:
                    return l
            if self.p.poll() is not None:
                time.sleep(0.2)
                for _, l in list(getattr(self, where)):
                    if needle in l:
                        return l
                raise RuntimeError("ended (%s) before %r:\n%s" % (self.p.returncode, needle, self.log()))
            time.sleep(0.01)
        raise RuntimeError("no %r within %s s:\n%s" % (needle, secs, self.log()))

    def signal(self, sig):
        os.kill(self.pid, sig)

    def exited(self, secs):
        """The exit status within `secs` (negative: died of that signal), or None."""
        try:
            return self.p.wait(timeout=secs)
        except subprocess.TimeoutExpired:
            return None

    def log(self, last=None):
        ls = [l for _, l in self.err]
        return "\n".join(ls[-last:] if last else ls)

    def kill(self):
        """SIGKILL by pid (this script's own child) and reap."""
        if self.p.poll() is None:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except OSError:
                pass
        try:
            self.p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass


class Server(Proc):
    def __init__(self, data, extra=(), env=None, preexec=None, restore_signals=True):
        super().__init__(data, ["serve", "--bind", "127.0.0.1:0"] + list(extra), env, preexec,
                         restore_signals=restore_signals)
        line = self.wait_line("listening on http://", secs=240)
        self.addr = line.split("listening on http://")[1].strip()
        with open(os.path.join(data, "api-token")) as f:
            self.token = f.read().strip()

    def call(self, method, path, body=None, timeout=30.0, token=None):
        req = urllib.request.Request("http://%s%s" % (self.addr, path), method=method)
        req.add_header("Authorization", "Bearer " + (token or self.token))
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            req.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(req, data, timeout=timeout) as r:
                code, text = r.status, r.read().decode()
        except urllib.error.HTTPError as e:
            code, text = e.code, e.read().decode()
        except Exception as e:  # connection refused, reset, timeout
            return 0, "%s: %s" % (type(e).__name__, e)
        try:
            return code, json.loads(text)
        except ValueError:
            return code, text

    def post(self, path, body):
        c, v = self.call("POST", path, body)
        if not 200 <= c < 300:
            raise RuntimeError("%s: %s %s" % (path, c, v))
        return v

    def session(self, ws, mode="auto", node=None):
        b = {"cwd": ws, "approval_mode": mode}
        if node:
            b["node_id"] = node
        return self.post("/api/sessions", b)["session_id"]

    def events(self, sid):
        c, v = self.call("GET", "/api/sessions/%s/events?limit=5000" % sid)
        if c != 200:
            raise RuntimeError("events %s: %s %s" % (sid, c, v))
        return v["events"]

    def wait_event(self, sid, kind, secs=30.0):
        by = time.monotonic() + secs
        while True:
            evs = self.events(sid)
            if any(e["kind"] == kind for e in evs):
                return evs
            if time.monotonic() > by:
                raise RuntimeError("no %s in %s: %s\n%s" % (kind, sid, kinds(evs), self.log(20)))
            time.sleep(0.05)


def kinds(evs):
    out = []
    for e in evs:
        k = e["kind"]
        p = e.get("payload", {})
        if k == "turn_ended":
            k += "(%s)" % p.get("reason")
        elif k == "approval_resolved":
            k += "(%s by %s)" % (p.get("decision"), p.get("by"))
        elif k == "error":
            k += "(%s)" % (p.get("message") or "")[:60]
        out.append("%s:%s" % (e.get("seq"), k))
    return " ".join(out)


def ends(evs):
    return [e["payload"].get("reason") for e in evs if e["kind"] == "turn_ended"]


# ---- processes, found from outside ----

def stat_fields(pid):
    try:
        with open("/proc/%d/stat" % pid) as f:
            return f.read().rsplit(")", 1)[1].split()
    except OSError:
        return None


def state(pid):
    f = stat_fields(pid)
    return f[0] if f else None


def alive(pid):
    s = state(pid)
    return s is not None and s not in "ZXx"


def cmdline(pid):
    try:
        with open("/proc/%d/cmdline" % pid, "rb") as f:
            return f.read()
    except OSError:
        return b""


def pids():
    return [int(p) for p in os.listdir("/proc") if p.isdigit()]


def sleeps(n):
    want = b"sleep\0%d\0" % n
    return [p for p in pids() if cmdline(p) == want and alive(p)]


def cwd(pid):
    try:
        return os.readlink("/proc/%d/cwd" % pid)
    except OSError:
        return ""


def holding(needle):
    """Live processes with `needle` (this run's folder) in their command
    line or as (a parent of) their working directory."""
    n = needle.encode()
    me = os.getpid()
    return [p for p in pids() if p != me and (n in cmdline(p) or cwd(p).startswith(needle)) and alive(p)]


def describe(ps):
    return "; ".join("%d[%s] %s" % (p, state(p), cmdline(p).replace(b"\0", b" ").decode(errors="replace")[:100])
                     for p in ps)


def wait_for(cond, secs=30.0, step=0.01):
    by = time.monotonic() + secs
    while time.monotonic() < by:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


def gone_within(fn, secs):
    by = time.monotonic() + secs
    left = fn()
    while left and time.monotonic() < by:
        time.sleep(0.02)
        left = fn()
    return left


def kill_pids(ps):
    for p in ps:
        try:
            os.kill(p, signal.SIGKILL)
        except OSError:
            pass


def cleanup(procs, needle=None, secs=()):
    """End everything this run started; report what was still there."""
    for p in procs:
        p.kill()
    left = []
    for _ in range(3):
        l = (holding(needle) if needle else []) + [p for n in secs for p in sleeps(n)]
        left += l
        kill_pids(l)
        time.sleep(0.05)
    return sorted(set(left))


def lock_free(data):
    """Whether <data>/lock can be taken now (flock, non-blocking); released at once."""
    import fcntl
    with open(os.path.join(data, "lock"), "r+") as f:
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return False
        fcntl.flock(f, fcntl.LOCK_UN)
        return True


class Stall:
    """`fuse_stall.py` mounted at `mnt`: every READ of `mnt/stall` is held
    until `release()` (the guard tests' server, used as it is)."""

    def __init__(self, mnt):
        os.makedirs(mnt, exist_ok=True)
        self.mnt = mnt
        self.p = subprocess.Popen(["python3", "-I", FUSE_STALL, mnt], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE)
        l = self.p.stdout.readline().decode().strip()
        if l != "mounted":
            self.p.kill()
            raise RuntimeError("FUSE does not mount here: %r" % l)

    def expect(self, what, secs=30.0):
        import select
        by = time.monotonic() + secs
        while time.monotonic() < by:
            r, _, _ = select.select([self.p.stdout], [], [], 0.2)
            if r:
                l = self.p.stdout.readline().decode().strip()
                if l == what:
                    return True
        return False

    def release(self):
        if self.p.poll() is None:
            try:
                self.p.stdin.close()
            except OSError:
                pass
            try:
                self.p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.kill(self.p.pid, signal.SIGKILL)
                self.p.wait()
        subprocess.run(["fusermount3", "-u", "-z", self.mnt], stderr=subprocess.DEVNULL)

    def mounted(self):
        with open("/proc/self/mountinfo") as f:
            return any(self.mnt in l for l in f)


class Browser:
    """A signed-in user of a `Server` this script started: the cookie and
    the login key, as the page sends them. Passwords are random test values
    made here, used only against this run's own server on 127.0.0.1."""

    def __init__(self, server, cookie, key):
        self.s, self.cookie, self.key = server, cookie, key

    @staticmethod
    def _raw(server, method, path, body, headers):
        req = urllib.request.Request("http://%s%s" % (server.addr, path), method=method)
        req.add_header("Origin", "http://" + server.addr)
        for k, v in headers.items():
            req.add_header(k, v)
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            req.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(req, data, timeout=30) as r:
                return r.status, r.read().decode(), r.headers
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode(), e.headers
        except Exception as e:
            return 0, "%s: %s" % (type(e).__name__, e), {}

    @classmethod
    def _signed(cls, server, path, body, headers=None):
        c, text, h = cls._raw(server, "POST", path, body, headers or {})
        if not 200 <= c < 300:
            raise RuntimeError("%s: %s %s" % (path, c, text))
        cookie = h.get("Set-Cookie").split(";")[0]
        return cls(server, cookie, json.loads(text).get("login_key"))

    @classmethod
    def claim(cls, server, username):
        with open(os.path.join(server.data, "claim-code")) as f:
            code = f.read().strip()
        return cls._signed(server, "/api/setup",
                           {"claim_code": code, "username": username, "password": _secret()})

    def member(self, username):
        """A member made by this (admin) user, signed in with their own password."""
        c, v = self.call("POST", "/api/users", {"username": username})
        if c != 200:
            raise RuntimeError("users: %s %s" % (c, v))
        first = Browser._signed(self.s, "/api/login", {"username": username, "password": v["password"]})
        return Browser._signed(self.s, "/api/me/password",
                               {"current": v["password"], "new": _secret()}, first._headers())

    def on(self, server):
        """The same login against the next start of the same data directory."""
        return Browser(server, self.cookie, self.key)

    def _headers(self):
        h = {"Cookie": self.cookie}
        if self.key:
            h["x-harness-login"] = self.key
        return h

    def call(self, method, path, body=None):
        c, text, _ = self._raw(self.s, method, path, body, self._headers())
        try:
            return c, json.loads(text)
        except ValueError:
            return c, text


def _secret():
    import secrets
    return secrets.token_urlsafe(18)
