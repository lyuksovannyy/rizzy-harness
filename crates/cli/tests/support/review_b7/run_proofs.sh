#!/bin/sh
# Review of B.7: run one of the proofs in this folder. They are not part of
# the test suite: this copies the file into the crate whose dev-dependencies
# it needs for the run and removes the copy again, whatever happens.
#
#   run_proofs.sh node [test-name-filter]    real_node_stop.rs  (crates/node)
#   run_proofs.sh link [test-name-filter]    link_variants.rs   (crates/link)
#
# One cargo command. Needs bash and coreutils only.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
which=$1
shift
case "$which" in
node) src=real_node_stop.rs; crate=node; pkg=harness-node ;;
link) src=link_variants.rs; crate=link; pkg=harness-link ;;
*) echo "node or link" >&2; exit 2 ;;
esac
name=review_b7_${src%.rs}
copy="$root/crates/$crate/tests/$name.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/$src" "$copy"
cd "$root"
CARGO_BUILD_JOBS=5 cargo test -p "$pkg" --test "$name" -- \
    --nocapture --test-threads=1 "$@"
