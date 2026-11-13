#!/usr/bin/env python3
"""A.19 re-check: the debug build's `HARNESS_STOP_WAIT_S` (`cli/src/stop.rs`
`stop_wait`), the tests' way to shorten the stop's wait. A release build
does not read it (`#[cfg(debug_assertions)]`; the image is built with
`--release`); `cargo run` and `target/debug/rizzy-harness` do, from the
environment or from `.env` in the folder `serve` is started in.

One running turn (its command ignores SIGTERM, so closing it takes 2 s),
then SIGTERM, with the variable unset, 0, and the largest number it parses.
"""
import os, signal, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

N = 983000 + os.getpid() % 900


def case(i, value, dotenv=False):
    root = scratch("seam%d" % i)
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    env = {}
    if value is not None and dotenv:
        with open(os.path.join(root, ".env"), "w") as f:      # serve's start folder
            f.write("HARNESS_STOP_WAIT_S=%s\n" % value)
        os.chmod(os.path.join(root, ".env"), 0o600)
    elif value is not None:
        env["HARNESS_STOP_WAIT_S"] = value
    procs = []
    try:
        s = Server(data, env=env)
        procs.append(s)
        sid = s.session(ws)
        s.post("/api/sessions/%s/prompt" % sid, {"text": "run: trap '' TERM; sleep %d" % (N + i)})
        assert wait_for(lambda: sleeps(N + i)), "the command never started"
        t0 = time.monotonic()
        s.signal(signal.SIGTERM)
        code = s.exited(100)
        took = time.monotonic() - t0
        last = [l.split("rizzy_harness")[-1][:150] for l in s.log(3).split("\n")]
        panicked = [l for l in s.log().split("\n") if "panicked" in l]
        s2 = Server(data)
        procs.append(s2)
        e = ends(s2.events(sid))
        s2.signal(signal.SIGTERM)
        s2.exited(100)
        print("HARNESS_STOP_WAIT_S %s%s: exit status %r in %.1f s; the turn at the next start: %s"
              % ("unset" if value is None else "= " + value, " (in .env)" if dotenv else "", code, took, e))
        for l in panicked[:1]:
            print("    " + l[:200])
        print("    last line: %s" % last[-1].strip())
    finally:
        cleanup(procs, needle=root, secs=[N + i])


case(0, None)
case(1, "0")
case(2, "18446744073709551615")
case(3, "0", dotenv=True)
