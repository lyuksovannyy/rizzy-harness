#!/usr/bin/env python3
"""Re-check of S2-1 (the loopback origin the server accepted but did not
hold) against the fix on `fix/loopback-both-families`: does the server now
hold every loopback address it accepts as its own, at its start, while it
runs and through its stop. The real binary, on ports the kernel picks.

    RIZZY=<fixed binary> T8D_TARGET=<its target dir> step2_loopback.py [original|held|start|stop|tls|binds|slow|down|netoff|prebound|all]

Every check is a bind or a request made by this script or by a command of
the server's own node (its sandbox, the network on), on this machine only.
"""
import errno, os, signal, socket, ssl, subprocess, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from step2_common import *

# Run by this script and, as a file in the chat's folder, by the agent's
# shell: every way another program of the same user could take the port.
TRY = r'''
import errno, socket, sys
port = int(sys.argv[1])
def t(name, fam, addr, typ=socket.SOCK_STREAM, reuseaddr=False, reuseport=False, v6only=None):
    s = socket.socket(fam, typ)
    try:
        if reuseaddr: s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        if reuseport: s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
        if v6only is not None: s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, int(v6only))
        s.bind((addr, port))
        if typ == socket.SOCK_STREAM: s.listen(1)
        print("%-44s BOUND" % name)
    except OSError as e:
        print("%-44s %s" % (name, errno.errorcode.get(e.errno, e.errno)))
    finally:
        s.close()
V6, V4 = socket.AF_INET6, socket.AF_INET
for fam, addr, n in ((V6, "::1", "[::1]"), (V4, "127.0.0.1", "127.0.0.1")):
    t(n + " plain", fam, addr)
    t(n + " SO_REUSEADDR", fam, addr, reuseaddr=True)
    t(n + " SO_REUSEPORT", fam, addr, reuseport=True)
    t(n + " both", fam, addr, reuseaddr=True, reuseport=True)
t("[::] IPv6 only", V6, "::", v6only=True, reuseaddr=True, reuseport=True)
t("[::] dual stack", V6, "::", v6only=False, reuseaddr=True, reuseport=True)
t("0.0.0.0", V4, "0.0.0.0", reuseaddr=True, reuseport=True)
t("[::ffff:127.0.0.1] (IPv4-mapped)", V6, "::ffff:127.0.0.1", v6only=False, reuseaddr=True, reuseport=True)
t("127.0.0.2 (another loopback address)", V4, "127.0.0.2")
t("UDP [::1]", V6, "::1", typ=socket.SOCK_DGRAM)
t("UDP 127.0.0.1", V4, "127.0.0.1", typ=socket.SOCK_DGRAM)
'''


def listeners(port):
    out = subprocess.run(["ss", "-Hltn", "sport = :%d" % port], capture_output=True, text=True).stdout
    return sorted(set(l.split()[3] for l in out.splitlines() if l.split()))


def start(data, bind, extra=(), wait=True):
    """`serve --bind <bind>` on `data`; the Proc and the port it printed."""
    p = Proc(data, ["serve", "--bind", bind] + list(extra))
    if not wait:
        return p, None
    line = p.wait_line("listening on http", secs=240)
    port = int(line.rsplit(":", 1)[1].strip().rstrip("/"))
    return p, port


def token(data):
    with open(os.path.join(data, "api-token")) as f:
        return f.read().strip()


def get(url, host=None, timeout=5, ctx=None):
    import urllib.request
    req = urllib.request.Request(url)
    if host:
        req.add_header("Host", host)
    try:
        with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
            return r.status, dict(r.headers), r.read(300).decode(errors="replace")
    except Exception as e:
        code = getattr(e, "code", 0)
        body = e.read(200).decode(errors="replace") if hasattr(e, "read") else "%s: %s" % (type(e).__name__, e)
        return code, dict(getattr(e, "headers", {}) or {}), body


def hold(fam, addr, port):
    s = socket.socket(fam, socket.SOCK_STREAM)
    s.bind((addr, port))
    s.listen(8)
    return s


