#!/usr/bin/env bash
# Test 2, process containment. For each option and each of three kills,
# start a command that leaves behind: a `setsid` child, a `nohup … &`
# child, a double-forked child, a child that ignores SIGTERM, and a plain
# foreground child. Then:
#   a  cancel as the node does today: killpg(client, TERM), 1 s, killpg(KILL)
#   b  stop the confinement: podman stop -t 2 / kill -9 of the namespace's init
#   c  kill -9 the node (node.sh), no cleanup
#   e  nothing: the command exits normally and leaves its escapers behind
# and report which are alive after 2 s and 10 s, and what is left behind.
# OPTS (default all) and SCEN (default "a b c") narrow the run.
source "$(dirname "$0")/lib.sh"
setup_dirs
OPTS=${OPTS:-sbx bw bws pmexec pmrun}
SCEN=${SCEN:-a b c}

leftovers() {
  local c
  c=$(podman ps -a --filter label=rizzy.b11=1 --filter "name=rh-b11-[cr]-$TAG" --format '{{.Names}}:{{.Status}}' | tr '\n' ' ')
  printf '   left: containers=[%s] podman-clients=%s bwrap=%s\n' "$c" \
    "$(pgrep -fc "^podman (exec|run) .*rh-b11-[cr]-$TAG" || true)" \
    "$(pgrep -fc "^bwrap .*info-fd 3 sleep infinity" || true)"
}

for opt in $OPTS; do
  for s in $SCEN; do
    TAG=$(date +%s%N | tail -c 7); export TAG
    [ "$opt" = pmexec ] && pm_start "rh-b11-c-$TAG"
    pf=$B11/pids-$TAG
    EXIT_NORMALLY=$([ "$s" = e ] && echo 1 || echo 0) setsid bash "$HERE/node.sh" "$opt" "$pf" </dev/null >/dev/null 2>&1 &
    sleep 2
    # shellcheck disable=SC1090
    eval "$(sed 's/ /; /g' "$pf")"
    report_alive "$opt/$s before "
    case $s in
      a) kill -TERM -- "-$client" 2>/dev/null; sleep 1; kill -KILL -- "-$client" 2>/dev/null ;;
      b) case $opt in
           sbx)    echo "   $opt/b: no confinement to stop (n/a)"; kill -KILL -- "-$client" ;;
           bw)     outer=$client; [ "$(ps -o comm= -p "$client")" = bwrap ] || outer=$(pgrep -P "$client" -x bwrap)
                   inner=$(pgrep -P "$outer" -x bwrap)
                   kill -9 "$inner" ;;
           bws)    kill -9 "$holder" ;;
           pmexec) podman stop -t 2 "rh-b11-c-$TAG" >/dev/null ;;
           pmrun)  podman stop -t 2 "rh-b11-r-$TAG" >/dev/null ;;
         esac ;;
      c) kill -9 "$node" ;;
      e) : ;;
    esac
    t0=$(date +%s)
    sleep 2; report_alive "$opt/$s +2s    "
    sleep 8; report_alive "$opt/$s +10s   "
    leftovers
    # clean up: our sleeps, our clients, our containers, our holder
    kill -KILL -- "-$client" 2>/dev/null; kill -9 "$node" 2>/dev/null
    [ -n "$holder" ] && kill -9 "$holder" 2>/dev/null
    kill_ours
    pm_rm "rh-b11-c-$TAG" "rh-b11-r-$TAG"
    rm -f "$pf" "$B11/info-$TAG.json"
  done
done
