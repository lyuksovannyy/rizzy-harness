#!/usr/bin/env python3
"""L-b: what the start's retry costs, measured with `doctor` (the same
`sandbox::start`): bubblewrap absent, a bubblewrap that always fails (a
stand-in that exits 1: user namespaces denied for good looks like this), one
that hangs for 5 s and then fails, and the real one.
"""
import os, stat, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from n10a_common import *


def doctor(data, toml):
    with open(os.path.join(data, "node.toml"), "w") as f:
        f.write(toml)
    t0 = time.monotonic()
    p = subprocess.run([common.EXE, "--data-dir", data, "doctor"], capture_output=True, timeout=300,
                       env=dict(os.environ, NO_COLOR="1"), cwd=os.path.dirname(data))
    took = time.monotonic() - t0
    out = p.stdout.decode(errors="replace")
    line = [l for l in out.splitlines() if "shell containment" in l]
    tries = (p.stderr.decode(errors="replace") + out).count("trying again in")
    return took, short(line[0] if line else out[-200:], 150), tries


def main():
    root = scratch("probe")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    base = 'workspace_roots = ["%s"]\n' % ws
    fail, slow, count = os.path.join(root, "bwrap-fail"), os.path.join(root, "bwrap-slow"), os.path.join(root, "runs")
    with open(fail, "w") as f:
        f.write("#!/bin/sh\necho x >> %s\nexit 1\n" % count)
    with open(slow, "w") as f:
        f.write("#!/bin/sh\necho x >> %s\nsleep 5\nexit 1\n" % count)
    for p in (fail, slow):
        os.chmod(p, 0o755)
    for name, toml in [
        ("the real bubblewrap", base),
        ("bubblewrap absent", base + '[sandbox]\nbwrap = "%s/none"\n' % root),
        ("always fails", base + '[sandbox]\nbwrap = "%s"\n' % fail),
        ("always fails, network off", base + '[sandbox]\nbwrap = "%s"\nnetwork = "off"\n' % fail),
        ("hangs 5 s, then fails", base + '[sandbox]\nbwrap = "%s"\n' % slow),
    ]:
        if os.path.exists(count):
            os.remove(count)
        took, line, tries = doctor(data, toml)
        runs = len(open(count).read().split()) if os.path.exists(count) else 0
        print("%-26s doctor took %5.1f s; stand-in runs %2d; 'trying again' lines %d\n      %s"
              % (name, took, runs, tries, line))


if __name__ == "__main__":
    main()
