#!/bin/sh
# Fix plan 4.3 review: the real binary on a data directory in which ONE
# database carries a newer schema version. Shows the message and exit code
# the person gets, and what else in the directory the start touched before
# it refused.
#
#   real_binary_refusal.sh <path to rizzy-harness>
#
# Binds 127.0.0.1:0 only. Work folders: target/tmp/review_4_3/bin (never /tmp).
set -eu
bin=$(realpath "$1")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
work="$root/target/tmp/review_4_3/bin"
rm -rf "$work"; mkdir -p "$work/cwd"
for v in $(env | sed -n 's/^\(HARNESS_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
versions() {
    for f in master.db node.db memory.db chat.db; do
        if [ -e "$1/$f" ]; then
            printf '    %s: user_version %s, sha256 %s\n' "$f" \
                "$(python3 -c "import sqlite3,sys; print(sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True).execute('pragma user_version').fetchone()[0])" "$1/$f")" \
                "$(sha256sum "$1/$f" | cut -c1-16)"
        else
            printf '    %s: absent\n' "$f"
        fi
    done
}
for newer in master.db node.db memory.db chat.db; do
    d="$work/$newer.d"; mkdir -m 700 "$d"
    python3 -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('pragma user_version=2'); c.commit()" "$d/$newer"
    before=$(sha256sum "$d/$newer" | cut -c1-16)
    echo "=== $newer at version 2 (sha256 $before), everything else absent"
    set +e
    (cd "$work/cwd" && timeout 40 "$bin" --data-dir "$d" serve --bind 127.0.0.1:0 >"$d.out" 2>&1)
    code=$?
    set -e
    echo "  exit code: $code (124 = it did not refuse and was stopped after 40 s)"
    echo "  last lines:"; grep -v '^$' "$d.out" | tail -4 | cut -c1-600 | sed 's/^/    /'
    echo "  databases after:"; versions "$d"
    echo "  other entries: $(ls -A "$d" | grep -v '\.db' | tr '\n' ' ')"
done
