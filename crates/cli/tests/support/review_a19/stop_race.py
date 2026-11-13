#!/usr/bin/env python3
"""A.19 review: prompts and approval answers racing the stop signal.

prompts   K idle sessions; K threads each send one prompt within 150 ms before to 20 ms after
          the SIGTERM. For every prompt: the answer, and
          after the next start the log. Looks for: answered 200 with nothing
          recorded; a turn the stopped process (exit 0) left open (closed
          `interrupted` by the next start); a turn recorded for a refused
          prompt.
approvals a turn waits for an approval; "allow" is sent within 150 ms before to 20 ms after
          hundred microseconds around the SIGTERM. Looks for: an approval
          recorded `allow` after the stop's log line with its command run.

    stop_race.py prompts [rounds] | approvals [tries]
"""
import os, random, signal, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def prompts(rounds, k=24):
    tally = {}
    bad = []
    for r in range(rounds):
        root = scratch("race%d" % r)
        data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
        os.makedirs(ws, exist_ok=True)
        write_config(data, ws)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            sids = [s.session(ws) for _ in range(k)]
            answers = {}
            start = time.monotonic() + 0.3
            def send(sid, at):
                time.sleep(max(0, at - time.monotonic()))
                answers[sid] = s.call("POST", "/api/sessions/%s/prompt" % sid, {"text": "slow: 40"})
            ts = [threading.Thread(target=send, args=(sid, start + random.uniform(-0.150, 0.020)))
                  for sid in sids]
            for t in ts:
                t.start()
            time.sleep(max(0, start - time.monotonic()))
            s.signal(signal.SIGTERM)
            for t in ts:
                t.join()
            code = s.exited(70)
            s2 = Server(data)
            procs.append(s2)
            for sid in sids:
                c = answers[sid][0]
                evs = s2.events(sid)
                e = ends(evs)
                started = sum(1 for x in evs if x["kind"] == "turn_started")
                key = (c, started, tuple(e))
                tally[key] = tally.get(key, 0) + 1
                if code == 0 and "interrupted" in e:
                    bad.append("exit 0 left a turn open: %s %s" % (c, kinds(evs)))
                if c == 200 and started == 0:
                    bad.append("answered 200, nothing recorded: %s" % sid)
                if c == 503 and started:
                    bad.append("refused, yet recorded: %s" % kinds(evs))
                if started != len(e):
                    bad.append("a turn without its end after the next start: %s" % kinds(evs))
            if code != 0:
                bad.append("round %d: exit status %r\n%s" % (r, code, s.log(4)))
            s2.signal(signal.SIGTERM)
            s2.exited(30)
        finally:
            cleanup(procs, needle=root)
    print("prompts sent within -150..+20 ms of SIGTERM, %d rounds of %d:" % (rounds, k))
    for (c, started, e), n in sorted(tally.items(), key=lambda x: str(x)):
        print("  answered %s, turns started %d, ends %s: %d" % (c, started, list(e), n))
    print("violations: %d" % len(bad))
    for b in bad[:10]:
        print("  " + b[:400])


def approvals(tries):
    tally = {}
    bad = []
    for i in range(tries):
        root = scratch("appr%d" % i)
        data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
        os.makedirs(ws, exist_ok=True)
        write_config(data, ws)
        procs = []
        try:
            s = Server(data)
            procs.append(s)
            sid = s.session(ws, mode="ask")
            s.post("/api/sessions/%s/prompt" % sid, {"text": "run: touch marker"})
            evs = s.wait_event(sid, "approval_requested")
            aid = [e for e in evs if e["kind"] == "approval_requested"][0]["payload"]["approval_id"]
            ans = {}
            start = time.monotonic() + 0.2
            # The answer takes about a millisecond to reach the node: sent
            # 0 to 3 ms before the signal.
            lead = random.uniform(0.0, 0.003)
            def allow():
                time.sleep(max(0, start - lead - time.monotonic()))
                ans["a"] = s.call("POST", "/api/sessions/%s/approvals/%s" % (sid, aid), {"decision": "allow"})
            t = threading.Thread(target=allow)
            t.start()
            time.sleep(max(0, start - time.monotonic()))
            s.signal(signal.SIGTERM)
            t.join()
            code = s.exited(70)
            stop_line = [l for l in s.log().split("\n") if "stopping the server" in l]
            stop_at = stop_line[0].split()[0] if stop_line else "?"
            s2 = Server(data)
            procs.append(s2)
            evs = s2.events(sid)
            res = [e for e in evs if e["kind"] == "approval_resolved"]
            dec = "%s by %s" % (res[0]["payload"]["decision"], res[0]["payload"]["by"]) if res else "none"
            ran = os.path.exists(os.path.join(ws, "marker"))
            key = (ans["a"][0], dec, ran, tuple(ends(evs)), code)
            tally[key] = tally.get(key, 0) + 1
            if res and res[0]["payload"]["decision"] != "deny":
                at = res[0].get("ts") or res[0].get("ts_ms") or res[0].get("time")
                if ran or True:
                    bad.append("allow recorded (event time %s, stop logged %s), command ran: %s, end %s"
                               % (at, stop_at, ran, ends(evs)))
            s2.signal(signal.SIGTERM)
            s2.exited(30)
        finally:
            cleanup(procs, needle=root)
    print("an `allow` sent 0..3 ms before SIGTERM, %d tries:" % tries)
    for (c, dec, ran, e, code), n in sorted(tally.items(), key=lambda x: str(x)):
        print("  answered %s; recorded: %s; the command ran: %s; ends %s; exit %s: %d" % (c, dec, ran, list(e), code, n))
    print("tries where `allow` was recorded (each to be read against the stop's time):")
    for b in bad[:12]:
        print("  " + b)


if sys.argv[1:2] == ["approvals"]:
    approvals(int(sys.argv[2]) if len(sys.argv) > 2 else 30)
else:
    prompts(int(sys.argv[2]) if len(sys.argv) > 2 else 6)
