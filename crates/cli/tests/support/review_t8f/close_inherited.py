#!/usr/bin/env python3
"""T.8f re-check: `sandbox-exec` closes every descriptor it was given.

The real binary is started as `sandbox-exec … -- python3 -c <lister>` with
descriptors 3, 50, 1000 and 5000 passed on purpose (pipes and a folder), the
soft descriptor limit lowered to 1024 AFTER they were opened:

  1. with /proc (every real start has one): the command holds 0, 1, 2;
  2. without /proc (bubblewrap with an empty folder over it, which no node
     start makes): the fallback closes the numbers below the soft limit, so
     the descriptor numbered above it stays open. Stated in the code; shown
     so the bound is on record.

usage: close_inherited.py <path to rizzy-harness>
"""
import os, resource, subprocess, sys

exe = sys.argv[1]
LISTER = (
    "import os\n"
    "held = []\n"
    "for n in range(3, 6000):\n"
    "    try:\n"
    "        os.fstat(n); held.append(n)\n"
    "    except OSError:\n"
    "        pass\n"
    "print('the command holds above 2:', held or 'nothing')\n"
)
wrapper = [exe, "sandbox-exec", "--best-effort", "--ro", "system", "/usr", "--ro", "system", "/etc",
           "--", "/usr/bin/python3", "-c", LISTER]

def start(argv):
    r, w = os.pipe()
    d = os.open("/usr", os.O_RDONLY | os.O_DIRECTORY)
    want = {3: r, 50: w, 1000: d, 5000: r}
    def pre():
        for n, src in want.items():
            os.dup2(src, n, inheritable=True)
        hard = resource.getrlimit(resource.RLIMIT_NOFILE)[1]
        resource.setrlimit(resource.RLIMIT_NOFILE, (1024, hard))
    p = subprocess.run(argv, preexec_fn=pre, close_fds=False, stdin=subprocess.DEVNULL,
                       capture_output=True, text=True)
    for x in (r, w, d):
        os.close(x)
    return (p.stdout + p.stderr).strip()

hard = resource.getrlimit(resource.RLIMIT_NOFILE)[1]
if hard < 6000:
    sys.exit(f"the hard descriptor limit here is {hard}: descriptor 5000 cannot be made")
resource.setrlimit(resource.RLIMIT_NOFILE, (hard, hard))
print("passed on purpose: 3, 50, 1000, 5000; soft limit then 1024")
print("1. with /proc:   ", start(wrapper))
bw = ["/usr/bin/bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--tmpfs", "/proc", "--"]
print("2. without /proc:", start(bw + wrapper))
