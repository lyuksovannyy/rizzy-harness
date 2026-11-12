#!/usr/bin/env python3
"""T.8e re-check: the guard's look at its parent (`--node <pid>`, the fix of
T8E-1). Is there an order in which the node is alive and the guard believes
it is not? Real guard, real bwrap; this script, or a child of it, is the node.

  A  a tracer that is not the parent attaches to the guard (this script, the
     guard's grandparent, with PTRACE_SEIZE): `getppid` must still name the
     node. The command must be alive 2 s later.
  B  the thread of the node that spawned the guard ends; the node lives.
  C  the guard is started by another process than the one named in `--node`
     (a helper between the node and the guard): what happens.
  D  the node is SIGSTOPped for 2 s (not dead): the command must live.
  E  the node dies while a second child holds a copy of the channel (T8E-1's
     case, without FUSE: the copy is simply kept open): how long until the
     command is gone.
"""
import ctypes, os, signal, subprocess, sys, threading, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977700 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
ok = True


def verdict(name, held, text):
    global ok
    ok &= held
    print(f"{name}: {'HELD' if held else 'BROKEN'}: {text}")


def node_process(body):
    """Fork a node that spawns one guarded `sleep`, runs body(p, ours) and
    then waits; returns (node pid, guard pid)."""
    r, w = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(r)
        try:
            p, ours = spawn_guard(guard_argv(root, "s1", ws, f"sleep {SECS}"))
            wait_for(lambda: sleeps(SECS))
            body(p, ours)
            os.write(w, b"%d\n" % p.pid)
            time.sleep(1000)
        finally:
            os._exit(1)
    os.close(w)
    guard = int(os.read(r, 32))
    os.close(r)
    return pid, guard


def end(node):
    os.kill(node, signal.SIGKILL)
    os.waitpid(node, 0)
    return gone_within(lambda: sleeps(SECS), 3.0)


# A
libc = ctypes.CDLL(None, use_errno=True)
PTRACE_SEIZE, PTRACE_DETACH = 0x4206, 17
node, guard = node_process(lambda p, ours: None)
rc = libc.ptrace(PTRACE_SEIZE, guard, 0, 0)
err = ctypes.get_errno()
tracer = [l.split()[1] for l in open(f"/proc/{guard}/status") if l.startswith("TracerPid")]
time.sleep(2)
alive_a = bool(sleeps(SECS))
libc.ptrace(PTRACE_DETACH, guard, 0, 0)
verdict("A", rc == 0 and alive_a,
        f"PTRACE_SEIZE of guard {guard} by its grandparent {os.getpid()}: rc {rc} errno {err}, "
        f"TracerPid {tracer}, PPid still {ppid(guard)} (node {node}); command alive 2 s later: {alive_a}")
left = end(node)
verdict("A'", not left, f"then the node SIGKILLed: command {'gone' if not left else left}")

# B
def from_a_thread_that_ends(p, ours):
    pass
r, w = os.pipe()
node = os.fork()
if node == 0:
    os.close(r)
    box = []
    t = threading.Thread(target=lambda: box.append(spawn_guard(guard_argv(root, "s1", ws, f"sleep {SECS}"))))
    t.start(); t.join()
    wait_for(lambda: sleeps(SECS))
    os.write(w, b"%d\n" % box[0][0].pid)
    time.sleep(1000)
    os._exit(1)
os.close(w)
guard = int(os.read(r, 32)); os.close(r)
time.sleep(2)
alive_b = bool(sleeps(SECS))
verdict("B", alive_b, f"the spawning thread ended, the node {node} lives: guard {guard} PPid {ppid(guard)}; "
                      f"command alive 2 s later: {alive_b}")
left = end(node)
verdict("B'", not left, f"then the node SIGKILLed: command {'gone' if not left else left}")

# C
argv = guard_argv(root, "s1", ws, f"sleep {SECS}")
at = argv.index("sandbox-guard") + 1
argv = argv[:at] + ["--node", str(os.getpid())] + argv[at:]
helper = subprocess.Popen([sys.executable, "-I", "-c",
                           "import subprocess,sys; p=subprocess.Popen(sys.argv[1:], stderr=subprocess.PIPE); "
                           "print(p.wait(), p.stderr.read().decode().strip())"] + argv,
                          stdout=subprocess.PIPE, text=True)
out = helper.stdout.read().strip()
helper.wait()
verdict("C", not sleeps(SECS), f"a guard whose parent is a helper, not the pid in --node: {out!r} "
                               f"(it starts nothing: the node must be the guard's direct parent)")

# D
node, guard = node_process(lambda p, ours: None)
os.kill(node, signal.SIGSTOP)
time.sleep(2)
alive_d = bool(sleeps(SECS))
os.kill(node, signal.SIGCONT)
verdict("D", alive_d, f"the node stopped for 2 s: command alive: {alive_d}")
left = end(node)

# E
def keep_a_copy(p, ours):
    if os.fork() == 0:          # a child of the node that never reaches exec
        time.sleep(30)
        os._exit(0)
node, guard = node_process(keep_a_copy)
t = time.monotonic()
os.kill(node, signal.SIGKILL)
os.waitpid(node, 0)
left = gone_within(lambda: sleeps(SECS), 3.0)
verdict("E", not left, f"the node SIGKILLed while its other child holds the channel: command "
                       f"{'gone after %.2f s' % (time.monotonic() - t) if not left else 'STILL RUNNING'}")
holders = [p for p in pids() if p != os.getpid() and cmdline(p) == cmdline(os.getpid()) and alive(p)]
kill_pids(holders)
print(f"cleanup: {cleanup(root, [SECS])}; the copy-holder {holders} killed")
sys.exit(0 if ok else 1)
