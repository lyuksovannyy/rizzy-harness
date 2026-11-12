# Experiment B.11: containers as the confinement for agent shells

Shell scripts that measure rootless Podman, bubblewrap and today's
`sandbox-exec` (Landlock) with the same tests. The write-up is
[`docs/container-experiment.md`](../../docs/container-experiment.md); the raw
output of the run it reports is in `results/` (home folder shown as `~`).

## Needs

Linux with rootless Podman (`subuid`/`subgid` for the user), `bwrap`,
`python3`, `git`, `gcc`, `setpriv`, `nsenter`, and a built harness binary
(`SBX_BIN`, default `target/debug/rizzy-harness` of the main checkout). No
root. The scripts are bash; run them with bash, not zsh.

## Run

```bash
cd experiments/b11
podman build --pull=never -t localhost/rh-b11:tools -f Containerfile .   # once; needs debian:trixie-slim pulled
./cost.sh      # test 1: start-up, per command (RUNS=50), idle memory, disk
./contain.sh   # test 2: which detached processes survive which kill (OPTS, SCEN narrow it)
./signals.sh   # test 3: signals to the client, the design's in-container kill, exit codes
./files.sh     # test 4: reach outside the folder, ownership, symlinks, swapped mount sources
./net.sh       # test 5: no network and pasta (the design's test D14); one DNS lookup and one HTTPS HEAD to example.com
./priv.sh      # test 6: capabilities, ptrace, nested user namespaces, ordinary work under the flags
./machine.sh   # tests 7 and 8: no_new_privs, no runtime directory, nesting, Landlock inside Podman and bwrap
```

`lib.sh` holds the folder layout and the invocations (`sbx`, `bw`, `bws_*`,
`pm_*`); `node.sh` stands in for the node in `contain.sh`; `timeit.py` and
`netprobe.py` are helpers.

## What they leave behind

- `~/.cache/rh-b11/run/`: the test folders (a 3,000-file repository, a
  session folder, a sibling, a stand-in secret), `debian.tar` and a second
  Podman store `altroot/` holding only the Debian base image (machine.sh).
  Remove with `podman unshare rm -rf ~/.cache/rh-b11` (the store holds files
  owned by subordinate uids).
- The images `localhost/rh-b11:tools` and `docker.io/library/debian:trixie-slim`
  in the user's Podman store. Remove with `podman rmi`.
- Containers are named `rh-b11-*`, labelled `rizzy.b11=1`, and removed by
  each script's exit trap. If a script is killed, `podman rm -f $(podman ps
  -aq --filter label=rizzy.b11=1)` removes the rest.
- Test processes are `sleep 1000.<tag>`; the scripts kill only those.
- The first version of `machine.sh` ran Podman inside a `systemd-run --user`
  unit; that removed the user's `pause.pid` and left an orphaned
  `catatonit -P` pause process. The step is no longer run.