def held(root, procs):
    data, ws = os.path.join(root, "h-data"), os.path.join(root, "h-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    s, port = start(data, "127.0.0.1:0")
    procs.append(s)
    print("1. --bind 127.0.0.1:0 -> port %d; listening sockets on it: %s" % (port, listeners(port)))
    print("   " + [l for _, l in s.err if "listening on" in l][-1].split("rizzy_harness")[-1][-90:])
    print("   another program of the same user tries the port (this script):")
    out = subprocess.run([sys.executable, "-I", "-c", TRY, str(port)], capture_output=True, text=True).stdout
    for l in out.splitlines():
        print("     " + l)
    # The same from the agent's shell (its sandbox, the network on).
    with open(os.path.join(ws, "try.py"), "w") as f:
        f.write(TRY)
    tok = token(data)
    def call(method, path, body=None):
        c, text, _, _ = raw("127.0.0.1:%d" % port, method, path, {"Authorization": "Bearer " + tok}, body)
        return c, (json.loads(text) if text[:1] in "{[" else text)
    c, v = call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
    sid = v["session_id"]
    call("POST", "/api/sessions/%s/prompt" % sid, script([
        fg("python3 try.py %d" % port, 30),
        fg("grep -E 'CapEff|CapBnd' /proc/self/status; readlink /proc/self/ns/net; readlink /proc/self/ns/user; "
           "cat /proc/sys/net/ipv6/conf/lo/disable_ipv6; test -w /proc/sys/net/ipv6/conf/lo/disable_ipv6 && echo WRITABLE || echo not-writable", 30)]))
    def done():
        c, v = call("GET", "/api/sessions/%s/events?limit=5000" % sid)
        return v["events"] if c == 200 and any(e["kind"] == "turn_ended" for e in v["events"]) else None
    evs = wait_for(done, 90, 0.1)
    res = results(evs)
    print("   the same from a command of the server's own node (bubblewrap, the network on):")
    for l in res[0].splitlines():
        if l.strip() and not l.startswith("[exit"):
            print("     " + l)
    print("   what that command is, for the machine's network (read only):")
    print("     " + " | ".join(l.strip() for l in res[1].splitlines() if l.strip() and not l.startswith("[exit")))
    print("     this script's own: net %s, user %s" % (os.readlink("/proc/self/ns/net"), os.readlink("/proc/self/ns/user")))
    ours = lambda u: "the server's own page" if "<div id=\"root\">" in get(u)[2] or "<!doctype html" in get(u)[2].lower() else get(u)[:1]
    print("   who answers: http://localhost:%d/ -> %s; http://[::1]:%d/ -> %s; http://127.0.0.1:%d/ -> %s"
          % (port, ours("http://localhost:%d/" % port), port, ours("http://[::1]:%d/" % port), port, ours("http://127.0.0.1:%d/" % port)))
    print("   Host names the server takes as its own (over 127.0.0.1 and over [::1]):")
    names = ["127.0.0.1:%d", "[::1]:%d", "localhost:%d", "LOCALHOST:%d", "localhost.:%d", "foo.localhost:%d", "ip6-localhost:%d",
             "ip6-loopback:%d", "127.0.0.2:%d", "127.1:%d", "2130706433:%d", "0.0.0.0:%d", "[::]:%d", "[::ffff:127.0.0.1]:%d",
             "[0:0:0:0:0:0:0:1]:%d", "[::1%%lo]:%d", "localhost", "127.0.0.1", "localhost:%d.", " localhost:%d", "localhost:0%d",
             socket.gethostname() + ":%d", "evil.example:%d"]
    took, refused = [], []
    for n in names:
        h = n % port if "%d" in n else n
        a = raw("127.0.0.1:%d" % port, "GET", "/", {"Host": h})[0]
        b = raw("[::1]:%d" % port, "GET", "/", {"Host": h})[0]
        (took if a == 200 or b == 200 else refused).append("%s (%s/%s)" % (h, a, b))
    print("     taken:   %s" % "; ".join(took))
    print("     refused: %s" % "; ".join(refused))
    # Origin: the same list decides.
    o = [raw("127.0.0.1:%d" % port, "POST", "/api/logout", {"Origin": x % port, "Cookie": "rizzy-harness_session=" + "a" * 64})[0]
         for x in ("http://localhost:%d", "http://[::1]:%d", "http://127.0.0.2:%d", "http://localhost.:%d", "http://foo.localhost:%d")]
    print("   a cookie write with Origin localhost / [::1] / 127.0.0.2 / localhost. / foo.localhost: %s (403 = origin not allowed)" % o)
    c, h, _ = get("http://127.0.0.1:%d/" % port)
    print("   the page's headers name another transport (Alt-Svc, for HTTP/3 over UDP): %s" % ("alt-svc" in [k.lower() for k in h]))
    s.signal(signal.SIGTERM)
    print("   SIGTERM: exit %r; listening sockets on the port afterwards: %s" % (s.exited(60), listeners(port)))


def start_checks(root, procs):
    data, ws = os.path.join(root, "s-data"), os.path.join(root, "s-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    for fam, addr, name in ((socket.AF_INET6, "::1", "[::1]"), (socket.AF_INET, "127.0.0.1", "127.0.0.1")):
        port = free_port()
        k = hold(fam, addr, port)
        t0 = time.monotonic()
        p, _ = start(data, "127.0.0.1:%d" % port, wait=False)
        procs.append(p)
        code = p.exited(60)
        took = time.monotonic() - t0
        last = (p.log(3) or "").splitlines()[-1:] or [""]
        print("2. another program holds %s:%d before the start: exit %r after %.2f s; listening on the port: %s"
              % (name, port, code, took, listeners(port)))
        print("   it says: %s" % short(last[0].split("Error: ")[-1], 330))
        print("   the data directory afterwards: %s; its lock is free: %s"
              % (sorted(os.listdir(data)), lock_free(data) if os.path.exists(os.path.join(data, "lock")) else "no lock file"))
        if fam == socket.AF_INET6:
            # A supervisor that starts it again and again.
            codes, t0 = [], time.monotonic()
            for _ in range(15):
                q, _ = start(data, "127.0.0.1:%d" % port, wait=False)
                procs.append(q)
                codes.append(q.exited(60))
            print("   15 starts in a row against the held port: exit codes %s in %.1f s; the data directory: %s; processes of it left: %d"
                  % (sorted(set(codes)), time.monotonic() - t0, sorted(os.listdir(data)), len(holding(data))))
        k.close()
        p, got = start(data, "127.0.0.1:%d" % port)
        procs.append(p)
        print("   the program gives the port back: the next start holds %s; http://localhost:%d/ is the server's: %s"
              % (listeners(port), port, get("http://localhost:%d/" % port)[0] == 200))
        p.signal(signal.SIGTERM)
        p.exited(60)
    # The window between the two binds: a program that tries [::1]:<port>
    # in a tight loop from the moment 127.0.0.1:<port> is taken.
    wins, served_both, refused = 0, 0, 0
    for i in range(12):
        port = free_port()
        stop, got = threading.Event(), []
        def racer():
            while not stop.is_set():
                # Only once the server's first bind is there.
                try:
                    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                    probe.bind(("127.0.0.1", port))
                    probe.close()
                    continue
                except OSError:
                    probe.close()
                try:
                    got.append(hold(socket.AF_INET6, "::1", port))
                    return
                except OSError:
                    pass
        th = threading.Thread(target=racer)
        th.start()
        p, _ = start(data, "127.0.0.1:%d" % port, wait=False)
        procs.append(p)
        code = None
        by = time.monotonic() + 60
        while time.monotonic() < by:
            code = p.p.poll()
            if code is not None or len(listeners(port)) >= 2 or got:
                break
            time.sleep(0.001)
        if got:
            wins += 1
            code = p.exited(30)
            refused += code not in (None, 0)
        else:
            served_both += 1
        stop.set()
        th.join()
        for k in got:
            k.close()
        if p.p.poll() is None:
            p.signal(signal.SIGTERM)
            p.exited(60)
    print("3. 12 starts with a program racing for [::1]:<port> from the first bind on: the program got it %d time(s), "
          "and the start was refused each of those times: %s; the server held both the other %d"
          % (wins, refused == wins, served_both))
    # Port 0 with the other family of the first choice taken cannot be
    # arranged from outside (the kernel picks); many port-0 starts instead.
    both = 0
    for i in range(4):
        p, port = start(data, "127.0.0.1:0")
        procs.append(p)
        both += listeners(port) == ["127.0.0.1:%d" % port, "[::1]:%d" % port]
        p.signal(signal.SIGTERM)
        p.exited(60)
    print("   4 starts on port 0: both families of the chosen port held in %d" % both)


def stop_checks(root, procs):
    data, ws = os.path.join(root, "t-data"), os.path.join(root, "t-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    s, port = start(data, "127.0.0.1:0")
    procs.append(s)
    # The cap is one for both: fill it from one listener, ask on the other.
    held_, n = [], 0
    try:
        for i in range(520):
            k = socket.create_connection(("::1", port), timeout=5)
            held_.append(k)
    except OSError as e:
        print("   (opened %d: %s)" % (len(held_), e))
    time.sleep(0.5)
    t0 = time.monotonic()
    c = get("http://127.0.0.1:%d/" % port, timeout=30)[0]
    t1 = time.monotonic() - t0
    fds = len(os.listdir("/proc/%d/fd" % s.pid))
    print("4. %d idle connections on [::1] (the cap is 512 for the whole server), then a request on 127.0.0.1: %s after %.1f s "
          "(a connection's head must come within 10 s); the server's descriptors: %d" % (len(held_), c, t1, fds))
    for k in held_:
        k.close()
    time.sleep(0.5)
    # Half on each: no listener has a cap of its own.
    a = [socket.create_connection(("::1", port), timeout=5) for _ in range(300)]
    b = [socket.create_connection(("127.0.0.1", port), timeout=5) for _ in range(300)]
    time.sleep(1.0)
    fds = len(os.listdir("/proc/%d/fd" % s.pid))
    base = 0
    print("   300 on each at once: the server holds %d descriptors (600 connections would be over the cap of 512 if each listener had its own)" % fds)
    for k in a + b:
        k.close()
    time.sleep(0.5)
    # A flood on one listener while the other is asked.
    stop = threading.Event()
    def flood():
        while not stop.is_set():
            try:
                k = socket.create_connection(("::1", port), timeout=2)
                k.close()
            except OSError:
                pass
    ths = [threading.Thread(target=flood) for _ in range(8)]
    for t in ths:
        t.start()
    times = []
    for i in range(40):
        t0 = time.monotonic()
        c = get("http://127.0.0.1:%d/" % port, timeout=30)[0]
        times.append((time.monotonic() - t0, c))
    stop.set()
    for t in ths:
        t.join()
    ok = [d for d, c in times if c == 200]
    print("   8 threads opening and closing connections on [::1] while 127.0.0.1 is asked 40 times: %d answered 200, slowest %.0f ms"
          % (len(ok), max(ok) * 1000 if ok else -1))
    # The stop with connections open on both listeners: an idle one, and
    # one with half a request sent.
    idle6 = socket.create_connection(("::1", port), timeout=5)
    half6 = socket.create_connection(("::1", port), timeout=5)
    half6.sendall(b"GET / HTTP/1.1\r\nHost: localhost:%d\r\n" % port)
    idle4 = socket.create_connection(("127.0.0.1", port), timeout=5)
    time.sleep(0.3)
    t0 = time.monotonic()
    s.signal(signal.SIGTERM)
    code = s.exited(90)
    took = time.monotonic() - t0
    def refused(fam, addr):
        k = socket.socket(fam, socket.SOCK_STREAM)
        k.settimeout(2)
        try:
            k.connect((addr, port))
            return False
        except OSError as e:
            return e.errno == errno.ECONNREFUSED
        finally:
            k.close()
    print("5. SIGTERM with an idle and a half-sent connection open on [::1] and an idle one on 127.0.0.1: exit %r after %.1f s; "
          "listening afterwards: %s; a connect is refused on [::1]: %s, on 127.0.0.1: %s"
          % (code, took, listeners(port), refused(socket.AF_INET6, "::1"), refused(socket.AF_INET, "127.0.0.1")))
    print("   it said: %s" % short((s.log(2) or "").splitlines()[-1].split("rizzy_harness")[-1], 120))
    for k in (idle6, half6, idle4):
        k.close()
    print("   the data directory's lock is free: %s; processes of it left: %d" % (lock_free(data), len(holding(data))))


def tls(root, procs):
    data, ws = os.path.join(root, "x-data"), os.path.join(root, "x-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    s, port = start(data, "127.0.0.1:0", ["--tls"])
    procs.append(s)
    print("6. --tls on port %d: listening %s" % (port, listeners(port)))
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    certs = {}
    for fam, addr, name in ((socket.AF_INET, "127.0.0.1", "127.0.0.1"), (socket.AF_INET6, "::1", "[::1]")):
        try:
            with socket.create_connection((addr, port), timeout=5) as k:
                with ctx.wrap_socket(k, server_hostname="localhost") as t:
                    der = t.getpeercert(binary_form=True)
                    t.sendall(b"GET / HTTP/1.1\r\nHost: localhost:%d\r\nConnection: close\r\n\r\n" % port)
                    first = t.recv(200).split(b"\r\n")[0].decode()
            certs[name] = der
            san = subprocess.run(["openssl", "x509", "-inform", "der", "-noout", "-ext", "subjectAltName"],
                                 input=der, capture_output=True).stdout.decode().replace("\n", " ")
            print("   %s: TLS %s; the request: %s; certificate names: %s" % (name, t.version() if False else "ok", first, " ".join(san.split())))
        except Exception as e:
            print("   %s: %s: %s" % (name, type(e).__name__, e))
        # Plain HTTP to the same listener gets no page.
        c = raw("%s:%d" % (name, port), "GET", "/", {"Host": "localhost:%d" % port}, timeout=12)
        print("   %s asked in plain HTTP: %s %s" % (name, c[0], short(c[1], 60)))
    print("   one certificate on both: %s" % (len(set(certs.values())) == 1 and len(certs) == 2))
    out = subprocess.run([sys.executable, "-I", "-c", TRY, str(port)], capture_output=True, text=True).stdout
    print("   another program on the port: %s" % "; ".join(" ".join(l.split()) for l in out.splitlines()[:2] + out.splitlines()[4:6]))
    s.signal(signal.SIGTERM)
    print("   SIGTERM: exit %r; listening afterwards: %s" % (s.exited(60), listeners(port)))


def binds(root, procs):
    data, ws = os.path.join(root, "b-data"), os.path.join(root, "b-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    for bind in ("127.0.0.5:%d", "[::1]:%d", "[::ffff:127.0.0.1]:%d"):
        port = free_port()
        try:
            s, got = start(data, bind % port)
        except RuntimeError as e:
            print("7. --bind %s: no start: %s" % (bind % port, short(str(e).splitlines()[-1], 200)))
            continue
        procs.append(s)
        took = []
        for n in ("127.0.0.5:%d", "127.0.0.1:%d", "[::1]:%d", "localhost:%d", "[::ffff:127.0.0.1]:%d", "127.0.0.2:%d"):
            h = n % port
            target = (bind % port) if not bind.startswith("[::ffff") else "127.0.0.1:%d" % port
            c = raw(target, "GET", "/", {"Host": h})[0]
            took.append("%s %s" % (h.rsplit(":", 1)[0], c))
        out = subprocess.run([sys.executable, "-I", "-c", TRY, str(port)], capture_output=True, text=True).stdout
        free = [" ".join(l.split()[:-1]) for l in out.splitlines() if l.endswith("BOUND") and "UDP" not in l]
        print("7. --bind %s: listening %s" % (bind % port, listeners(port)))
        print("   Host taken: %s" % "; ".join(took))
        print("   TCP addresses of that port another program can still take: %s" % (free or "none"))
        s.signal(signal.SIGTERM)
        s.exited(60)
    # The wildcard binds are not started (a port open to the network on an
    # unclaimed server); what a wildcard socket covers is the kernel's, so
    # it is asked with plain sockets that serve nothing.
    port = free_port()
    w = hold(socket.AF_INET, "0.0.0.0", port)
    k6 = hold(socket.AF_INET6, "::1", port)
    out = subprocess.run([sys.executable, "-I", "-c", TRY, str(port)], capture_output=True, text=True).stdout
    print("   the kernel, 0.0.0.0:%d and [::1]:%d held (what --bind 0.0.0.0 holds): another program can take: %s"
          % (port, port, [" ".join(l.split()[:-1]) for l in out.splitlines() if l.endswith("BOUND") and "UDP" not in l] or "none"))
    w.close()
    k6.close()
    port = free_port()
    w = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
    w.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
    w.bind(("::", port))
    w.listen(8)
    out = subprocess.run([sys.executable, "-I", "-c", TRY, str(port)], capture_output=True, text=True).stdout
    print("   the kernel, a dual-stack [::]:%d held (what --bind [::] holds): another program can take: %s"
          % (port, [" ".join(l.split()[:-1]) for l in out.splitlines() if l.endswith("BOUND") and "UDP" not in l] or "none"))
    w.close()


def slow(root, procs):
    """The binds are now the first thing: what a browser meets while the
    rest of the start runs."""
    data, ws = os.path.join(root, "w-data"), os.path.join(root, "w-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    port = free_port()
    t0 = time.monotonic()
    p, _ = start(data, "127.0.0.1:%d" % port, wait=False)
    procs.append(p)
    first, answer = None, None
    k = None
    while time.monotonic() - t0 < 120:
        try:
            k = socket.create_connection(("::1", port), timeout=1)
            first = time.monotonic() - t0
            break
        except OSError:
            time.sleep(0.002)
    made = sorted(os.listdir(data))
    k.settimeout(120)
    k.sendall(b"GET / HTTP/1.1\r\nHost: localhost:%d\r\nConnection: close\r\n\r\n" % port)
    try:
        line = k.recv(200).split(b"\r\n")[0].decode()
    except OSError as e:
        line = "%s: %s" % (type(e).__name__, e)
    answer = time.monotonic() - t0
    k.close()
    p.wait_line("listening on http", secs=240)
    print("8. a first start (the data directory is made new): [::1]:%d takes a connection %.2f s after the start "
          "(the directory then holds %s); the request sent then is answered %.2f s after the start: %s"
          % (port, first, made, answer, line))
    p.signal(signal.SIGTERM)
    p.exited(60)
    # A start that fails after the binds: nothing keeps the port.
    with open(os.path.join(data, "config.toml"), "a") as f:
        f.write("\nthis is not toml =\n")
    p, _ = start(data, "127.0.0.1:%d" % port, wait=False)
    procs.append(p)
    code = p.exited(60)
    print("   a start that fails after the binds (a broken config.toml): exit %r; listening on the port afterwards: %s; it said: %s"
          % (code, listeners(port), short(" ".join(l.strip() for l in (p.log(4) or "").split("Error: ")[-1].splitlines()), 160)))


# Run by a command of a node: wait until [::1]:<port> can be taken, hold it
# (nothing is served or read) for some seconds, give it back.
WAIT = r'''
import socket, sys, time
port, secs = int(sys.argv[1]), float(sys.argv[2])
by = time.time() + 90
while time.time() < by:
    s = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
    try:
        s.bind(("::1", port)); s.listen(1)
        print("held", flush=True); time.sleep(secs); s.close(); print("given back", flush=True); break
    except OSError:
        s.close(); time.sleep(0.02)
'''


def down(root, procs):
    """The residual the fix's note names: while the server is down its port
    is anyone's. Who can be there: a command of a node that is not the
    server's own (a second node on the same machine keeps running when the
    server stops)."""
    data, ndata, ws = os.path.join(root, "d-data"), os.path.join(root, "d-node"), os.path.join(root, "d-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    config(ndata, ws)
    nport = free_port()
    listen = ["--node-listen", "127.0.0.1:%d" % nport]
    s = Server(data, listen)
    procs.append(s)
    port = int(s.addr.rsplit(":", 1)[1])
    n, node_id = remote_node(s, nport, ndata, root)
    procs.append(n)
    assert wait_for(lambda: connected(s, node_id), 30, 0.1), n.log(10)
    with open(os.path.join(ws, "wait.py"), "w") as f:
        f.write(WAIT)
    # The embedded node first: its commands end with the server.
    sid0 = s.session(ws)
    s.post("/api/sessions/%s/prompt" % sid0, script([fg("python3 wait.py %d 25" % port, 120)]))
    sid = s.session(ws, node=node_id)
    s.post("/api/sessions/%s/prompt" % sid, script([fg("python3 wait.py %d 25" % port, 120)]))
    s.wait_event(sid, "tool_call_started")
    s.wait_event(sid0, "tool_call_started")
    time.sleep(1.0)
    print("9. the server runs on %s; a command of its own node and one of a second node on this machine both wait for [::1]:%d: neither has it"
          % (listeners(port), port))
    s.signal(signal.SIGTERM)
    code = s.exited(100)
    got = wait_for(lambda: listeners(port), 10, 0.02)
    want = b"\0".join([b"python3", b"wait.py", str(port).encode(), b"25"]) + b"\0"
    time.sleep(1.0)
    waiting = [p for p in pids() if cmdline(p) == want and alive(p)]
    print("   the server is stopped (exit %r): listening on its port now: %s; of the two commands, still alive: %d (the second node's; the server's own node ended its command with it)"
          % (code, got, len(waiting)))
    p, _ = start(data, "127.0.0.1:%d" % port, listen, wait=False)
    procs.append(p)
    code = p.exited(60)
    print("   the server started again while the command holds it: exit %r: %s"
          % (code, short(" ".join(l.strip() for l in (p.log(3) or "").split("Error: ")[-1].splitlines()), 120)))
    assert wait_for(lambda: not listeners(port), 40, 0.1), "the command never gave the port back"
    p, got = start(data, "127.0.0.1:%d" % port, listen)
    procs.append(p)
    print("   the command gives it back (25 s): the next start holds %s" % listeners(port))
    p.signal(signal.SIGTERM)
    p.exited(100)
    n.signal(signal.SIGTERM)
    n.exited(100)


def netoff(root, procs):
    """Can a command take `::1` away from the machine, so that the server's
    second bind is "not available"? What it is, read only, the network off."""
    data, ws = os.path.join(root, "o-data"), os.path.join(root, "o-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws, node_extra='[sandbox]\nnetwork = "off"\n')
    s = Server(data)
    procs.append(s)
    port = int(s.addr.rsplit(":", 1)[1])
    with open(os.path.join(ws, "try.py"), "w") as f:
        f.write(TRY)
    sid = s.session(ws)
    s.post("/api/sessions/%s/prompt" % sid, script([
        fg("grep -E 'CapEff|CapBnd' /proc/self/status; readlink /proc/self/ns/net; readlink /proc/self/ns/user; "
           "cat /proc/sys/net/ipv6/conf/lo/disable_ipv6; test -w /proc/sys/net/ipv6/conf/lo/disable_ipv6 && echo WRITABLE || echo not-writable", 30),
        fg("python3 try.py %d" % port, 30)]))
    evs = s.wait_event(sid, "turn_ended", secs=90)
    res = results(evs)
    print("10. [sandbox] network = \"off\": a command is: %s" % " | ".join(l.strip() for l in res[0].splitlines() if l.strip() and not l.startswith("[exit")))
    print("    this script's own: %s; the server still holds %s" % (os.readlink("/proc/self/ns/net"), listeners(port)))
    print("    its tries at the port: %s" % sorted(set(l.split()[-1] for l in res[1].splitlines() if l.strip() and not l.startswith("[exit"))))
    s.signal(signal.SIGTERM)
    s.exited(60)


def prebound(root, procs):
    """A socket bound to [::1]:<port> with SO_REUSEADDR and not listening,
    there before the start (Linux lets a second SO_REUSEADDR socket bind
    beside it; tokio's listeners set SO_REUSEADDR)."""
    data, ws = os.path.join(root, "p-data"), os.path.join(root, "p-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    port = free_port()
    k = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
    k.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    k.bind(("::1", port))
    p, _ = start(data, "127.0.0.1:%d" % port, wait=False)
    procs.append(p)
    by = time.monotonic() + 120
    while time.monotonic() < by and p.p.poll() is None and not any("listening on http" in l for _, l in p.out):
        time.sleep(0.05)
    if p.p.poll() is not None:
        print("11. a bound, not listening SO_REUSEADDR socket on [::1]:%d before the start: the start is refused (exit %r)" % (port, p.p.returncode))
    else:
        try:
            k.listen(1)
            mine = "LISTENS"
        except OSError as e:
            mine = errno.errorcode.get(e.errno, e.errno)
        who = "the server's own page" if "<!doctype html" in get("http://[::1]:%d/" % port)[2].lower() else "NOT the server"
        print("11. a bound, not listening SO_REUSEADDR socket on [::1]:%d before the start: the server starts and listens on %s; "
              "that socket's listen() afterwards: %s; http://[::1]:%d/ is %s"
              % (port, listeners(port), mine, port, who))
        p.signal(signal.SIGTERM)
        p.exited(60)
        try:
            k.listen(1)
            after = "LISTENS (the port is free once the server is down)"
        except OSError as e:
            after = errno.errorcode.get(e.errno, e.errno)
        print("    once the server has stopped, the same socket's listen(): %s" % after)
    k.close()


def original(root, procs):
    """The proof of S2-1 as first run (`step2_defaults.py` part 4): a
    background command of the node serves on [::1]:<the server's port>."""
    data, ws = os.path.join(root, "g-data"), os.path.join(root, "g-ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws, node_extra="[limits]\nbackground_per_session = 8\n")
    s = Server(data)
    procs.append(s)
    port = int(s.addr.rsplit(":", 1)[1])
    with open(os.path.join(ws, "srv.py"), "w") as f:
        f.write("import http.server,socketserver,socket\n"
                "class H(http.server.BaseHTTPRequestHandler):\n"
                "    def do_GET(self):\n"
                "        self.send_response(200); self.end_headers(); self.wfile.write(b'another-program')\n"
                "class S(socketserver.TCPServer):\n"
                "    address_family = socket.AF_INET6\n"
                "    allow_reuse_address = True\n"
                "    allow_reuse_port = True\n"
                "S(('::1', %d), H).serve_forever()\n" % port)
    sid = s.session(ws)
    s.post("/api/sessions/%s/prompt" % sid, script([bg("exec python3 srv.py", 600)]))
    evs = s.wait_event(sid, "turn_ended", secs=60)
    last = [l for l in results(evs)[-1].splitlines() if l.strip()]
    body = get("http://localhost:%d/" % port)[2]
    print("0. the first proof again (a background command serves on [::1]:%d, SO_REUSEADDR and SO_REUSEPORT set): its result ends: %s"
          % (port, short(" / ".join(last[-3:]), 200)))
    print("   started as a background command: %s; listening on the port: %s; http://localhost:%d/ is answered by: %s"
          % (bg_account(evs)[0], listeners(port), port,
             "ANOTHER PROGRAM" if "another-program" in body else "the server" if "<!doctype html" in body.lower() else short(body, 40)))
    s.signal(signal.SIGTERM)
    s.exited(60)


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    root = scratch("loopback")
    procs = []
    try:
        for name, f in (("original", original), ("held", held), ("start", start_checks), ("stop", stop_checks), ("tls", tls), ("binds", binds), ("slow", slow),
                        ("down", down), ("netoff", netoff), ("prebound", prebound)):
            if which in (name, "all"):
                f(root, procs)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
