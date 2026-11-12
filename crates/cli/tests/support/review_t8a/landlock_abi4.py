#!/usr/bin/env python3
"""T.8a review: a stand-in for `rizzy-harness sandbox-exec` on a kernel with
Landlock ABI 4 (Ubuntu 24.04's 6.8, the kernel README.md names).

It takes the wrapper's arguments (`--rw <expect> <path>`, `--ro <expect>
<path>`, `-- <command>`), applies the rights such a kernel has (the
filesystem rights of ABI 1 to 3; no device-ioctl right, no abstract-socket or
signal scope, no pathname-socket right), sets `no_new_privs` and execs the
command. The real wrapper asks for the newer rights best-effort
(`crates/tools/src/sandbox.rs`, `imp::restrict`), so on such a kernel this is
what it enforces. Identities are not checked here: the proofs that use this
do not swap folders.
"""
import ctypes
import os
import sys

libc = ctypes.CDLL(None, use_errno=True)
FS_ABI3 = (1 << 15) - 1  # LANDLOCK_ACCESS_FS_EXECUTE .. _TRUNCATE
FS_FILE = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 14)  # execute, write, read, truncate
FS_READ = (1 << 0) | (1 << 2) | (1 << 3)  # execute, read file, read dir


def check(ok, what):
    if not ok:
        sys.exit(f"landlock_abi4.py: {what}: {os.strerror(ctypes.get_errno())}")


args = sys.argv[1:]
rules, i = [], 0
while args[i] != "--":
    if args[i] in ("--rw", "--ro"):
        rules.append((args[i], args[i + 2]))
        i += 3
    else:
        i += 1
command = args[i + 1:]

handled = ctypes.c_uint64(FS_ABI3)  # struct landlock_ruleset_attr, first field only
ruleset = libc.syscall(444, ctypes.byref(handled), ctypes.c_size_t(8), 0)
check(ruleset >= 0, "landlock_create_ruleset")
for kind, path in rules:
    try:
        fd = os.open(path, os.O_PATH | os.O_CLOEXEC)
    except FileNotFoundError:
        continue
    access = FS_ABI3 if kind == "--rw" else FS_READ
    if not os.path.isdir(path):
        access &= FS_FILE
    # struct landlock_path_beneath_attr { __u64 allowed_access; __s32 parent_fd; } packed
    attr = access.to_bytes(8, "little") + fd.to_bytes(4, "little", signed=True)
    check(libc.syscall(445, ruleset, 1, attr, 0) == 0, f"landlock_add_rule {path}")
check(libc.prctl(38, 1, 0, 0, 0) == 0, "no_new_privs")
check(libc.syscall(446, ruleset, 0) == 0, "landlock_restrict_self")
os.execvp(command[0], command)
