# Shared setup for the B.11 scripts. Source it; do not run it.
# Everything lives under $B11 (default ~/.cache/rh-b11/run); containers are
# named rh-b11-*; the image is localhost/rh-b11:tools (build.sh).
set -u
B11=${B11:-$HOME/.cache/rh-b11/run}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SBX_BIN=${SBX_BIN:-/run/media/lebron/PROJ/Rust/rizzy-harness/target/debug/rizzy-harness}
IMG=${IMG:-localhost/rh-b11:tools}
BASE=$B11/base/ws1
SA=$BASE/sessA/files      # the session's folder (read-write)
SB=$BASE/sessB/files      # a sibling session's folder (must be unreachable)
REPO=$B11/repo            # a repository (read-only)
TMPA=$B11/tmpA            # sandbox-exec's per-session TMPDIR
SECRET=$B11/secret        # stands in for ~/.ssh: must be unreachable

setup_dirs() {
  mkdir -p "$SA" "$SB" "$TMPA" "$SECRET"
  chmod 700 "$TMPA" "$SECRET"
  echo sibling-secret > "$SB/sibling.txt"
  echo host-secret > "$SECRET/id_test"
  if [ ! -d "$REPO/.git" ]; then
    mkdir -p "$REPO"
    # 3,000 small files in 60 folders, committed.
    for d in $(seq 1 60); do
      mkdir -p "$REPO/d$d"
      for f in $(seq 1 50); do echo "file $d/$f" > "$REPO/d$d/f$f.txt"; done
    done
    git -C "$REPO" init -q
    git -C "$REPO" -c user.name=b11 -c user.email=b11@invalid add -A
    git -C "$REPO" -c user.name=b11 -c user.email=b11@invalid commit -qm init
  fi
  # A small repository inside the session folder, for `git status` with
  # write access (the index refresh writes).
  if [ ! -d "$SA/work/.git" ]; then
    mkdir -p "$SA/work"
    for f in $(seq 1 200); do echo "w $f" > "$SA/work/f$f.txt"; done
    git -C "$SA/work" init -q
    git -C "$SA/work" -c user.name=b11 -c user.email=b11@invalid add -A
    git -C "$SA/work" -c user.name=b11 -c user.email=b11@invalid commit -qm init
  fi
}

id_of() { stat -c '%d:%i' "$1"; }

# today's sandbox: the node's own invocation shape (Sandbox::wrapper in
# crates/tools/src/sandbox.rs): rw = session folder + TMPDIR (by identity)
# + device files (system); ro = SYSTEM_READ (system) + the repository.
# SBX_NET=off adds --no-network.
sbx_args() {
  local a=("$SBX_BIN" sandbox-exec)
  [ "${SBX_NET:-on}" = off ] && a+=(--no-network)
  a+=(--rw "$(id_of "$SA"):the session's folder" "$SA")
  a+=(--rw "$(id_of "$TMPA"):the session's temp folder" "$TMPA")
  for d in /dev/null /dev/zero /dev/full /dev/random /dev/urandom; do a+=(--rw system "$d"); done
  for d in /usr /bin /sbin /lib /lib32 /lib64 /libx32 /etc /opt /proc; do
    [ -e "$d" ] && a+=(--ro system "$d")
  done
  a+=(--ro system "$REPO" --)
  printf '%s\0' "${a[@]}"
}
sbx() { # sbx <cmd string>
  local -a a; mapfile -d '' a < <(sbx_args)
  (cd "$SA" && TMPDIR=$TMPA "${a[@]}" bash -c "$1")
}

# bubblewrap, one namespace set per command. BW_NET=on shares the network.
bw_args() {
  local a=(bwrap --unshare-all --die-with-parent --new-session
    --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/bin /sbin
    --symlink usr/lib /lib --symlink usr/lib /lib64
    --ro-bind /etc /etc --proc /proc --dev /dev --tmpfs /tmp
    --bind "$SA" "$SA" --ro-bind "$REPO" "$REPO" --chdir "$SA"
    --setenv HOME "$SA" --setenv TMPDIR /tmp)
  [ "${BW_NET:-off}" = on ] && a+=(--share-net)
  printf '%s\0' "${a[@]}"
}
bw() { local -a a; mapfile -d '' a < <(bw_args); "${a[@]}" bash -c "$1"; }

# Podman: the nodes design's argv (docs/nodes-credentials-environments.md
# §4.6) minus what this machine cannot or need not show (memory/cpu caps
# kept; relabel dropped: no SELinux here).
pm_args() { # pm_args <keepid:yes|no>
  local a=(--userns=keep-id)
  [ "${1:-yes}" = no ] && a=()
  a+=(--cap-drop=all --security-opt=no-new-privileges
    --read-only --tmpfs /tmp:size=1g,mode=1777 --tmpfs /run:nosuid,nodev,noexec,size=64m
    --network="${PM_NET:-none}" --memory=4g --memory-swap=4g --cpus=2 --pids-limit=1024
    --ulimit core=0
    --mount "type=bind,src=$SA,dst=$SA" --mount "type=bind,src=$REPO,dst=$REPO,ro=true"
    --workdir "$SA" --env HOME="$SA" --init --pull=never --label rizzy.b11=1)
  printf '%s\0' "${a[@]}"
}
pm_start() { # pm_start <name> [keepid]
  local -a a; mapfile -d '' a < <(pm_args "${2:-yes}")
  podman run --detach --name "$1" "${a[@]}" "$IMG" sleep infinity >/dev/null
}
pm_rm() { podman rm -f -t 0 "$@" >/dev/null 2>&1 || true; }
pm_exec() { podman exec --workdir "$SA" "$1" bash -c "$2"; }
pm_once() { # podman run --rm per command
  local -a a; mapfile -d '' a < <(pm_args yes)
  podman run --rm "${a[@]}" "$IMG" bash -c "$1"
}

# A run tag: our sleeps are `sleep 1000.<tag><n>`, so the scripts find and
# kill only their own processes.
TAG=${TAG:-$(date +%s%N | tail -c 7)}
alive() { pgrep -f "sleep 1000\\.$TAG$1" >/dev/null && echo alive || echo gone; }
kill_ours() { pkill -9 -f "sleep 1000\\.$TAG" 2>/dev/null || true; }

# The four ways a command escapes its process group, as one shell string.
# 1 setsid, 2 nohup &, 3 double fork, 4 ignores SIGTERM, 5 plain child.
escapers() {
  cat <<EOF
setsid sleep 1000.${TAG}1 </dev/null >/dev/null 2>&1 &
nohup sleep 1000.${TAG}2 </dev/null >/dev/null 2>&1 &
( ( sleep 1000.${TAG}3 </dev/null >/dev/null 2>&1 & ) & )
sh -c 'trap "" TERM; exec sleep 1000.${TAG}4' </dev/null >/dev/null 2>&1 &
sleep 1000.${TAG}5
EOF
}
report_alive() { # report_alive <label>
  printf '%s setsid=%s nohup=%s dfork=%s noterm=%s plain=%s\n' "$1" \
    "$(alive 1)" "$(alive 2)" "$(alive 3)" "$(alive 4)" "$(alive 5)"
}
