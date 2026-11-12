#!/usr/bin/env bash
# Test 1, cost: start-up, per-command latency, idle memory and disk per
# session, for sandbox-exec, bubblewrap (per command and per session),
# podman exec into a session container and podman run --rm per command.
# RUNS (default 50) runs per cell after one warm-up.
source "$(dirname "$0")/lib.sh"
RUNS=${RUNS:-50}
setup_dirs
T=$HERE/timeit.py
trap 'pm_rm rh-b11-cost rh-b11-mem{1..8}; [ -n "${P:-}" ] && kill -9 $P 2>/dev/null' EXIT

cmds=("true"
  "git -C $SA/work status --porcelain"
  "git -C $REPO status --porcelain"
  "ls -R $REPO")
names=(true git-status-rw-200 git-status-ro-3000 ls-R-3000)

echo "== start-up (create the confinement), 10 runs"
python3 - "$IMG" "$SA" "$REPO" <<'EOF'
import subprocess, sys, time, statistics
img, sa, repo = sys.argv[1:4]
args = ["--userns=keep-id", "--cap-drop=all", "--security-opt=no-new-privileges",
  "--read-only", "--tmpfs", "/tmp:size=1g,mode=1777", "--network=none", "--init",
  "--pull=never", "--memory=4g", "--memory-swap=4g", "--cpus=2", "--pids-limit=1024",
  "--mount", f"type=bind,src={sa},dst={sa}", "--mount", f"type=bind,src={repo},dst={repo},ro=true",
  "--workdir", sa, "--label", "rizzy.b11=1"]
for label, net in (("podman run -d (network none)", "none"), ("podman run -d (pasta)", "pasta")):
    ts, rm = [], []
    for i in range(10):
        a = [x if not x.startswith("--network") else f"--network={net}" for x in args]
        t = time.perf_counter()
        subprocess.run(["podman", "run", "-d", "--name", f"rh-b11-start{i}", *a, img, "sleep", "infinity"],
                       check=True, stdout=subprocess.DEVNULL)
        ts.append((time.perf_counter() - t) * 1000)
        t = time.perf_counter()
        subprocess.run(["podman", "rm", "-f", "-t", "0", f"rh-b11-start{i}"], stdout=subprocess.DEVNULL)
        rm.append((time.perf_counter() - t) * 1000)
    ts.sort(); rm.sort()
    print(f"{label:<28} n=10 min={ts[0]:.0f} median={statistics.median(ts):.0f} max={ts[-1]:.0f} ms;"
          f" podman rm -f -t 0: median={statistics.median(rm):.0f} max={rm[-1]:.0f} ms")
EOF
ts=()
for i in $(seq 1 10); do
  s=$(date +%s%N); P=$(bws_start "$B11/info.json"); e=$(date +%s%N)
  # ready = a command can join it
  bws "$P" true; f=$(date +%s%N)
  ts+=("$(( (e - s) / 1000 ))") ; kill -9 "$P"; wait 2>/dev/null
done
printf 'bwrap session holder        n=10 times(us): %s\n' "${ts[*]}"
P=

echo "== per command ($RUNS runs each)"
for i in "${!cmds[@]}"; do
  c=${cmds[$i]}; n=${names[$i]}
  python3 "$T" "host-unconfined $n" "$RUNS" bash -c "cd $SA && $c"
  mapfile -d '' a < <(sbx_args)
  python3 "$T" "sandbox-exec $n" "$RUNS" env HOME="$SA" TMPDIR="$TMPA" "${a[@]}" bash -c "cd $SA && $c"
  mapfile -d '' a < <(bw_args)
  python3 "$T" "bwrap-per-cmd $n" "$RUNS" "${a[@]}" bash -c "$c"
done
P=$(bws_start "$B11/info.json")
mapfile -d '' a < <(bws_args "$P")
for i in "${!cmds[@]}"; do
  python3 "$T" "bwrap-session ${names[$i]}" "$RUNS" "${a[@]}" bash -c "${cmds[$i]}"
