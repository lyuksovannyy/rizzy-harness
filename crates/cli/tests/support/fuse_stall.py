#!/usr/bin/env python3
"""A FUSE file system the test controls (T.8e): one file, `stall`, whose
every READ is never answered while this process lives. A process reading it
waits in the kernel; once SIGKILLed it waits *uninterruptibly* (the request
is already in user space, `fs/fuse/dev.c` `request_wait_answer`), so the
kernel cannot end it until this process lets it go.

    fuse_stall.py <mountpoint>

Mounts through the setuid `fusermount3` (no root), prints `mounted` when it
serves and `read` when a READ is held, and exits when its stdin closes:
closing /dev/fuse aborts the connection and every held READ fails with
ENOTCONN, which releases the reader. The caller unmounts
(`fusermount3 -u -z <mountpoint>`). Writes no file.
"""
import errno, os, select, socket, struct, sys

mnt = sys.argv[1]
ours, theirs = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
pid = os.fork()
if pid == 0:
    ours.close()
    os.environ["_FUSE_COMMFD"] = str(theirs.fileno())
    os.set_inheritable(theirs.fileno(), True)
    os.execvp("fusermount3", ["fusermount3", "-o", "fsname=t8e-stall,subtype=t8e", "--", mnt])
theirs.close()
_, anc, _, _ = ours.recvmsg(1, socket.CMSG_LEN(4))
os.waitpid(pid, 0)
if not anc:
    sys.exit("fusermount3 sent no descriptor")
fd = struct.unpack("i", anc[0][2][:4])[0]

IN = struct.Struct("<IIQQIIIHH")      # fuse_in_header, 40 bytes
OUT = struct.Struct("<IiQ")           # fuse_out_header, 16 bytes
UID, GID = os.getuid(), os.getgid()


def attr(ino):
    # fuse_attr: ino size blocks atime mtime ctime (u64) atimensec mtimensec
    # ctimensec mode nlink uid gid rdev blksize flags (u32)
    if ino == 1:
        return struct.pack("<QQQQQQIIIIIIIIII", 1, 0, 0, 0, 0, 0, 0, 0, 0,
                           0o40555, 2, UID, GID, 0, 4096, 0)
    return struct.pack("<QQQQQQIIIIIIIIII", 2, 4096, 8, 0, 0, 0, 0, 0, 0,
                       0o100444, 1, UID, GID, 0, 4096, 0)


def reply(unique, err=0, body=b""):
    os.write(fd, OUT.pack(OUT.size + len(body), -err, unique) + body)


print("mounted", flush=True)
held = []
while True:
    r, _, _ = select.select([fd, sys.stdin], [], [])
    if sys.stdin in r and not os.read(sys.stdin.fileno(), 1):
        os._exit(0)  # closes /dev/fuse: the kernel aborts the connection
    if fd not in r:
        continue
    try:
        req = os.read(fd, 1 << 20)
    except OSError as e:
        if e.errno in (errno.ENOENT, errno.EINTR, errno.EAGAIN):
            continue
        if e.errno == errno.ENODEV:  # unmounted
            os._exit(0)
        raise
    _, op, unique, node, _, _, _, _, _ = IN.unpack_from(req)
    body = req[IN.size:]
    if op == 26:  # INIT
        major, minor, readahead, _ = struct.unpack_from("<IIII", body)
        reply(unique, 0, struct.pack("<IIIIHHIIHHII", 7, 31, readahead, 0, 16, 12,
                                     4096, 1, 0, 0, 0, 0) + b"\0" * 24)
    elif op == 1:  # LOOKUP
        if node == 1 and body.rstrip(b"\0") == b"stall":
            reply(unique, 0, struct.pack("<QQQQII", 2, 0, 0, 0, 0, 0) + attr(2))
        else:
            reply(unique, errno.ENOENT)
    elif op == 3:  # GETATTR
        reply(unique, 0, struct.pack("<QII", 0, 0, 0) + attr(node))
    elif op == 14:  # OPEN
        reply(unique, 0, struct.pack("<QII", 0, 0, 0))
    elif op == 15:  # READ: held, never answered
        held.append(unique)
        print("read", flush=True)
    elif op in (2, 36, 42):  # FORGET, INTERRUPT, BATCH_FORGET: no reply
        pass
    elif op in (18, 25):  # RELEASE, FLUSH
        reply(unique)
    else:
        reply(unique, errno.ENOSYS)
