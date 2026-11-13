#!/usr/bin/env python3
"""T.8d step 2 review: what a steered agent can leave running on a node
with the defaults, now that background commands are on (8 a chat, 64 a
node), and what still refuses a start. The real binary, its own node.

1. node.toml names only the folder: the node's own default mode (`ask`).
2. The same node with `max_approval_mode = "auto"` (the owner's choice).
3. `[sandbox] contain = "off"`, a bubblewrap that is not there, and
   `background_per_session = 0`.
4. A listener of the agent on the master's own port, on the other loopback
   address (the master listens on 127.0.0.1 only).
"""
import os, signal, socket, subprocess, sys, time, urllib.request
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from step2_common import *

BASE = 979500


def pending(s, sid, seen, secs=20):
    def f():
        for p in of(s.events(sid), "approval_requested"):
            if p["approval_id"] not in seen:
                return p
    p = wait_for(f, secs)
    if p:
        seen.add(p["approval_id"])
    return p


def answer(s, sid, p, decision):
    s.post("/api/sessions/%s/approvals/%s" % (sid, p["approval_id"]), {"decision": decision})


def listeners(port):
    out = subprocess.run(["ss", "-Hltn", "sport = :%d" % port], capture_output=True, text=True).stdout
    return sorted(set(l.split()[3] for l in out.splitlines() if l.split()))


def get(url, timeout=3):
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return r.status, r.read(200).decode(errors="replace")
    except Exception as e:
        return 0, "%s: %s" % (type(e).__name__, e)


def node_toml(data, ws, extra):
    os.makedirs(data, exist_ok=True)
    config(data, ws)
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write('workspace_roots = ["%s"]\n%s' % (ws, extra))


