#!/usr/bin/env python3
"""RN-1: the revocation sweep and a demoted admin's running `host` turn.

`cancel_blocked` (master/src/workspaces.rs) asks `agentic_blocked` once per
(owner, workspace, node) and reuses the answer for every other chat of that
triple. Since L-h the answer also depends on the chat's own mode, so the
first row read (`ORDER BY updated_ms DESC`) decides for the others.

Dana is a server admin with a `host` chat and a `sandbox` chat in her
Personal workspace on the server's own node, both with a turn running a long
`sleep`. She is demoted to member (`local_node_users = "granted"`, so the
node rule does not stop her; only the mode rule does).

    alone        only the host chat exists: the sweep cancels its turn.
    sandbox-last the sandbox chat's last event is the newer one.
    host-last    the host chat's last event is the newer one.
"""
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from n10a_common import *

BASE = 978100


def case(name, order, marks):
    root = scratch(name)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws, node_extra=HOST_ON, cfg_extra=GRANTED)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        dana, dana_id = granted(alex, "dana")
        set_role(alex, dana_id, "admin")
        offered = modes(alex)["local"][0]
        assert offered == ["sandbox", "host"], "this machine's node offers %s" % offered
        sids = {}
        for i, mode in enumerate(order):
            c, v = new_chat(dana, ws, mode)
            assert c == 200, (c, v)
            sid = v["session_id"]
            sids[mode] = sid
            assert created(s, sid)["exec_mode"] == mode
            m = marks[mode]
            c, v = dana.call("POST", "/api/sessions/%s/prompt" % sid, script([fg("sleep %d" % m, 3000)]))
            assert c == 200, (c, v)
            if mode == "host":
                allow_cards(s, dana, sid)
            assert wait_for(lambda: sleeps(m), 30, 0.05), "sleep %d does not run: %s" % (m, kinds(s.events(sid)))
            time.sleep(1.2)  # the rows' updated_ms differ
        set_role(alex, dana_id, "member")
        time.sleep(6)
        out = []
        for mode in order:
            sid = sids[mode]
            evs = s.events(sid)
            c, v = dana.call("GET", "/api/sessions/%s" % sid)
            out.append("%s chat: turn %s, its sleep %s, blocked=%r"
                       % (mode, "ENDED (%s)" % ends(evs)[-1] if ends(evs) else "STILL RUNNING",
                          "alive" if sleeps(marks[mode]) else "gone", short(v.get("blocked") or "", 40)))
        print("%-13s started in the order %s; dana demoted, 6 s later:" % (name, order))
        for l in out:
            print("    " + l)
    finally:
        left = cleanup(procs, root, list(marks.values()))
        if left:
            print("    (cleanup ended %d leftover processes of this run)" % len(left))


def main():
    case("alone", ["host"], {"host": BASE})
    case("sandbox-last", ["host", "sandbox"], {"host": BASE + 1, "sandbox": BASE + 2})
    case("host-last", ["sandbox", "host"], {"host": BASE + 3, "sandbox": BASE + 4})


if __name__ == "__main__":
    main()
