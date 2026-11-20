# T.6 **sec**

## Adds

seccomp (C.10, if O4).
**Shrunk by O12 (2026-11-12):** where bubblewrap runs, T.8b's network namespace closes UDP and DNS with network off, so seccomp is no longer needed for that.
What it still adds: UDP and DNS off on the Landlock-only path (nested in the published image, no user namespaces); `io_uring`, `AF_VSOCK`, `ptrace`, `process_vm_*` and the syscall surface on every path; refusing new user namespaces on the Landlock path (bubblewrap has `--disable-userns`).
Keep it, re-scoped to those, after T.8b; loaded through `bwrap --add-seccomp-fd` on the bubblewrap path.

## Safeguards and abuse tests, written first

With network off, UDP `sendto` and a DNS lookup fail `EPERM`; `io_uring_setup` fails; a cargo build in the cwd works; unknown architecture refused under `required`.

## Invariants

No `unsafe` in our crates

## Podman / Node

No / No

## Depends

C.10, outside review, T.8b

## Size

**M**

