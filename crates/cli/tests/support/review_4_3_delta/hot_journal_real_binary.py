#!/usr/bin/env python3
"""Fix plan 4.3, delta re-check (finding D1): the real binary on a data
directory whose master.db was interrupted during its first creation.

  hot_journal_real_binary.py <path to rizzy-harness> [attempts]

Part A: a crash image made with sqlite3 (migration 1 inside an open
        transaction with a small page cache, files copied before the
        commit), then `serve` three times.
Part B: the real thing: `serve` on an empty directory, SIGKILL (by pid,
        this script's own child) at the moment master.db has pages and its
        journal still exists, then `serve` again on what is left.

Binds 127.0.0.1:0 only. Work folders: target/tmp/review_4_3_delta/bin
(never /tmp). Nothing is left running.
"""
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import time

BIN = os.path.realpath(sys.argv[1])
ATTEMPTS = int(sys.argv[2]) if len(sys.argv) > 2 else 40
HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, "..", "..", "..", "..", ".."))
WORK = os.path.join(ROOT, "target", "tmp", "review_4_3_delta", "bin")
ENV = {k: v for k, v in os.environ.items() if not k.startswith("HARNESS_")}

shutil.rmtree(WORK, ignore_errors=True)
os.makedirs(os.path.join(WORK, "cwd"))


def listing(d):
    return sorted("%s (%d)" % (n, os.path.getsize(os.path.join(d, n))) for n in os.listdir(d))


def serve(d, tag):
    """Run serve until it exits (a refusal) or 25 s pass (it started)."""
    p = subprocess.Popen(
        [BIN, "--data-dir", d, "serve", "--bind", "127.0.0.1:0"],
        cwd=os.path.join(WORK, "cwd"), env=ENV, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    try:
        _, err = p.communicate(timeout=25)
        code = p.returncode
    except subprocess.TimeoutExpired:
        p.send_signal(signal.SIGINT)
        try:
            _, err = p.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            p.kill()
            _, err = p.communicate()
        code = "started (stopped by this script after 25 s)"
    last = [l for l in err.splitlines() if l.strip()][-2:]
    print("  %s: exit %s" % (tag, code))
    for l in last:
        print("      " + l[:700])
    return code


print("=== Part A: crash image made with sqlite3")
live = os.path.join(WORK, "live")
os.makedirs(live)
schema = open(os.path.join(ROOT, "crates", "store", "schema", "1.sql")).read()
c = sqlite3.connect(os.path.join(live, "master.db"), isolation_level=None)
c.executescript("PRAGMA cache_size = 10; BEGIN IMMEDIATE;\n" + schema)
a = os.path.join(WORK, "a")
os.makedirs(a, mode=0o700)
for n in ("master.db", "master.db-journal"):
    shutil.copy(os.path.join(live, n), os.path.join(a, n))
c.execute("ROLLBACK")
c.close()
print("  image:", listing(a))
for i in (1, 2, 3):
    serve(a, "start %d" % i)
print("  after:", listing(a))

print("=== Part B: SIGKILL of the real binary during the first creation of master.db")
caught = None
for attempt in range(1, ATTEMPTS + 1):
    d = os.path.join(WORK, "b%02d" % attempt)
    os.makedirs(d, mode=0o700)
    db, journal = os.path.join(d, "master.db"), os.path.join(d, "master.db-journal")
    p = subprocess.Popen(
        [BIN, "--data-dir", d, "serve", "--bind", "127.0.0.1:0"],
        cwd=os.path.join(WORK, "cwd"), env=ENV, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 30
    state = "never saw master.db"
    while time.monotonic() < deadline and p.poll() is None:
        try:
            size = os.stat(db).st_size
        except FileNotFoundError:
            continue
        has_journal = os.path.exists(journal)
        if size > 0 and has_journal:
            os.kill(p.pid, signal.SIGKILL)
            state = "killed with pages in master.db and its journal present"
            break
        if size > 0 and not has_journal:
            state = "missed (commit already finished)"
            break
    if p.poll() is None:
        os.kill(p.pid, signal.SIGKILL)
    p.wait()
    left = listing(d)
    if state.startswith("killed") and os.path.exists(journal) and os.path.getsize(db) > 0:
        print("  attempt %d: %s; left: %s" % (attempt, state, left))
        caught = d
        break
    shutil.rmtree(d, ignore_errors=True)
if caught is None:
    print("  the window was not hit in %d attempts (Part A stands on its own)" % ATTEMPTS)
else:
    for i in (1, 2, 3):
        serve(caught, "start %d after the kill" % i)
    print("  after:", listing(caught))
