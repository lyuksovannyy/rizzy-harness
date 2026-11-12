#!/usr/bin/env python3
"""T.8e re-check, T8E-5's fix on a real node: does `serve` hold a descriptor
of its own running image, close-on-exec, and does that descriptor still name
the image after the file at the binary's path was replaced?

A copy of the binary (a copy, not a link: nothing here touches the build) is
started as `serve` on a scratch data directory. Its descriptors are read from
/proc; the path is then given another file; an open of the running image for
writing is tried (no byte is written). The node is stopped.
"""
import os, shutil, signal, subprocess, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

data = scratch("data")
bindir = scratch("bin")
exe = os.path.join(bindir, "rizzy-harness")
shutil.copy2(EXE, exe)
ino = os.stat(exe).st_ino
log = open(os.path.join(SCRATCH, "node.log"), "w")
n = subprocess.Popen([exe, "--data-dir", data, "serve", "--bind", "127.0.0.1:47920"],
                     stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
try:
    time.sleep(5)
    held = []
    for fd in os.listdir(f"/proc/{n.pid}/fd"):
        try:
            st = os.stat(f"/proc/{n.pid}/fd/{fd}")
        except OSError:
            continue
        if st.st_ino == ino:
            flags = [l.split()[1] for l in open(f"/proc/{n.pid}/fdinfo/{fd}") if l.startswith("flags")][0]
            held.append((int(fd), os.readlink(f"/proc/{n.pid}/fd/{fd}"), flags,
                         bool(int(flags, 8) & 0o2000000)))
    print(f"the node {n.pid} holds its image as: {held} (fd, link, flags, close-on-exec)")
    os.rename(exe, exe + ".old")
    with open(exe, "w") as f:
        f.write("#!/bin/sh\necho replaced\n")
    os.chmod(exe, 0o755)
    for fd, _, _, _ in held:
        st = os.stat(f"/proc/{n.pid}/fd/{fd}")
        print(f"the path replaced: fd {fd} is still inode {st.st_ino} (the image is {ino}): "
              f"{st.st_ino == ino}; link now {os.readlink(f'/proc/{n.pid}/fd/{fd}')}")
    try:
        w = os.open(exe + ".old", os.O_WRONLY)
        os.close(w)
        print("an open of the running image for writing: ALLOWED")
    except OSError as e:
        print(f"an open of the running image for writing: refused ({e.strerror})")
finally:
    n.send_signal(signal.SIGINT)
    try:
        n.wait(10)
    except subprocess.TimeoutExpired:
        n.kill(); n.wait()
    log.close()
    shutil.rmtree(bindir, ignore_errors=True)
