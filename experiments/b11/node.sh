#!/usr/bin/env bash
# node.sh <option> <pidfile>: stands in for the node. Starts the session's
# confinement if the option has one it owns (bws), then one command that
# leaves the escapers of lib.sh behind, in its own process group as the node
# does (`process_group(0)` in crates/tools/src/bash.rs). Writes
# "node=<pid> client=<pid> holder=<pid>" to <pidfile> and waits.
# For pmexec the container rh-b11-c-$TAG must already run (conmon does not
# belong to the node in Podman's model either).
source "$(dirname "$0")/lib.sh"
opt=$1 pidfile=$2
set -m   # background jobs get their own process group
holder=
cmd=$(escapers)
# EXIT_NORMALLY=1: the command returns at once instead of waiting on its
# plain child (scenario e of contain.sh).
[ "${EXIT_NORMALLY:-0}" = 1 ] && cmd="${cmd/sleep 1000.${TAG}5/exit 0}"
case $opt in
  sbx)    sbx "$cmd" & ;;
  bw)     bw "$cmd" & ;;
  bws)    bws_start "$B11/info-$TAG.json"; holder=$BWS_PID; bws "$holder" "$cmd" & ;;
  pmexec) pm_exec "rh-b11-c-$TAG" "$cmd" & ;;
  pmrun)  mapfile -d '' a < <(pm_args yes)
          podman run --rm --name "rh-b11-r-$TAG" "${a[@]}" "$IMG" bash -c "$cmd" & ;;
esac
echo "node=$$ client=$! holder=$holder" >"$pidfile"
wait
