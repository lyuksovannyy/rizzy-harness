#!/usr/bin/env python3
"""T.8e review: a FUSE file system with one folder, `d`, that stops
answering on command: the stand-in for a session folder on a network mount
whose server went away.

    fuse_hang.py <mountpoint>

Mounts through the setuid `fusermount3` (as `support/fuse_stall.py`), prints
`mounted`. A line `stall` on stdin makes every later LOOKUP of `d` and every
request on `d` itself wait for ever (prints `stalling`, then `held` for each
request it holds). Closing stdin ends the server: the kernel aborts the
connection and every held request fails with ENOTCONN. The caller unmounts
(`fusermount3 -u -z <mountpoint>`). Nothing is cached (validity 0), so each
`chdir` or `stat` of `d` asks the server again. Writes no file.
"""
import errno, os, select, socket, struct, sys

mnt = sys.argv[1]
ours, theirs = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
pid = os.fork()
if pid == 0:
    ours.close()
    os.environ["_FUSE_COMMFD"] = str(theirs.fileno())
    os.set_inheritable(theirs.fileno(), True)
    os.execvp("fusermount3", ["fusermount3", "-o", "fsname=t8e-review,subtype=t8e-review", "--", mnt])
theirs.close()
_, anc, _, _ = ours.recvmsg(1, socket.CMSG_LEN(4))
os.waitpid(pid, 0)
if not anc:
    sys.exit("fusermount3 sent no descriptor")
fd = struct.unpack("i", anc[0][2][:4])[0]

IN = struct.Struct("<IIQQIIIHH")      # fuse_in_header
OUT = struct.Struct("<IiQ")           # fuse_out_header
UID, GID = os.getuid(), os.getgid()


def attr(ino):
    return struct.pack("<QQQQQQIIIIIIIIII", ino, 0, 0, 0, 0, 0, 0, 0, 0,
                       0o40755, 2, UID, GID, 0, 4096, 0)


def reply(unique, err=0, body=b""):
    try:
        os.write(fd, OUT.pack(OUT.size + len(body), -err, unique) + body)
    except OSError:
        pass  # the request was interrupted and is gone


print("mounted", flush=True)
stalling = False
buf = b""
while True:
    r, _, _ = select.select([fd, sys.stdin], [], [])
    if sys.stdin in r:
        got = os.read(sys.stdin.fileno(), 64)
        if not got:
            os._exit(0)  # closes /dev/fuse: the kernel aborts the connection
        buf += got
        if b"stall" in buf:
            stalling = True
            buf = b""
            print("stalling", flush=True)
    if fd not in r:
        continue
    try:
        req = os.read(fd, 1 << 20)
    except OSError as e:
        if e.errno in (errno.ENOENT, errno.EINTR, errno.EAGAIN):
            continue
        if e.errno == errno.ENODEV:
            os._exit(0)
        raise
    _, op, unique, node, _, _, _, _, _ = IN.unpack_from(req)
    body = req[IN.size:]
    if op in (2, 36, 42):  # FORGET, INTERRUPT, BATCH_FORGET: no reply
        continue
    is_d = node == 2 or (op == 1 and node == 1 and body.rstrip(b"\0") == b"d")
    if stalling and is_d:
        print("held", flush=True)
        continue
    if op == 26:  # INIT
        major, minor, readahead, _ = struct.unpack_from("<IIII", body)
        reply(unique, 0, struct.pack("<IIIIHHIIHHII", 7, 31, readahead, 0, 16, 12,
                                     4096, 1, 0, 0, 0, 0) + b"\0" * 24)
    elif op == 1:  # LOOKUP
        if node == 1 and body.rstrip(b"\0") == b"d":
            reply(unique, 0, struct.pack("<QQQQII", 2, 0, 0, 0, 0, 0) + attr(2))
        else:
            reply(unique, errno.ENOENT)
    elif op == 3:  # GETATTR
        reply(unique, 0, struct.pack("<QII", 0, 0, 0) + attr(node))
    elif op in (27,):  # OPENDIR
        reply(unique, 0, struct.pack("<QII", 0, 0, 0))
    elif op in (28, 44):  # READDIR, READDIRPLUS: empty
        reply(unique)
    elif op in (18, 25, 29, 30):  # RELEASE, FLUSH, RELEASEDIR, FSYNCDIR
        reply(unique)
    elif op == 34:  # ACCESS
        reply(unique)
    else:
        reply(unique, errno.ENOSYS)
