#!/usr/bin/env python3
"""T.8e review, T8E-1: the kill order is "the node's end of the channel
closes". It closes when the *last copy* of that descriptor closes, and a child
the node has forked but that has not reached `exec` yet holds a copy of every
descriptor of the node, close-on-exec or not. A spawn whose `chdir` waits on a
folder that does not answer (the node's git in a session folder,
`node/src/git.rs:371`; a tool that is not the shell, `tools/src/sandbox.rs:797`)
is such a child for as long as the folder hangs.

    held_channel.py            both runs, with the real guard and bwrap
    SWEEP=1 held_channel.py    also start a real node on the data directory
                               (its start-up sweep) while the command is left

Run "control": a node with one contained `sleep`, SIGKILLed. Expected and
found: the guard ends the namespace within milliseconds.
Run "stalled": the same node also started one other child whose folder hangs
(a FUSE folder this script stalls), then is SIGKILLed.
Writes under target/review_t8e/ only; unmounts and kills what it started.
"""
import os, signal, subprocess, sys, threading, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

SECS = 977100 + os.getpid() % 50
data = scratch("data")
root = os.path.join(data, "sandbox-tmp")
ws = scratch("ws")
mnt = scratch("mnt")


def node(stalled, w):
    """The node: one contained command, and (stalled) one more child whose
    working directory is the folder that hangs. Then it waits to be killed."""
    p, ours = spawn_guard(guard_argv(root, "s1", ws, f"sleep {SECS}"))
    wait_for(lambda: sleeps(SECS))
    if stalled:
        # What `std::process::Command::current_dir(folder).spawn()` does:
        # fork, chdir in the child, exec. Descriptors are not closed by hand
        # in the child (Rust relies on close-on-exec), hence close_fds=False.
        threading.Thread(target=lambda: subprocess.Popen(
            ["true"], cwd=os.path.join(mnt, "d"), close_fds=False), daemon=True).start()
    os.write(w, b"x")
    time.sleep(1000)
    os._exit(0)


def run(stalled, fuse):
    r, w = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(r)
        # Not the node's: this script's pipes to the FUSE server.
        os.close(fuse.stdin.fileno())
        os.close(fuse.stdout.fileno())
        try:
            node(stalled, w)
        finally:
            os._exit(1)
    os.close(w)
    os.read(r, 1)
    os.close(r)
    if stalled:
        assert fuse.stdout.readline().strip() == "held", "the folder did not hang"
    guard = [p for p in holding(root) if cmdline(p).split(b"\0")[1:2] == [b"sandbox-guard"]]
    t = time.monotonic()
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    left = gone_within(lambda: sleeps(SECS), 3.0)
    name = "stalled" if stalled else "control"
    print(f"{name}: node {pid} SIGKILLed; after {time.monotonic() - t:.2f} s: "
          f"command {'STILL RUNNING ' + str(left) if left else 'gone'}; "
          f"guard {guard} {'alive' if any(alive(g) for g in guard) else 'gone'}")
    return left, guard


fuse = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "fuse_hang.py"), mnt],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
try:
    assert fuse.stdout.readline().strip() == "mounted", "FUSE does not mount here"
    run(False, fuse)
    fuse.stdin.write("stall\n"); fuse.stdin.flush()
    assert fuse.stdout.readline().strip() == "stalling"
    left, guard = run(True, fuse)
    if left:
        holders = [p for p in pids() if p not in guard and p != os.getpid() and alive(p)
                   and cmdline(p) == cmdline(os.getpid())]
        print(f"  the node's unfinished child (same command line as the node, state "
              f"{[state(p) for p in holders]}): {holders}")
        time.sleep(5)
        print(f"  5 s later: command {sleeps(SECS) or 'gone'}")
        if os.environ.get("SWEEP"):
            # A real node start on this data directory: its sweep runs before
            # it serves. The log is printed; the node is stopped right after.
            log = os.path.join(SCRATCH, "node.log")
            with open(log, "w") as f:
                n = subprocess.Popen([EXE, "--data-dir", data, "serve", "--bind", "127.0.0.1:47917"],
                                     stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                                     env={**os.environ, "NO_COLOR": "1"})
            time.sleep(6)
            n.send_signal(signal.SIGINT)
            try:
                n.wait(10)
            except subprocess.TimeoutExpired:
                n.kill(); n.wait()
            lines = [l.rstrip() for l in open(log) if "containment" in l or "sandbox" in l]
            print("  a node restarted on the same data directory logged:")
            for l in lines:
                print("    " + l[:230])
            print(f"  after that start (its sweep): command {sleeps(SECS) or 'gone'}")
        fuse.stdin.close()
        fuse.wait()
        gone = gone_within(lambda: sleeps(SECS), 3.0)
        print(f"  the folder answers again (the child reaches exec or fails): command "
              f"{gone or 'gone'}")
finally:
    try:
        fuse.stdin.close()
    except OSError:
        pass
    fuse.wait()
    subprocess.run(["fusermount3", "-u", "-z", "--", mnt], stderr=subprocess.DEVNULL)
    k = cleanup(root, [SECS])
    print(f"cleanup: killed {k}; FUSE mounts of this review left: {my_fuse_mounts()}")