done
kill -9 "$P"; P=
pm_start rh-b11-cost
for i in "${!cmds[@]}"; do
  python3 "$T" "podman-exec ${names[$i]}" "$RUNS" podman exec --workdir "$SA" rh-b11-cost bash -c "${cmds[$i]}"
done
mapfile -d '' a < <(pm_args yes)
for i in 0 1; do
  python3 "$T" "podman-run-rm ${names[$i]}" 20 podman run --rm "${a[@]}" "$IMG" bash -c "${cmds[$i]}"
done

echo "== idle memory per session (kB; PSS from smaps_rollup, RSS from ps)"
mem() { # mem <label> <pids...>
  local label=$1 rss=0 pss=0 p; shift
  for p in "$@"; do
    rss=$((rss + $(ps -o rss= -p "$p" 2>/dev/null || echo 0)))
    pss=$((pss + $(awk '/^Pss:/{print $2}' /proc/"$p"/smaps_rollup 2>/dev/null || echo 0)))
  done
  printf '%-34s pids=%s RSS=%s kB PSS=%s kB\n' "$label" "$*" "$rss" "$pss"
}
podman_pids() { # conmon and its descendants
  local c=$1 q=() p
  q=("$(podman inspect -f '{{.State.ConmonPid}}' "$c")")
  while [ ${#q[@]} -gt 0 ]; do
    p=${q[0]}; q=("${q[@]:1}"); echo "$p"
    q+=($(pgrep -P "$p"))
  done
}
sleep 2
mapfile -t pp < <(podman_pids rh-b11-cost)
mem "podman session (none): conmon+ctr" "${pp[@]}"
mem "  conmon alone" "${pp[0]}"
cg=$(podman inspect -f '{{.State.CgroupPath}}' rh-b11-cost)
echo "  container cgroup memory.current: $(( $(cat /sys/fs/cgroup"$cg"/memory.current) / 1024 )) kB"
PM_NET=pasta pm_start rh-b11-mem1
sleep 2
mapfile -t pp < <(podman_pids rh-b11-mem1)
pasta=$(pgrep -f "^/usr/bin/pasta.*" -n)
mem "podman session (pasta): conmon+ctr" "${pp[@]}"
[ -n "$pasta" ] && mem "  + its pasta process" "$pasta"
pm_rm rh-b11-mem1
P=$(bws_start "$B11/info.json"); sleep 1
mem "bwrap session: bwrap x2 + sleep" "$(ps -o ppid= -p "$P" | tr -d ' ')" "$P" $(pgrep -P "$P")
kill -9 "$P"; P=
echo "sandbox-exec: nothing stays between commands (no process per session)"

echo "== 8 idle session containers (network none)"
for i in 1 2 3 4 5 6 7 8; do pm_start rh-b11-mem$i; done
sleep 2
all=()
for i in 1 2 3 4 5 6 7 8; do mapfile -t pp < <(podman_pids rh-b11-mem$i); all+=("${pp[@]}"); done
mem "8 sessions: conmon+ctr" "${all[@]}" | sed 's/pids=[0-9 ]*//'
pm_rm rh-b11-mem{1..8}

echo "== disk per session container"
podman container inspect --size -f 'SizeRw={{.SizeRw}} bytes SizeRootFs={{.SizeRootFs}} bytes' rh-b11-cost
id=$(podman inspect -f '{{.Id}}' rh-b11-cost)
du -sk "$HOME/.local/share/containers/storage/overlay-containers/$id" | awk '{print "overlay-containers/<id>: " $1 " kB"}'
upper=$(podman inspect -f '{{.GraphDriver.Data.UpperDir}}' rh-b11-cost)
du -sk "$upper" 2>/dev/null | awk '{print "upper dir: " $1 " kB"}'
du -sk "$XDG_RUNTIME_DIR/containers" 2>/dev/null | awk '{print "XDG_RUNTIME_DIR/containers (all): " $1 " kB"}'
podman image inspect -f 'image size: {{.Size}} bytes' "$IMG"
