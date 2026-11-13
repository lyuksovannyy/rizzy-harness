#!/usr/bin/env python3
"""T.8d step 3 review: two bounds of a callback turn that the branch states
and has no test for, tried on the real binary (its own node, bubblewrap, the
demo provider, an `auto` chat of a signed-in owner).

    refill     a command that restarts itself from its own output; while each
               callback turn runs, the owner sends "hello" and the node
               refuses it ("a turn is already running"), and the owner
               presses Stop on the second one. Does a refused message or a
               Stop give callback turns back? (It must not: only a recorded
               `user_message` does.)
    delegate   the command's output asks the agent to hand work to a
               sub-agent. Does a callback turn delegate? (It must not.) The
               same line as the owner's own message is the control.

Everything goes under <target>/review_t8d_step3/run-<pid>/; the commands poll
with `sleep 0.05`, no marker sleeps.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from step2_common import *

common.SCRATCH = os.path.join(common.TARGET, "review_t8d_step3", "run-%d" % os.getpid())


def config3(data, ws, obeys=True, mode="auto"):
    """Step 3 re-check: the demo provider obeys program output only with
    `mock_obeys_output = true` (the fix of S3-2); the key sits in the
    provider's table, before its models."""
    os.makedirs(data, exist_ok=True)
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write('workspace_roots = ["%s"]\nmax_approval_mode = "%s"\n' % (ws, mode))
    with open(os.path.join(data, "config.toml"), "w") as f:
        f.write('[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\n'
                'mock_delay_ms = 1\n%s\n[[providers.models]]\nid = "mock"\n'
                % ("mock_obeys_output = true\n" if obeys else ""))


HELD = "while [ ! -e go ]; do sleep 0.05; done; rm -f go; cat %s; exit 1"


def events(b, sid):
    c, v = b.call("GET", "/api/sessions/%s/events?limit=5000" % sid)
    assert c == 200, (c, v)
    return v["events"]


def until(b, sid, pred, secs=90):
    by = time.monotonic() + secs
    while True:
        evs = events(b, sid)
        if pred(evs):
            return evs
        assert time.monotonic() < by, "not seen: %s" % kinds(evs)
        time.sleep(0.05)


def n(evs, kind):
    return sum(1 for e in evs if e["kind"] == kind)


def callbacks(evs):
    return [e["payload"]["cause"]["callback"] for e in evs if e["kind"] == "turn_started" and e["payload"].get("cause")]


def chat(alex, d):
    os.makedirs(d, exist_ok=True)
    c, v = alex.call("POST", "/api/sessions", {"cwd": d, "approval_mode": "auto"})
    assert c == 200, (c, v)
    return v["session_id"]


def refill(alex, ws):
    d = os.path.join(ws, "refill")
    sid = chat(alex, d)
    again = "script: " + json.dumps([bg(HELD % "loop.txt", 120)])
    with open(os.path.join(d, "loop.txt"), "w") as f:
        f.write(again + "\n")
    c, v = alex.call("POST", "/api/sessions/%s/prompt" % sid, {"text": again})
    assert c == 200, (c, v)
    until(alex, sid, lambda e: n(e, "turn_ended") == 1)
    said = []
    for k in range(1, 4):
        until(alex, sid, lambda e: n(e, "background_started") >= k)
        open(os.path.join(d, "go"), "w").close()
        until(alex, sid, lambda e: len(callbacks(e)) == k)
        # The callback turn runs (its start's first wait is 3 s).
        c, v = alex.call("POST", "/api/sessions/%s/prompt" % sid, {"text": "hello"})
        said.append("%s %s" % (c, short(v, 50)))
        if k == 2:
            until(alex, sid, lambda e: n(e, "background_started") >= 3)
            c, v = alex.call("POST", "/api/sessions/%s/cancel" % sid, {})
            said.append("Stop: %s" % c)
        until(alex, sid, lambda e: n(e, "turn_ended") == k + 1)
    until(alex, sid, lambda e: n(e, "background_started") >= 4)
    open(os.path.join(d, "go"), "w").close()
    evs = until(alex, sid, lambda e: n(e, "callback_refused") >= 1 or len(callbacks(e)) >= 4, secs=60)
    time.sleep(3.0)
    evs = events(alex, sid)
    print("== refill ==")
    print("    the owner while each callback turn ran: %s" % said)
    print("    user_message events: %d; callback turns: %s; turn ends: %s"
          % (n(evs, "user_message"), callbacks(evs), ends(evs)))
    print("    callback_refused: %s" % [(p["bg_ids"], p["reason"]) for p in of(evs, "callback_refused")])
    return len(callbacks(evs)) == 3 and n(evs, "callback_refused") == 1


def delegate(alex, ws):
    line = "delegate: scout=say the word pineapple"
    # Control: the owner's own message.
    d0 = os.path.join(ws, "delegate-own")
    own = chat(alex, d0)
    c, v = alex.call("POST", "/api/sessions/%s/prompt" % own, {"text": line})
    assert c == 200, (c, v)
    evs0 = until(alex, own, lambda e: n(e, "turn_ended") == 1, secs=120)
    # The same line as program output.
    d = os.path.join(ws, "delegate-output")
    sid = chat(alex, d)
    with open(os.path.join(d, "out"), "w") as f:
        f.write(line + "\n")
    c, v = alex.call("POST", "/api/sessions/%s/prompt" % sid, script([bg(HELD % "out", 120)]))
    assert c == 200, (c, v)
    until(alex, sid, lambda e: n(e, "turn_ended") == 1)
    open(os.path.join(d, "go"), "w").close()
    evs = until(alex, sid, lambda e: len(callbacks(e)) == 1 and n(e, "turn_ended") == 2, secs=120)
    at = [i for i, e in enumerate(evs) if e["kind"] == "turn_started" and e["payload"].get("cause")][0]
    c, lst = alex.call("GET", "/api/sessions")
    kids = lambda parent: [r["session_id"] for r in lst.get("sessions", []) if r.get("parent_id") == parent]
    res = lambda es: [(e["payload"].get("ok"), short(e["payload"].get("output"), 70)) for e in es if e["kind"] == "tool_call_finished"]
    print("== delegate ==")
    print("    the owner's own message: results %s; sub-agent chats %d" % (res(evs0), len(kids(own))))
    print("    the same line printed by the command: the callback turn's results %s; sub-agent chats %d"
          % (res(evs[at:]), len(kids(sid))))
    return not kids(sid) and all(not ok for ok, _ in res(evs[at:]))


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    root = scratch("bounds")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config3(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        out = {}
        if which in ("refill", "all"):
            out["a refused message and a Stop give no callback turns back"] = refill(alex, ws)
        if which in ("delegate", "all"):
            out["a callback turn does not delegate"] = delegate(alex, ws)
        print("RESULT (True: the bound held): %s" % out)
        s.signal(signal.SIGTERM)
        print("the server's exit status: %r" % s.exited(60))
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))


main()
