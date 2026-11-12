#!/usr/bin/env python3
"""T.8f review: can another file come to have the device and inode of an
image that is still held (run, or open), after its last name is gone?

The guard compares the link's (device, inode) with its own /proc/self/exe.
The guard runs that inode, so the file system cannot give the number to
another file while the comparison can be made at all. Shown on this file
system: a file is held open and unlinked; N files are created; none gets its
number. The descriptor is closed; files are created again; the number comes
back. Works in a scratch folder under TMPDIR, removed at the end.
"""
import os, shutil, sys, tempfile

n = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
d = tempfile.mkdtemp(prefix="review-t8f-ino-")
try:
    p = os.path.join(d, "image")
    with open(p, "w") as f:
        f.write("image")
    fd = os.open(p, os.O_RDONLY)
    held = os.fstat(fd)
    os.unlink(p)
    print(f"held and unlinked: inode {held.st_ino}, links now {os.fstat(fd).st_nlink}; "
          f"/proc/self/fd names {os.readlink(f'/proc/self/fd/{fd}')!r}")
    same = 0
    for i in range(n):
        q = os.path.join(d, f"f{i}")
        with open(q, "w"):
            pass
        st = os.lstat(q)
        same += (st.st_dev, st.st_ino) == (held.st_dev, held.st_ino)
        os.unlink(q)
    print(f"while it is held: {n} files created, {same} with its device and inode")
    os.close(fd)
    again = None
    for i in range(n):
        q = os.path.join(d, f"g{i}")
        with open(q, "w"):
            pass
        st = os.lstat(q)
        os.unlink(q)
        if (st.st_dev, st.st_ino) == (held.st_dev, held.st_ino):
            again = i
            break
    print("after it was closed: " + (f"file {again} got its device and inode" if again is not None
                                      else f"not reused in {n} files"))
finally:
    shutil.rmtree(d, ignore_errors=True)
