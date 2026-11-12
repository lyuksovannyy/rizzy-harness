#!/usr/bin/env python3
"""timeit.py <label> <runs> <argv...>: run argv <runs> times after one
warm-up, wall clock per run (fork to wait), print one line:
label runs min median p90 max (milliseconds). A run that exits non-zero is
counted and reported (rc!=0), not dropped."""
import statistics
import subprocess
import sys
import time

label, runs, argv = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
subprocess.run(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ts, bad = [], 0
for _ in range(runs):
    t = time.perf_counter()
    r = subprocess.run(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ts.append((time.perf_counter() - t) * 1000)
    bad += r.returncode != 0
ts.sort()
p90 = ts[min(len(ts) - 1, int(len(ts) * 0.9))]
print(f"{label:<28} n={runs} min={ts[0]:.1f} median={statistics.median(ts):.1f} "
      f"p90={p90:.1f} max={ts[-1]:.1f} ms" + (f" rc!=0:{bad}" if bad else ""))
