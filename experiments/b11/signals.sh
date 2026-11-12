#!/usr/bin/env bash
# Test 3, signals and exit: does a signal to the client reach the command;
# does the nodes design's `podman exec <ctr> kill -TERM -<pgid>` stop one
# command (and its setsid child) without stopping the session; are stdout,
# stderr and the exit code kept; what does a killed command report.
source "$(dirname "$0")/lib.sh"
setup_dirs
C=rh-b11-sig
trap 'pm_rm $C; kill_ours' EXIT
pm_start $C
LOG=$SA/sig.log

# The command: records its pid, pgid and sid, traps TERM and INT into the
# log, starts a setsid child, and waits.
probe() {
  cat <<EOF
echo "pid=\$\$ pgid=\$(ps -o pgid= -p \$\$ | tr -d ' ') sid=\$(ps -o sid= -p \$\$ | tr -d ' ')" > $LOG
trap 'echo got-TERM >> $LOG; exit 143' TERM
trap 'echo got-INT >> $LOG; exit 130' INT
setsid sleep 1000.${TAG}1 </dev/null >/dev/null 2>&1 &
sleep 1000.${TAG}5 & wait
EOF
}

# Job control on: a background job gets its own process group and keeps
# SIGINT (a non-interactive shell ignores SIGINT in background jobs).
set -m
client_signal() { # client_signal <option> <signal>
  local opt=$1 sig=$2 pid rc a
  rm -f "$LOG"
  # $! must be the client itself (sandbox-exec, bwrap, podman), not a
  # subshell around it: exec in the background job.
  case $opt in
    sbx)    mapfile -d '' a < <(sbx_args)
            (cd "$SA" && HOME=$SA TMPDIR=$TMPA exec "${a[@]}" bash -c "$(probe)") & ;;
    bw)     mapfile -d '' a < <(bw_args); ( exec "${a[@]}" bash -c "$(probe)" ) & ;;
    pmexec) ( exec podman exec --workdir "$SA" $C bash -c "$(probe)" ) & ;;
  esac
  pid=$!; sleep 1.5
  kill -"$sig" "$pid"; wait "$pid"; rc=$?
  sleep 1
  printf '%-7s SIG%-4s to the client: client rc=%s; log: %s; plain child %s; setsid child %s\n' \
    "$opt" "$sig" "$rc" "$(tr '\n' ' ' < "$LOG" 2>/dev/null)" "$(alive 5)" "$(alive 1)"
  kill_ours
}

echo "== a signal to the client"
for opt in sbx bw pmexec; do
  for sig in TERM INT KILL; do client_signal $opt $sig; done
done

echo "== the nodes design's stop: podman exec <ctr> kill -TERM -<pgid>, then -KILL"
rm -f "$LOG"
pm_exec $C "$(probe)" & pid=$!
sleep 1.5
cat "$LOG"
pg=$(sed -n 's/.*pgid=\([0-9]*\).*/\1/p' "$LOG")
s=$(date +%s%N)
podman exec $C kill -TERM -- "-$pg"; echo "kill -TERM -$pg rc=$?"
e=$(date +%s%N)
wait "$pid"; echo "client rc=$? after the in-container TERM ($(( (e - s) / 1000000 )) ms for the kill exec)"
sleep 1
echo "log: $(tr '\n' ' ' < "$LOG")"
echo "plain child $(alive 5); setsid child $(alive 1); container $(podman inspect -f '{{.State.Status}}' $C)"
podman exec $C kill -KILL -- "-$pg" 2>&1 | sed 's/^/kill -KILL: /'
echo "after -KILL of the group: setsid child $(alive 1)"
echo "the setsid child by session id instead (pkill -s):"
sid=$(podman exec $C sh -c "ps -eo sid,args | awk '/sleep 1000.${TAG}1/ && !/awk/ {print \$1}'")
podman exec $C pkill -KILL -s "$sid"; echo "pkill -s $sid: setsid child $(alive 1)"
echo "what remains in the container after the command:"
podman exec $C ps -eo pid,pgid,sid,args | sed 's/^/   /'
kill_ours

echo "== exit codes and streams"
for opt in sbx bw pmexec pmrun; do
  case $opt in
    sbx) run() { sbx "$1"; } ;;
    bw) run() { bw "$1"; } ;;
    pmexec) run() { pm_exec $C "$1"; } ;;
    pmrun) run() { pm_once "$1"; } ;;
  esac
  out=$(run 'echo to-stdout; echo to-stderr >&2; exit 3' 2>"$B11/err"); rc=$?
  k=$(run 'kill -KILL $$' >/dev/null 2>&1; echo $?)
  t=$(run 'kill -TERM $$' >/dev/null 2>&1; echo $?)
  printf '%-7s exit 3 -> rc=%s stdout=[%s] stderr=[%s]; self-SIGKILL -> rc=%s; self-SIGTERM -> rc=%s\n' \
    "$opt" "$rc" "$out" "$(cat "$B11/err")" "$k" "$t"
done
echo "(podman exec of a command killed by a TERM/KILL sent from inside the container, as the design's timeout does:)"
pm_exec $C "sleep 1000.${TAG}6" & pid=$!; sleep 1
podman exec $C pkill -KILL -f "sleep 1000.${TAG}6"; wait $pid; echo "podman exec client rc=$? (killed by SIGKILL inside)"
