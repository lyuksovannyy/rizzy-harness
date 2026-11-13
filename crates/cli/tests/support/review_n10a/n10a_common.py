"""N.10a part 1, delta re-check of the review's fixes: what the proof scripts
share. On top of review_t8d's `common.py` / `step2_common.py` (used as they
are). The binary under test is `RIZZY` (required: the pinned hard link under
<target>/review_n10a/); everything a script makes goes under
<target>/review_n10a/run-<pid>/, never /tmp; servers listen on a port the
kernel picks; marker sleeps are `sleep 978xxx` / `sleep 979xxx`; a signal goes
only to a pid the script started.
"""
import os, sys

if not os.environ.get("RIZZY"):
    sys.exit("set RIZZY=<target>/review_n10a/rizzy-harness (the pinned binary)")
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "review_t8d"))
import common
from step2_common import *

common.SCRATCH = os.path.join(common.TARGET, "review_n10a", "run-%d" % os.getpid())

HOST_ON = "[execution]\nhost = true\n"
GRANTED = '\n[nodes]\nlocal_node_users = "granted"\n'


def set_role(admin, uid, role):
    c, v = admin.call("PATCH", "/api/users/%s" % uid, {"role": role})
    assert c == 200, (c, v)


def modes(b, node="local"):
    c, v = b.call("GET", "/api/nodes")
    assert c == 200, (c, v)
    return {n["node_id"]: (n.get("exec_modes"), n.get("exec_gaps")) for n in v["nodes"]}


def new_chat(b, ws, mode=None, approval="auto", **more):
    body = {"cwd": ws, "approval_mode": approval}
    if mode:
        body["exec_mode"] = mode
    body.update(more)
    return b.call("POST", "/api/sessions", body)


def created(s, sid):
    evs = s.wait_event(sid, "session_created", 30)
    return [e["payload"] for e in evs if e["kind"] == "session_created"][0]


def allow_cards(s, b, sid, want=1, secs=30, decision="allow"):
    """Answer `want` waiting approval cards of `sid` as `b`; the reasons shown."""
    import time
    done, reasons = set(), []
    by = time.monotonic() + secs
    while len(done) < want and time.monotonic() < by:
        evs = s.events(sid)
        resolved = {e["payload"].get("approval_id") for e in evs if e["kind"] == "approval_resolved"}
        for e in evs:
            if (e["kind"] == "approval_requested" and e["payload"]["approval_id"] not in done
                    and e["payload"]["approval_id"] not in resolved):
                aid = e["payload"]["approval_id"]
                c, v = b.call("POST", "/api/sessions/%s/approvals/%s" % (sid, aid), {"decision": decision})
                assert c == 200, (c, v)
                done.add(aid)
                reasons.append(e["payload"]["reason"])
        time.sleep(0.05)
    return reasons
