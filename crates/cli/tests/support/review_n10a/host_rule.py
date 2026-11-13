#!/usr/bin/env python3
"""H1, L-h, L-i, N-d, N-e on the real binary: who gets a `host` chat, what a
demoted admin can still do with one, what a host chat's approvals are, and
which requests for `host` reach the audit log.

One server, its own node with `[execution] host = true`, `max_approval_mode =
"auto"`, `[nodes] local_node_users = "granted"` (so the node rule is not what
refuses a member). alex: server admin. bob: member with the grant. wanda:
member with the grant, admin of alex's team. nora: member without the grant.
dana: admin, later demoted.
"""
import os, sqlite3, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from n10a_common import *

M = 978200


def audit(data):
    c = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
    try:
        return [r[0] for r in c.execute("select detail from audit where kind = 'session.create.host' order by rowid")]
    finally:
        c.close()


def main():
    root = scratch("host_rule")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws, node_extra=HOST_ON, cfg_extra=GRANTED)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        bob, bob_id = granted(alex, "bob")
        wanda, wanda_id = granted(alex, "wanda")
        nora = alex.member("nora")
        dana, dana_id = granted(alex, "dana")
        set_role(alex, dana_id, "admin")
        assert modes(alex)["local"][0] == ["sandbox", "host"], modes(alex)
        c, w = alex.call("POST", "/api/workspaces", {"name": "Team"})
        team = w.get("workspace_id")
        c, v = alex.call("POST", "/api/workspaces/%s/members" % team, {"username": "wanda", "role": "admin"})
        assert c == 200, (c, v)
        c, w = bob.call("POST", "/api/workspaces", {"name": "Bobs"})
        bobs = w.get("workspace_id")
        c, v = bob.call("GET", "/api/workspaces")
        bob_personal = v["workspaces"][0]["workspace_id"]

        print("== H1: who is given a host chat (POST /api/sessions, exec_mode host) ==")
        asked = 0
        for name, who, more in [
            ("bob, Personal (active)", bob, {}),
            ("bob, Personal by id", bob, {"workspace_id": bob_personal}),
            ("bob, a team he made and owns", bob, {"workspace_id": bobs}),
            ("bob, node named: local", bob, {"node_id": "local"}),
            ("wanda, admin of alex's team", wanda, {"workspace_id": team}),
            ("nora, no agentic grant", nora, {}),
            ("bob, profile chat", bob, {"profile": "chat"}),
        ]:
            c, v = new_chat(who, ws, "host", **more)
            asked += 1
            print("  %-32s %s %s   audit lines so far: %d of %d asked"
                  % (name, c, short(v, 70), len(audit(data)), asked))
        c, v = s.call("POST", "/api/sessions", {"cwd": ws, "exec_mode": "host", "approval_mode": "auto"})
        asked += 1
        tok = v.get("session_id")
        print("  %-32s %s %s" % ("the API token", c, short(v, 60)))
        if tok:
            p = created(s, tok)
            print("      recorded: exec_mode=%s approval_mode=%s" % (p["exec_mode"], p["approval_mode"]))
        print("  the audit lines (%d for %d requests):" % (len(audit(data)), asked))
        for l in audit(data):
            print("     ", l)

        print("== L-i: /api/nodes for nora (no grant) and bob ==")
        print("  nora:", modes(nora), " bob:", modes(bob))

        print("== N-e: dana (admin) asks for a host chat in auto ==")
        c, v = new_chat(dana, ws, "host", approval="auto")
        assert c == 200, (c, v)
        h = v["session_id"]
        p = created(s, h)
        print("  recorded: exec_mode=%s approval_mode=%s" % (p["exec_mode"], p["approval_mode"]))
        cmd = "echo ran >> %s/host.txt" % ws
        c, v = dana.call("POST", "/api/sessions/%s/prompt" % h, script([fg(cmd), fg(cmd), fg(cmd + " #2")]))
        assert c == 200, (c, v)
        r = allow_cards(s, dana, h, want=1, decision="allow_session")
        print("  card 1: %r -> answered allow_session (\"for this turn\")" % short(r[0] if r else "", 110))
        time.sleep(3)
        evs = s.events(h)
        cards = [e for e in evs if e["kind"] == "approval_requested"]
        lines = open(os.path.join(ws, "host.txt")).read().count("ran") if os.path.exists(os.path.join(ws, "host.txt")) else 0
        print("  after 3 s: %d cards asked, the command ran %d times (the identical 2nd call needs no card; the 3rd, different, waits)"
              % (len(cards), lines))
        allow_cards(s, dana, h, want=1, secs=5, decision="deny")
        s.wait_event(h, "turn_ended", 30)

        print("== L-h: dana is demoted; her host chat ==")
        set_role(alex, dana_id, "member")
        time.sleep(1)
        for name, method, path, body in [
            ("prompt", "POST", "/api/sessions/%s/prompt" % h, {"text": "hello"}),
            ("approval allow", "POST", "/api/sessions/%s/approvals/x" % h, {"decision": "allow"}),
            ("approval deny", "POST", "/api/sessions/%s/approvals/x" % h, {"decision": "deny"}),
            ("restore", "POST", "/api/sessions/%s/turns/x/restore" % h, {}),
            ("save as artifact", "POST", "/api/sessions/%s/artifacts" % h, {"path": "host.txt"}),
            ("set agent", "POST", "/api/sessions/%s/agent" % h, {"agent_id": "coder"}),
            ("select variant", "POST", "/api/sessions/%s/variants" % h, {"group": "g", "message_id": "m"}),
        ]:
            c, v = dana.call(method, path, body)
            print("  %-17s %s %s" % (name, c, short(v, 90)))
        c, v = s.call("POST", "/api/sessions/%s/prompt" % h, {"text": "hello"})
        print("  the API token prompts dana's host chat: %s %s" % (c, short(v, 80)))
        c, v = new_chat(dana, ws, "host")
        print("  dana asks for a new host chat: %s %s" % (c, short(v, 60)))
    finally:
        left = cleanup(procs, root, [M])
        if left:
            print("  (cleanup ended %d leftover processes of this run)" % len(left))


if __name__ == "__main__":
    main()
