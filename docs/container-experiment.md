# Containers as the confinement for agent shells (experiment B.11)

The owner's direction of 2026-11-12 is to replace the Landlock sandbox of the
agent's `bash` with Podman, after quick Podman tests on a separate branch.
This note is those tests. Two things are open after the security review:
the sandbox exists only where the kernel has Landlock, and a process that
detaches itself (`setsid sleep 1000`) survives cancel, timeout and a
`kill -9` of the node (B.1). Bubblewrap (`bwrap`) is measured beside Podman
because it needs no image, daemon or `subuid` and might give the same
process-tree kill for less setup.

The scripts are in `experiments/b11/` (how to rerun them, and what they
leave behind, is in its README); their raw output is in
`experiments/b11/results/`. Every number below comes from those files.

## The options compared

| Name | What it is | Invocation (all in `experiments/b11/lib.sh`) |
|---|---|---|
| **sandbox-exec** | Today's confinement: the harness binary confines itself with Landlock, then `exec`s `bash -c`. | The node's own argument shape (`Sandbox::wrapper`, `crates/tools/src/sandbox.rs:349`): `--rw <dev>:<ino>:<label>` for the session folder and its `TMPDIR`, `--rw system` for the five device files, `--ro system` for `SYSTEM_READ` and the repository, `--no-network` where stated. Started with `HOME` set to the session folder (see finding 6). |
| **bwrap per command** | One bubblewrap sandbox per command. | `bwrap --unshare-all --die-with-parent --new-session`, read-only `/usr` and `/etc`, the merged-usr links, a new `/proc`, a small `/dev`, a tmpfs `/tmp`, the session folder read-write and the repository read-only at their own paths. No network unless `--share-net`. |
| **bwrap per session** | A long-lived holder (`sleep infinity` under bwrap's init, the PID namespace's pid 1, `--die-with-parent` to the node). Each command enters the holder's user, PID and mount namespaces with `nsenter -U -p -m` and runs a fresh `bwrap --unshare-user --unshare-pid …` there. | `bws_start`, `bws_args`. See finding 4 for why it has to be this shape. |
| **podman exec** | The nodes design (§4.6): one container per session, `podman exec` per command. | `podman run -d --userns=keep-id --cap-drop=all --security-opt=no-new-privileges --read-only --tmpfs /tmp --tmpfs /run:nosuid,nodev,noexec --network=none\|pasta --memory=4g --memory-swap=4g --cpus=2 --pids-limit=1024 --ulimit core=0 --init --pull=never`, the session folder and repository mounted at their own paths, `sleep infinity`; then `podman exec --workdir <folder> <ctr> bash -c <cmd>`. `relabel=private` and `--log-driver=none` were left out (no SELinux here; logging is irrelevant to the tests). |
| **podman run --rm** | One container per command, for comparison. | The same flags, `podman run --rm … bash -c <cmd>`. |

