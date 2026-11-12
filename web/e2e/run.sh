#!/bin/sh
# Runs the browser scripts against a built binary, one at a time (they use
# fixed ports). Setup and usage: README.md, "Development".
#
#   web/e2e/run.sh                 every script
#   web/e2e/run.sh phase3 rich     the named ones
#
# BIN is the binary (default target/debug/rizzy-harness); E2E_VENV is the
# folder that holds node_modules/playwright and ms-playwright (default .venv).
# `container` runs the binary as the image does (HARNESS_CONTAINER=1,
# HARNESS_NO_LOCAL_NODE=1) on 127.0.0.1:7781; for the image itself, run
# container.cjs by hand as README.md shows.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN=$(realpath "${BIN:-$ROOT/target/debug/rizzy-harness}")
VENV=$(realpath "${E2E_VENV:-$ROOT/.venv}")
export NODE_PATH="$VENV/node_modules"
export PLAYWRIGHT_BROWSERS_PATH="$VENV/ms-playwright"
[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build -p harness, or set BIN)" >&2; exit 2; }
[ -d "$NODE_PATH/playwright" ] || { echo "no Playwright in $NODE_PATH (see README.md)" >&2; exit 2; }

if [ $# -eq 0 ]; then
  set --
  for f in "$ROOT"/web/e2e/*.cjs; do
    n=$(basename "$f" .cjs)
    [ "$n" = onboard ] || set -- "$@" "$n"
  done
fi

container() {
  data=$(mktemp -d)
  HARNESS_CONTAINER=1 HARNESS_NO_LOCAL_NODE=1 "$BIN" --data-dir "$data" serve --bind 127.0.0.1:7781 >"$data/serve.log" 2>&1 &
  pid=$!
  i=0
  until [ -s "$data/claim-code" ] && curl -sf http://127.0.0.1:7781/ >/dev/null; do
    i=$((i + 1))
    if [ $i -gt 120 ]; then echo "FAILED: the server did not start" >&2; kill "$pid"; return 1; fi
    sleep 0.5
  done
  timeout 600 node "$ROOT/web/e2e/container.cjs" http://127.0.0.1:7781/ "$(cat "$data/claim-code")"
  rc=$?
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  rm -rf "$data"
  return $rc
}

failed=""
for n in "$@"; do
  echo "== $n"
  if [ "$n" = container ]; then
    container
  else
    timeout 600 node "$ROOT/web/e2e/$n.cjs" "$BIN"
  fi || failed="$failed $n"
done
if [ -n "$failed" ]; then
  echo "failed:$failed"
  exit 1
fi
echo "all passed: $*"
