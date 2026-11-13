"""T.8d step 2 review: what the step 2 proof scripts share, on top of
`common.py` (step 1's). Everything a script makes goes under
`<target>/review_t8d_step2/run-<pid>/`, never /tmp; the servers listen on a
port the kernel picks; markers are `sleep 979xxx`. A signal goes only to a
pid the script started.
"""
import http.client, json, os, socket, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from common import *

common.SCRATCH = os.path.join(common.TARGET, "review_t8d_step2", "run-%d" % os.getpid())
SCRATCH = common.SCRATCH

# The node's defaults (step 2: background commands on, 8 a chat): nothing
# under [limits].
DEFAULTS = ""


def config(data, ws, node_extra=DEFAULTS, cfg_extra="", mode="auto", delay_ms=1):
    os.makedirs(data, exist_ok=True)
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write('workspace_roots = ["%s"]\nmax_approval_mode = "%s"\n%s' % (ws, mode, node_extra))
    with open(os.path.join(data, "config.toml"), "w") as f:
        f.write('[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\n'
                'mock_delay_ms = %d\n\n[[providers.models]]\nid = "mock"\n%s' % (delay_ms, cfg_extra))


def free_port():
    with socket.socket() as k:
        k.bind(("127.0.0.1", 0))
        return k.getsockname()[1]


def raw(addr, method, path, headers=None, body=None, timeout=30.0):
    """One request with exactly these headers (http.client adds Host and
    Accept-Encoding only): (status, body text, headers, seconds)."""
    host, port = addr.rsplit(":", 1)
    c = http.client.HTTPConnection(host, int(port), timeout=timeout)
    h = dict(headers or {})
    data = None
    if body is not None:
        data = json.dumps(body).encode() if not isinstance(body, bytes) else body
        h.setdefault("Content-Type", "application/json")
    t0 = time.perf_counter()
    try:
        c.putrequest(method, path, skip_host="Host" in h, skip_accept_encoding=True)
        for k, v in h.items():
            c.putheader(k, v)
        if data is not None:
            c.putheader("Content-Length", str(len(data)))
        c.endheaders(data)
        r = c.getresponse()
        text = r.read().decode(errors="replace")
        return r.status, text, dict(r.getheaders()), time.perf_counter() - t0
    except Exception as e:
        return 0, "%s: %s" % (type(e).__name__, e), {}, time.perf_counter() - t0
    finally:
        c.close()


def page(b, **over):
    """The headers a signed-in page sends, with some replaced or (None) left out."""
    h = {"Cookie": b.cookie, "x-harness-login": b.key, "Origin": "http://" + b.s.addr}
    for k, v in over.items():
        k = k.replace("_", "-")
        if v is None:
            h.pop(k, None)
            h.pop(k.title(), None)
            for have in list(h):
                if have.lower() == k.lower():
                    h.pop(have)
        else:
            for have in list(h):
                if have.lower() == k.lower():
                    h.pop(have)
            h[k] = v
    return h


def user_id(admin, name):
    c, users = admin.call("GET", "/api/users")
    ulist = users["users"] if isinstance(users, dict) else users
    return [u for u in ulist if u["username"] == name][0]["user_id"]


def granted(admin, name):
    """A member with the agentic grant, signed in."""
    b = admin.member(name)
    uid = user_id(admin, name)
    c, v = admin.call("PATCH", "/api/users/%s" % uid, {"can_agentic": True})
    assert c == 200, (c, v)
    return b, uid


def remote_node(s, port, ndata, root):
    """Join and run a second node of server `s` (started with --node-listen on `port`)."""
    line = s.wait_line("nodes connect to wss://")
    fp = line.split("(certificate ")[1].rstrip(")").strip()
    token = s.post("/api/admin/nodes/join", {})["token"]
    env = dict(os.environ, HARNESS_JOIN_TOKEN=token)
    j = subprocess.run([EXE, "--data-dir", ndata, "node", "join", "wss://127.0.0.1:%d" % port,
                        "--fingerprint", fp], env=env, cwd=root, capture_output=True)
    assert j.returncode == 0, j.stderr
    node_id = [l.split('"')[1] for l in open(os.path.join(ndata, "remote.toml")) if l.startswith("node_id")][0]
    n = Proc(ndata, ["node", "run"])
    n.wait_line("connecting to")
    return n, node_id


def connected(s, node_id):
    c, v = s.call("GET", "/api/nodes")
    return c == 200 and any(n.get("node_id") == node_id and n.get("connected") for n in v.get("nodes", []))


def short(v, n=110):
    t = v if isinstance(v, str) else json.dumps(v)
    return t if len(t) <= n else t[:n] + "…"