def main():
    root = scratch("defaults")
    ws = os.path.join(root, "ws")
    os.makedirs(os.path.join(ws, "empty"), exist_ok=True)
    procs, marks = [], [BASE + i for i in range(40)]
    try:
        # 1. the node's defaults
        d1 = os.path.join(root, "d1")
        node_toml(d1, ws, "")
        s = Server(d1)
        procs.append(s)
        for l in [l for _, l in s.err if "shell sandbox" in l or "shell containment" in l]:
            print("   " + l.split("rizzy_harness: ")[-1][:150])
        c, v = s.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        print("1. node.toml with the folder only. A chat asked for in auto: %s %s" % (c, short(v, 160)))
        c, v = s.call("POST", "/api/sessions", {"cwd": ws})
        sid = v["session_id"]
        c, v = s.call("GET", "/api/sessions/%s" % sid)
        print("   a chat with no mode named runs in: %s" % (v.get("session") or v).get("approval_mode"))
        seen = set()
        s.post("/api/sessions/%s/prompt" % sid, script([
            fg("sleep %d" % BASE, 5), bg("sleep %d" % (BASE + 1), 99999), bg("sleep %d" % (BASE + 1), 99999)]))
        p = pending(s, sid, seen)
        print("   card 1 (a foreground bash): offers 'for this turn': %s; answered allow_session" % p.get("turn_allow"))
        answer(s, sid, p, "allow_session")
        p = pending(s, sid, seen)
        print("   card 2 (a background start, after that allowance): %s" % (short(p["reason"], 260) if p else "NO CARD"))
        print("   nothing runs before the click: %s; 'for this turn' offered: %s" % (not sleeps(BASE + 1), p and p.get("turn_allow")))
        if p:
            answer(s, sid, p, "allow")
        p = pending(s, sid, seen)
        print("   the identical start again asks again: %s" % bool(p))
        if p:
            answer(s, sid, p, "deny")
        s.wait_event(sid, "turn_ended", secs=60)
        st, en = bg_account(s.events(sid))
        e = of(s.events(sid), "background_started")
        print("   started %s with timeout_s %s (asked 99999); running: %s" % (st, [x["timeout_s"] for x in e], bool(sleeps(BASE + 1))))
        c, v = s.call("GET", "/api/sessions")
        rows = v.get("sessions", v)
        row = [r for r in rows if r.get("session_id") == sid][0]
        print("   the chat's row in the list of chats (what the sidebar has): running=%s; fields about background commands: %s"
              % (row.get("running"), [k for k in row if "background" in k or k.startswith("bg")]))
        c, v = s.call("GET", "/api/attention")
        print("   GET /api/attention: %s %s" % (c, short(v, 120)))
        s.call("POST", "/api/sessions/%s/background/stop" % sid, {})
        gone_within(lambda: sleeps(BASE + 1), 5)
        s.signal(signal.SIGTERM)
        s.exited(40)

        # 2. auto
        d2 = os.path.join(root, "d2")
        node_toml(d2, ws, 'max_approval_mode = "auto"\n')
        s = Server(d2)
        procs.append(s)
        sid = s.session(ws, mode="auto")
        port = free_port()
        s.post("/api/sessions/%s/prompt" % sid, script(
            [bg("cd empty && exec python3 -m http.server %d" % port, 99999)]
            + [bg("sleep %d" % (BASE + 10 + i), 3600) for i in range(8)]))
        evs = s.wait_event(sid, "turn_ended", secs=120)
        st, en = bg_account(evs)
        print("2. max_approval_mode = \"auto\", an auto chat, nine starts in one turn, no click: cards %d; started %s"
              % (len(of(evs, "approval_requested")), st))
        print("   the ninth: %s" % short(results(evs)[-1], 130))
        print("   the listener: bound at %s; answers another process of this machine: %s; its timeout: %s s"
              % (listeners(port), get("http://127.0.0.1:%d/" % port)[0], of(evs, "background_started")[0]["timeout_s"]))
        c, v = s.call("POST", "/api/sessions/%s/cancel" % sid, {})
        time.sleep(1.0)
        print("   the turn is over; the chat's Stop (cancel) answers %s and the listener still answers: %s" % (c, get("http://127.0.0.1:%d/" % port)[0]))
        # A second chat fills more of the node.
        sid2 = s.session(ws, mode="auto")
        s.post("/api/sessions/%s/prompt" % sid2, script([bg("sleep %d" % (BASE + 20 + i), 3600) for i in range(8)]))
        evs2 = s.wait_event(sid2, "turn_ended", secs=120)
        print("   a second chat of the same person: started %d more (the node's cap is 64: eight chats fill it)" % len(bg_account(evs2)[0]))
        t0 = time.monotonic()
        a = s.call("POST", "/api/sessions/%s/background/stop" % sid, {})
        b = s.call("POST", "/api/sessions/%s/background/stop" % sid2, {})
        left = gone_within(lambda: [p for i in range(10, 30) for p in sleeps(BASE + i)], 10)
        print("   Stop all in both: %s %s; left %d after %.1f s; the listener answers: %s"
              % (a[0], b[0], len(left), time.monotonic() - t0, get("http://127.0.0.1:%d/" % port, 1)[0]))

        # 4. the master's own port on the other loopback address
        mport = int(s.addr.rsplit(":", 1)[1])
        body = "agent-page"
        srv = ("import http.server,socketserver,socket,sys\n"
               "class H(http.server.BaseHTTPRequestHandler):\n"
               "    def do_GET(self):\n"
               "        sys.stderr.write('COOKIE-SEEN:%%s\\n' %% bool(self.headers.get('Cookie')))\n"
               "        self.send_response(200); self.send_header('Content-Type','text/html'); self.end_headers(); self.wfile.write(b'%s')\n"
               "class S(socketserver.TCPServer):\n"
               "    address_family = socket.AF_INET6\n"
               "    allow_reuse_address = True\n"
               "S(('::1', %d), H).serve_forever()\n" % (body, mport))
        with open(os.path.join(ws, "srv.py"), "w") as f:
            f.write(srv)
        sid3 = s.session(ws, mode="auto")
        print("4. the master listens at %s; before: http://localhost:%d/api/setup/status -> %s"
              % (listeners(mport), mport, short(get("http://localhost:%d/api/setup/status" % mport)[1], 40)))
        s.post("/api/sessions/%s/prompt" % sid3, script([bg("exec python3 srv.py", 3600)]))
        evs3 = s.wait_event(sid3, "turn_ended", secs=60)
        print("   the agent's background command binds [::1]:%d: %s; listeners on that port now: %s"
              % (mport, short(results(evs3)[-1], 90).replace("\n", " "), listeners(mport)))
        a = get("http://localhost:%d/api/setup/status" % mport)
        b = get("http://127.0.0.1:%d/api/setup/status" % mport)
        print("   http://localhost:%d/… is now answered by: %s (http://127.0.0.1:%d/… by: %s)"
              % (mport, "THE AGENT'S COMMAND" if a[1] == body else "the master" if "claimed" in a[1] else a, mport,
                 "the master" if "claimed" in b[1] else b))
        hosts = raw("127.0.0.1:%d" % mport, "GET", "/api/setup/status", {"Host": "localhost:%d" % mport})
        print("   the master takes Host: localhost:%d (it is one of its allowed origins): %s" % (mport, hosts[0]))
        s.call("POST", "/api/sessions/%s/background/stop" % sid3, {})
        time.sleep(1.0)
        print("   after its Stop: listeners %s" % listeners(mport))
        s.signal(signal.SIGTERM)
        s.exited(40)

        # 3. what still refuses
        for name, extra in (("[sandbox] contain = \"off\"", 'max_approval_mode = "auto"\n[sandbox]\ncontain = "off"\n'),
                            ("[sandbox] bwrap = a path that is not there", 'max_approval_mode = "auto"\n[sandbox]\nbwrap = "%s/no-bwrap"\n' % root),
                            ("[limits] background_per_session = 0", 'max_approval_mode = "auto"\n[limits]\nbackground_per_session = 0\n'),
                            ("[sandbox] network = \"off\"", 'max_approval_mode = "auto"\n[sandbox]\nnetwork = "off"\n')):
            d3 = os.path.join(root, "d3-%d" % abs(hash(name)))
            node_toml(d3, ws, extra)
            try:
                s = Server(d3)
            except RuntimeError as e:
                print("3. %s: the server does not start: %s" % (name, str(e).splitlines()[-1][-160:]))
                continue
            procs.append(s)
            line = [l for _, l in s.err if "shell containment" in l]
            sid = s.session(ws, mode="auto")
            port = free_port()
            cmd = "sleep %d" % (BASE + 30) if "network" not in name else "cd empty && exec python3 -m http.server %d" % port
            s.post("/api/sessions/%s/prompt" % sid, script([bg(cmd, 600), {"tool": "bash_output", "input": {}}]))
            evs = s.wait_event(sid, "turn_ended", secs=60)
            res = results(evs)
            print("3. %s (%s):" % (name, line[0].split("shell containment: ")[-1][:60] if line else "?"))
            print("   the start: %s" % short(res[0].replace("\n", " "), 230))
            print("   started %s; processes %d%s" % (bg_account(evs)[0], len(sleeps(BASE + 30)),
                                                   "; listeners on its port: %s" % listeners(port) if "network" in name else ""))
            s.call("POST", "/api/sessions/%s/background/stop" % sid, {})
            s.signal(signal.SIGTERM)
            s.exited(40)
    finally:
        k = cleanup(procs, needle=root, secs=marks)
        print("cleanup killed: %s" % (k or "nothing"))

main()