The image is `localhost/rh-b11:tools`, Debian 13 (`docker.io/library/debian:trixie-slim`,
the published image's base) plus git, procps, gcc, libcap2-bin, kmod,
iproute2, python3, bubblewrap and podman (`experiments/b11/Containerfile`).
Both images were left in the user's Podman store.

## Machine

| | |
|---|---|
| CPU, RAM | AMD Ryzen 7 9800X3D (8 cores, 16 threads), 31 GiB |
| Kernel | Linux 7.2.15 (Arch), LSMs `capability,landlock,lockdown,yama,bpf`, Landlock ABI 10 (the binary uses up to 9), `ptrace_scope` 1, cgroups v2 with `cpu memory pids` delegated to the user manager |
| Podman | 6.2.0 rootless, crun 1.30.1, netavark with pasta 2026_10_02, systemd cgroup manager, overlay storage in `$HOME` |
| bubblewrap | 0.13.1, not setuid |
| Disk | Test folders and Podman's store on NVMe ext4 (`~/.cache/rh-b11`); not the slow USB disk the repository is on |
| sandbox-exec | `target/debug/rizzy-harness`, a **debug build** (177 MB) from the main checkout, built 2026-11-12 01:21; not rebuilt for this note, so it is the binary of that time, which I take to be `main` near `4961065` but did not verify |
| Load | about 1.3 (1-minute average) at the end; another agent ran browser tests on the machine at times |

Times are wall-clock, from a Python `subprocess.run` loop (`timeit.py`): one
warm-up, then 50 runs (20 for `podman run --rm`), reported as median and
worst. Memory is RSS from `ps` and PSS from `/proc/<pid>/smaps_rollup`
(PSS divides shared pages among their users and is the fairer per-session
number).

## 1. Cost

### Per command (milliseconds, median / worst of 50)

| Command | Unconfined | sandbox-exec (debug) | bwrap per command | bwrap per session | podman exec | podman run --rm (20 runs) |
|---|---|---|---|---|---|---|
| `true` | 0.5 / 0.8 | 2.2 / 3.0 | 1.5 / 2.3 | 1.7 / 2.4 | **70.8 / 77.7** | 67.6 / 78.1 |
| `git status`, 200 files, read-write | 1.1 / 1.5 | 2.8 / 3.4 | 2.1 / 2.6 | 2.3 / 3.5 | 68.7 / 75.4 | 68.4 / 79.4 |
| `git status`, 3,000 files, read-only mount | 1.8 / 2.1 | 3.6 / 4.3 | 2.7 / 3.4 | 3.0 / 3.4 | 68.5 / 79.4 | not run |
| `ls -R`, 3,000 files | 1.3 / 1.6 | 2.9 / 3.3 | 2.4 / 2.6 | 2.6 / 3.2 | 69.4 / 81.2 | not run |

The overhead over unconfined is about 1 ms for bwrap, 1.7 ms for the debug
`sandbox-exec`, and about 68 ms for either Podman form. Podman's cost is
the `podman` client itself (a Go program that starts, opens the store and
talks to conmon), not the container: `podman exec` is no faster than
starting a whole new container with `podman run --rm`. Its client peaks at
52 MB RSS per call (`getrusage`), against 14 MB for bwrap and 19 MB for the
debug `sandbox-exec`. An agent turn with 30 shell calls pays about 2 s more
with Podman than with bwrap.

### Start-up and per-session cost

| | sandbox-exec | bwrap per command | bwrap per session | podman (container per session) |
|---|---|---|---|---|
| Create the confinement | nothing (a 0700 temp folder) | nothing | 2.0 to 2.5 ms (one run 13 ms), 10 runs | `podman run -d`: 72 ms median, 80 worst (network none); 79 / 92 (pasta); 10 runs |
| Remove it | nothing | nothing | `kill -9` of the holder | `podman rm -f -t 0`: 74 ms median |
| Processes per idle session | none | none | 3 (two bwrap, one sleep) | conmon, the init (`podman-init`, catatonit), `sleep infinity`; with network, one `pasta` |
| Idle memory per session | 0 | 0 | 7.7 MB RSS, 0.46 MB PSS | network none: 4.9 MB RSS, 2.7 MB PSS (conmon alone 2.3 MB RSS, 0.4 MB PSS). **pasta adds 26.4 MB RSS, 25.7 MB PSS per session** |
| 8 idle sessions | 0 | 0 | not measured | network none: 38.6 MB RSS, 5.7 MB PSS in all; with pasta, eight more pasta processes (not measured together; 8 × 25.7 MB PSS would be about 206 MB) |
| Disk per session | the temp folder's contents | none | none | 44 kB in `overlay-containers/<id>`; 14 kB writable layer (`SizeRw`, with `--read-only`); the image is shared (81 MB base; this test image 1.1 GB because it carries gcc and podman) |
| A daemon | no | no | no | no daemon; one `catatonit -P` pause process per user holds the user namespace |

Putting a command into a cgroup of its own with `systemd-run --user --scope`
(the cgroup route of C.11) costs 2.5 ms median (30 runs), so a cgroup per
command is affordable with bwrap or `sandbox-exec` where a user manager runs.

## 2. Process containment (T.8)

Each command leaves behind five processes: a `setsid` child, a `nohup … &`
child, a double-forked child, a child that ignores SIGTERM (started by
`sh -c 'trap "" TERM; exec sleep …'`), and a plain foreground child the
command waits on. A stand-in node (`node.sh`) starts the command in its own
process group, as `bash.rs` does. Then one of four things happens, and
`pgrep` looks for each child 2 s and 10 s later. In every cell the 2 s and
10 s answers were the same.

| What happens | sandbox-exec | bwrap per command | bwrap per session | podman exec | podman run --rm |
|---|---|---|---|---|---|
| **a. Cancel as today**: `killpg(TERM)`, 1 s, `killpg(KILL)` of the client's group | **setsid survives**; the rest gone | all gone | all gone | **setsid and the TERM-ignoring child survive**; the rest gone | all gone |
| **b. Stop the confinement** | n/a (nothing to stop) | `kill -9` of bwrap's init: all gone | `kill -9` of the holder's init: all gone | `podman stop -t 2`: all gone | `podman stop -t 2`: all gone |
| **c. `kill -9` of the node**, no cleanup | **all five survive** | all gone | all gone | **all five survive**, the container stays up, the orphaned `podman exec` client stays alive | **all five survive**, the container stays up, the orphaned `podman run` client stays alive |
| **e. The command exits normally** | setsid, nohup, double fork and TERM-ignoring child survive | all gone | all gone | all four survive in the container | all gone |

The mechanism in every "all gone" cell is the PID namespace: when its init
(pid 1) dies, the kernel kills every process in it, nested namespaces
included, whatever their session or signal mask. bwrap's own init exits
when the command does, so a bwrap command takes its whole tree with it,
and with `--die-with-parent` it also dies when its parent dies (case c,
through `PR_SET_PDEATHSIG`). For `podman run --rm` the container's init
ends with the command. A `podman exec` command is one more process in a
container whose init is `sleep infinity`'s; nothing ends it but stopping
the container.

What is left behind, and what a restarted node must clean up:

| | After a node `kill -9` | How a restarted node finds it |
|---|---|---|
| sandbox-exec | the processes themselves, now children of init | nothing marks them; B.1's open residual |
| bwrap | nothing | nothing to find |
| podman exec, podman run --rm | a running container with everything in it, conmon, and the orphaned `podman` client | `podman ps -a --filter label=…` (the design's `rizzy.env`/`rizzy.ws` labels); `podman rm -f` each, 74 ms per container. Until the node restarts, the processes run on |

## 3. Signals and exit

| | sandbox-exec | bwrap | podman exec |
|---|---|---|---|
| SIGTERM to the client | reaches the command (it is the command: `sandbox-exec` `exec`s) | **does not reach it**: bwrap dies, its child gets SIGKILL from `--die-with-parent`; no trap runs | **reaches it** (the command's TERM trap ran); its process group's other members died too, the `setsid` child did not |
| SIGINT to the client | reaches the command | does not reach it (as above) | reaches it |
| SIGKILL to the client | kills the command only | kills everything (pdeathsig) | **reaches nothing**: the command and all its children run on |
| Client exit code: `exit 3` / killed by SIGKILL / by SIGTERM | 3 / 137 / 143 | 3 / 137 / 143 | 3 / 137 / 143 (also `podman run --rm`) |
| stdout and stderr | kept apart | kept apart | kept apart (also `podman run --rm`) |

**The nodes design's stop** (`podman exec <ctr> kill -TERM -<pgid>`, then
`-KILL`) was tested as written. A `podman exec` command is already its own
session and group leader (pid = pgid = sid inside the container), so the
wrapper only needs to print its pid. `kill -TERM -<pgid>` took 106 ms (one
more `podman exec`), the command's trap ran and the client exited 143. The
`setsid` child was still alive after `-TERM` and after `-KILL` of the group.
`pkill -KILL -s <sid>` with the child's own session id killed it, but the node
cannot know that id without walking the container's `/proc`, which is the
racy walk C.11 already rejected. **Only stopping the container killed a
`setsid` child** of a `podman exec` command. So "then stops the container"
in §4.6 is not a fallback for a stuck command; it is the only step that
meets T.8's test, and it ends every other process of the session too.

A command stopped by the node's timeout would report what its client
reports: a `podman exec` whose command is killed from inside the container
exits 137.

## 4. Files

Session folder read-write, repository read-only, a sibling session's folder
and a stand-in for `~/.ssh` (a 0700 folder in the home; this machine has no
`~/.ssh`) not granted.

| Can the command… | sandbox-exec | bwrap | podman, keep-id | podman, root in container, no keep-id | podman, uid 1000 in container, no keep-id |
|---|---|---|---|---|---|
| read the host's `/etc/passwd` | yes (`/etc` is granted) | yes (as configured here; `/etc` could be narrowed) | no, the image's | no, the image's | no, the image's |
| read the `~/.ssh` stand-in | no (EACCES) | no (absent) | no (absent) | no (absent) | no (absent) |
| read a sibling session's folder | no (EACCES) | no (absent) | no (absent) | no (absent) | no (absent) |
| list `/home` | no | sees only the mount path's parents | same | same | same |
| see host processes in `/proc` | **yes: 422 pids, every command line** (pid 1's read as `/sbin/init`) | no: 5 pids, its own | no: 6 pids | no | no |
| read another process's `environ` (the test driver's) | no (EACCES: Landlock's ptrace rule) | no (not visible) | no | no | no |
| follow a symlink in its folder to the secret | no (EACCES) | no (target absent) | no | no | no |
| follow a symlink to `/etc/passwd` | yes, the host's | yes, the host's | the image's | the image's | the image's |
| create a file in its folder | yes | yes | yes | yes | **no (EACCES)** |
| owner and mode on the host of what it created | 1000:1000, 644 / 755 | 1000:1000, 644 / 755 | 1000:1000, 644 / 755 | 1000:1000, 644 / 755 | n/a |

Without `keep-id`, container root is the host user, so files come out owned
by the user; a non-root user in the container maps to a subordinate uid
(101000 here) and cannot write the user's folder at all. `keep-id` with
`--user <uid>` as the design says is the combination that gives both a
non-root process and files the node owns.

**A mount source that is a symlink is followed** at `podman run` time, and by
`bwrap --bind` too: `src=<link to the secret folder>` mounted the secret
folder in both. So the design's "no symlink in the path" check before
`podman run` leaves the check-then-use window of the review's S-2 open.
What closes it:

| | Result |
|---|---|
| `bwrap --bind-fd <fd>` on a descriptor the node opened, after the folder was swapped | mounts the folder the node opened (marked "checked"), not the one now at the path: race-free |
| Podman `--mount src=/proc/<node pid>/fd/<fd>` | refused: `statfs /proc/<pid>/fd/9: permission denied` (Podman runs in its own user namespace and cannot open the node's descriptor) |
| Podman, check after start: `podman exec <ctr> stat -c %d:%i <dst>` | gives the same device and inode as `stat` on the host (`66310:4202263` both sides), so the node can compare the mounted folder with its pin before the first command, and remove the container on a mismatch |

## 5. Network (the design's test D14)

Listeners on the host: TCP and UDP on 127.0.0.1 and on the LAN address
(192.168.1.100). One DNS lookup of `example.com` per option, one HTTPS
`HEAD` where a network was meant to exist, and (outside the plan) two bare
TCP connects to `example.com:443` while testing pasta's options.

| From inside | sandbox-exec `--no-network` | bwrap `--unshare-net` | podman `--network=none` | podman, pasta (the default) | podman, pasta `--map-guest-addr none` | sandbox-exec, network on |
|---|---|---|---|---|---|---|
| TCP to host 127.0.0.1 | denied (EACCES) | refused (own loopback) | refused (own loopback) | **refused** | not run | connected |
| TCP to host LAN address | denied | unreachable | unreachable | refused (the container holds the same address itself) | refused | connected |
| TCP to `169.254.1.2` / `host.containers.internal`, port bound on the LAN address or `0.0.0.0` | denied | unreachable | unreachable | **connected to the host's service** | timed out (not mapped) | n/a |
| same, port bound on 127.0.0.1 only | denied | unreachable | unreachable | refused | not run | n/a |
| TCP to `169.254.169.254:80` (cloud metadata) | denied | unreachable | unreachable | timed out (no such service on this LAN) | not run | timed out |
| UDP to host 127.0.0.1 | **sent and received by the host** | sent, not received (own loopback) | sent, not received | sent, not received | not run | received |
| UDP to host LAN address | **sent and received by the host** | unreachable | unreachable | sent, not received | not run | received |
| DNS lookup | **resolved** | failed | failed | resolved | not run | resolved |
| HTTPS to example.com | not run | not run | not run | HTTP 200 | TCP connect works | HTTP 200 |

So rootless pasta keeps a container off the host's loopback, as the design
hoped, but **not off the host**: every host service bound to `0.0.0.0` or
the LAN address is reachable at `169.254.1.2` (`host.containers.internal`
in the container's `/etc/hosts`), and the connection arrives from the host's
own LAN address, so a service that trusts its LAN will trust it. Podman
already passes `--no-map-gw`; `--network=pasta:--map-guest-addr,none` removes
the mapping (169.254.1.2 then timed out) and the internet still worked.
IPv6 was not tested. Landlock's "network off" lets UDP and DNS out, as
`sandbox.rs` documents; both namespace options close them.

## 6. Privilege

| Inside | sandbox-exec | bwrap | podman (keep-id, the design's flags) | podman, root in container |
|---|---|---|---|---|
| Effective capabilities | none | none | none | none |
| Bounding set | full | empty | empty | empty |
| `no_new_privs` | 1 | 1 | 1 | 1 |
| seccomp | none | none | filter (mode 2) | filter |
| `mount -t tmpfs` | refused | refused | refused | refused |
| `insmod` | fails (EINVAL from kmod; the test did not reach the capability check, so it shows nothing; no option holds `CAP_SYS_MODULE` in the initial namespace) | same | same | same |
| `ptrace` own child / a sibling | allowed / denied (Yama) | allowed / denied | allowed / denied | allowed / denied |
| `/dev` | listing denied; five device files usable | 14 entries, no block devices | 15 entries, no block devices | 15, none |
| `unshare -U` (new user namespace) | allowed | allowed | allowed | allowed |
| `unshare -Urm` (user + mount, map root) | denied | **allowed** | **allowed** | denied |
| write `/usr`, `/etc` | denied | read-only | read-only | read-only |
| write `/tmp` | **denied** (`TMPDIR` is the session's temp folder) | yes (tmpfs) | yes (tmpfs) | yes |
| gcc compiles into the session folder and the result runs | yes | yes | yes | yes |
| `git commit` in the session folder | yes | yes | yes | yes |

Ordinary work runs under `--cap-drop=all`, `no-new-privileges` and a
read-only root. Every option lets a command create a user namespace, and in
bwrap and Podman it can also map itself root in it and mount; that is
kernel attack surface, not a way out of the confinement. `bwrap
--disable-userns` would close it for bwrap (not tested).

## 7. What the machine must provide

| Needs | sandbox-exec | bwrap | rootless Podman |
|---|---|---|---|
| Kernel | Landlock (ABI 3 enforced in full; ABI 4 for TCP off) | unprivileged user namespaces | unprivileged user namespaces; cgroups v2 with delegation for `--memory`, `--cpus`, `--pids-limit` |
| Helper programs | none (the harness binary) | `bwrap` (not setuid) | `podman`, `conmon`, `crun`, `pasta`, `catatonit`, **`newuidmap`/`newgidmap` with file capabilities or setuid** |
| `/etc/subuid`, `/etc/subgid` | no | no | yes |
| A user runtime directory | no | no (worked without `XDG_RUNTIME_DIR`) | yes, see below |
| Works under `no_new_privs` (the unit's `NoNewPrivileges=yes`) | yes (it sets it itself) | yes | **no, for a user without a running pause process**: `setpriv --no-new-privs newuidmap …` fails with "Could not set caps"; the same call without `no_new_privs` works. Podman succeeded under `setpriv --no-new-privs` only because it joined this user's existing pause process |
| Works under `ProtectSystem=strict` + `ReadWritePaths` | yes (as a `systemd-run --user` unit) | yes (same) | not established (see "not run") |
| Nested in a rootless container with default flags | **yes: Landlock enforced in full (ABI 9) inside** | **no**: "Can't mount proc on /proc: Operation not permitted" (the container's masked `/proc` paths). It runs without a new `/proc`, or with the outer container started `--security-opt unmask=ALL` (seccomp could stay on) | **no**: as the container's uid 1000 the image cannot be unpacked (no subordinate ids inside); with `vfs.ignore_chown_errors=true` it fails at `open /proc/sys/net/ipv4/ping_group_range: Read-only file system`; as container root it fails at unpacking too |

**The README's systemd unit** (`User=rizzy-harness`, `NoNewPrivileges=yes`,
`ProtectSystem=strict`, `ReadWritePaths=/var/lib/rizzy-harness
/home/rizzy-harness`):

- `NoNewPrivileges=yes` stops rootless Podman from mapping a user namespace
  (`newuidmap` loses its capability), so a node started by that unit cannot
  create its first container. bwrap and `sandbox-exec` are unaffected.
- A system unit for a `nologin` user without linger has no
  `/run/user/<uid>` and no user bus. Podman then needs a runtime directory
  of its own and falls back from the systemd cgroup manager to `cgroupfs`,
  where rootless resource limits need delegation it does not have. I could
  not reproduce that here: with `XDG_RUNTIME_DIR` and the bus variable
  unset, Podman still found `/run/user/1000` and the bus (I am logged in),
  and all three runs (plain, with `--memory --pids-limit`, with pasta)
  succeeded. This needs a real system unit and a second user (root).
- `ProtectSystem=strict` leaves `/run/user/<uid>` read-only unless it is
  added to `ReadWritePaths`; Podman writes there. Not tested as a system
  unit.

**When the harness runs in the published container image**, neither
option nests without loosening the outer container: Podman not at all as
tested; bwrap only with `/proc` unmasked or without a `/proc`. Docker's
default seccomp profile is documented to refuse new namespaces to a process
without `CAP_SYS_ADMIN`, so under Docker bwrap may need a seccomp change
too (from its documentation; not tested, no Docker here). Landlock works
there unchanged.

## 8. Landlock inside the containers

`sandbox-exec --status` inside the design's session container (all its
flags) and inside bwrap reported `landlock Available { effective_abi: V9 },
ruleset FullyEnforced, no_new_privs true`, the granted folder was usable and
an ungranted one (the session folder) was refused. The binary needed glibc
2.43 and Debian 13 has 2.41, so inside Podman it was started with the
host's loader and libraries mounted read-only; the command it then ran was
the container's own `bash`. Landlock stacks with both, so the node's own
helpers (its shadow git, the PDF parser) are not affected by either
choice, and a container session could still run commands under Landlock as
a second layer.

## Findings along the way

1. **Podman's per-call cost is the client.** 68 to 71 ms for any command,
   `exec` or a whole new container alike, 52 MB RSS per call.
2. **pasta costs 26 MB per session** with the network on, more than
   everything else of an idle session together.
3. **bwrap per command already contains everything**, because its PID
   namespace ends with the command. Nothing a command starts survives the
   command, a cancel, or the node. That also means a background process
   cannot outlive its command (T.8's background commands must be their own
   long-running bwrap the node holds).
4. **A long-lived bwrap namespace per session is awkward and gains
   nothing.** bwrap's own `--userns`/`--pidns` joins failed here ("Setting
   pidns failed: Operation not permitted"). With `nsenter -U -p` and no new
   PID namespace, bwrap could not mount a `/proc` for that namespace and
   silently bound the host's `/proc` instead: 413 host processes visible,
   all their command lines readable. Without `-m`, bwrap read
   `/proc/<pid>` of its child from the host's `/proc`, where that number
   is another process (1 failed run in 6: "open /proc/10/ns/ns failed").
   The working shape nests a PID namespace per command, so each command
   still ends its own tree, and the session holder adds only a session-wide
   kill that per-command bwrap does not need.
5. **SIGTERM to bwrap is not a graceful stop**: bwrap does not forward it,
   and the command dies by SIGKILL. A graceful cancel has to signal inside
   (the command's own pid, from `--info-fd`) and then kill bwrap.
   SIGTERM to bwrap's init (pid 1) from outside is ignored; only SIGKILL
   ends it.
6. **Today's sandbox breaks `git` when `HOME` is the user's home and
   `~/.gitconfig` is not in `[sandbox] read`**: git exits 128 with
   "unable to access '/home/…/.gitconfig': Permission denied". The README's
   `[sandbox] read` example lists `~/.gitconfig`; a node without it has no
   working git in the shell. The timing rows above set `HOME` to the
   session folder to measure git at all.
7. **A `podman exec` command is its own session leader**, so the design's
   pgid wrapper is simpler than written (print `$$`), and a `setsid` child
   still escapes the group kill.

## What this means

### Recommendation: bubblewrap for plain sessions, Podman only for image-based environments

Of the four outcomes, the numbers support **Podman for image-based
environments with bubblewrap for plain sessions**, with bubblewrap as the
default shell confinement on Linux. Reasons:

- **Containment.** bwrap per command is the only option in which no
  detached process survived any of the four endings, including a `kill -9`
  of the node, with no cleanup code in the node. Podman exec, the design's
  form, met T.8's test only by stopping the whole container, and after a
  node `kill -9` everything kept running until a restarted node finds the
  container by label.
- **Cost.** 1.5 ms against 70 ms per call, no idle process, no 26 MB pasta
  per session, no 52 MB client per call.
- **Setup.** bwrap needs unprivileged user namespaces and the `bwrap`
  package; it works under the README unit's `NoNewPrivileges=yes` and
  without a user runtime directory. Rootless Podman needs `subuid`,
  `newuidmap` with privileges, a runtime directory, a user manager for
  cgroup limits, and a unit without `NoNewPrivileges=yes`.
- **Network off** is complete in both namespace options (TCP, UDP, DNS);
  Landlock's leaks UDP and DNS.
- **Podman is still the right tool for environments** (a pinned image, a
  toolchain the host does not have, `--memory`/`--pids-limit`, its own
  `/etc`), which bwrap does not give. For those, run each command with a
  graceful stop inside and stop the container on cancel when it must be
  certain; treat a container as disposable state.

Podman alone is the weakest of the four for plain sessions: slowest,
heaviest, does not contain without stopping the session, and does not run
under the unit the README ships. bubblewrap alone leaves out the image-based
environments the nodes design is built around.

### What it costs

| | bwrap for plain sessions | Podman for environments |
|---|---|---|
| Node owner's setup | install `bubblewrap`; on distributions that restrict unprivileged user namespaces (Ubuntu 24.04's AppArmor rule, Debian's older sysctl) allow them for `bwrap` (not tested here) | install Podman; `subuid`/`subgid` for the node user; `loginctl enable-linger` (or a runtime directory and user manager some other way); remove `NoNewPrivileges=yes` from the unit or run the node as a user service |
| Latency per call | about 1 ms over unconfined | about 68 ms over unconfined |
| Memory | none idle | about 2.7 MB PSS per idle session without network, plus about 26 MB with pasta |
| Platforms that lose the shell | macOS and Windows (no bwrap); Linux without unprivileged user namespaces; the published image unless run with `/proc` unmasked (Podman) or the Docker equivalents (untested) | the same, plus any host without rootless Podman; the published image entirely |
| Resource limits | none by itself; a `systemd-run --user --scope` per command adds a cgroup in 2.5 ms where a user manager runs | yes |

### A node without the chosen mechanism

The owner's position is that the Landlock sandbox is replaced and that a
node without a confinement refuses the shell rather than running it
unconfined. Plainly, that means:

- **The published Docker image has no agent shell** with its default run
  command. Rootless Podman does not nest in it at all as tested, and bwrap
  needs the container's `/proc` unmasked (`--security-opt unmask=ALL` in
  Podman; with Docker likely `--security-opt systempaths=unconfined` and a
  seccomp change, not tested). The README would have to say so and give
  the run flags, and those flags loosen the outer container.
- **macOS has no agent shell** on a local node: no Landlock, no bwrap, and
  Podman there runs containers in a Linux VM (`podman machine`), which was
  not tested and would put the session folder behind a VM file share.
- **A Linux node with neither** (no user namespaces, or a locked-down
  distribution) refuses agentic sessions, as `mode = "required"` does today
  without Landlock.

Landlock is the one confinement that worked in every place tested,
including inside both other options and inside a default rootless
container. Removing it for agent shells is a choice to lose the shell in
the published image; keeping it as the confinement there (with its
documented gaps: the setsid escape, host `/proc`, UDP and DNS) is the
alternative the owner should decide on explicitly.

### `docs/nodes-credentials-environments.md`, section by section

| Section | Sentence | Result |
|---|---|---|
| §2 | "Rootless Podman needs setuid `newuidmap`, `/etc/subuid` and a user runtime directory; the README's systemd unit (`NoNewPrivileges=yes`, `nologin` user) must change." | **Confirmed**, with a correction: on this machine `newuidmap` has file capabilities, not setuid; either way `no_new_privs` disables it (measured). The runtime-directory half was not reproduced. |
| §4.5 | `network = "on"  # "on" (pasta, no published ports)` | **Incomplete**: pasta's default maps the host at `169.254.1.2`; services on `0.0.0.0` or the LAN address are reachable. Add `--map-guest-addr none` to "on". |
| §4.6 | `--userns=keep-id --user <node uid>:<gid>`, "same uid inside and out", files usable by the node | **Confirmed**: 1000:1000, 644/755. Without keep-id a non-root user cannot write the folder at all. |
| §4.6 | `--cap-drop=all --security-opt=no-new-privileges --read-only --tmpfs /tmp` | **Confirmed**: no capabilities, `no_new_privs` 1, seccomp on; shell, git and gcc work. Nested user namespaces (with root mapped and `mount`) are still allowed inside. |
| §4.6 | "Before `podman run` the node checks every mount source: … no symlink in the path" | **Not enough**: Podman follows a symlinked source at run time, and a source passed as `/proc/<pid>/fd/N` is refused. Add a check after start (`stat` of the mount inside equals the pin's device and inode; measured equal) and remove the container on a mismatch. |
| §4.6 | "Killing the `podman exec` client may not kill the process inside" | **Confirmed for SIGKILL** (nothing reaches inside). SIGTERM and SIGINT are forwarded. |
| §4.6 | "the command runs under a wrapper that prints its process group id first. On cancel or timeout the node runs `podman exec <ctr> kill -TERM -<pgid>`, then `-KILL`, then stops the container (T9)." | **Contradicted as a containment**: the group kill works for the group (106 ms) but a `setsid` child survives `-TERM` and `-KILL`; only stopping the container kills it. The exec'd command is already its own group and session leader. |
| §4.6 | `--init` | **Confirmed as the mechanism**: with the init as pid 1, `podman stop -t 2` ends every process including setsid and TERM-ignoring ones. |
| §4.10.3 | "Containers are the separated path for all of it: own mount, PID and network namespaces." | **Confirmed** for files, `/proc` and network (with `none`, or pasta with the mapping off). |
| §6 | "When the harness itself runs in a container …, container execution is refused." | **Confirmed necessary**: rootless Podman does not run nested in a default rootless container. |
| §6 | "A cancel that does not reach the process inside the container would break T9; the wrapper in §4.6 handles it, with a test." | **Contradicted** for detached processes (above). |
| §8 build step 4 | "*Cancel kills `sleep` inside within 2 s." | True for a plain `sleep`; the test should use a `setsid` child, which it would fail without a container stop. |
| §9, not verified | "whether `podman exec` passes signals to the process inside" | **Settled**: TERM and INT yes, KILL no. |
| §9, not verified | "that Landlock denies `environ`, `fd` and `mem` of a sibling session's processes … and that their `cmdline` is readable" | **Partly settled**: under `sandbox-exec`, `/proc` shows every host process and their command lines are readable; the `environ` of a process outside the domain was denied. |
| §9, not verified | "whether rootless `pasta` keeps a container off the host's loopback (test D14)" | **Settled**: off 127.0.0.1, yes; off the host, no (above). |

## What was not run

- **A real system unit**: a `nologin` user without linger, `NoNewPrivileges=yes`,
  `ProtectSystem=strict`, started by the system manager. Needs root. The
  `systemd-run --user` approximation runs in a user namespace of its own,
  so its Podman failure mixed two causes; that step was removed after it
  deleted the user's `pause.pid` and left an orphaned pause process
  (recorded in `results/machine-run1.txt`).
- **Podman without a user runtime directory and user bus**, faithfully: it
  found `/run/user/1000` on its own.
- **Docker**: none on this machine. The published image under Docker, and
  Docker's seccomp and `systempaths` against bwrap, are untested.
- **Other distributions**: Ubuntu's AppArmor restriction on unprivileged
  user namespaces and Debian's defaults were not tested; whether bwrap runs
  for a node user there is unknown.
- **macOS** (`podman machine`) and Windows.
- **A release build** of `sandbox-exec`; its numbers are from a debug build.
- **IPv6** in any network test; UDP and DNS with pasta's mapping turned off.
- **Eight sessions with pasta together** (one was measured).
- **SELinux** (`relabel=private`): no SELinux here.
- **`bwrap --disable-userns`**, seccomp filters for bwrap (C.10), and
  resource limits for bwrap through `systemd-run --scope -p MemoryMax=…`
  (only the scope's latency was measured).
- **A tokio node holding `--die-with-parent`**: `PR_SET_PDEATHSIG` fires
  when the parent *thread* exits, so a command started from a short-lived
  thread would be killed early. Read, not measured; the implementation
  must spawn from a thread that lives as long as the node.
- **Podman-in-Podman with the published approaches** (a privileged outer
  container, `--userns=auto`, `/dev/fuse`): stopped after the default
  and `ignore_chown_errors` attempts failed.
- **Cold caches** and a loaded machine.
