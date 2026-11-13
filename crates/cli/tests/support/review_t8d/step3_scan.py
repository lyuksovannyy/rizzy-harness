#!/usr/bin/env python3
"""T.8d step 3 review (S3-3): what one check of the callback watcher reads
from the master's mirror, under the mirror's one lock.

`Store::events_since_last_turn` (crates/store/src/lib.rs) is two statements:
the floor (the last `turn_started` or `callback_refused`), then
`events_after(sid, floor, usize::MAX >> 1)`: every event after the floor with
its payload, each payload parsed as JSON, and only then filtered to
`background_ended` in Rust. A check runs 300 ms after every end that may call
back, again every 15 s while the node is away, busy or stopping (up to 60
minutes), and once at the master's start for every chat whose log moved in
the last 65 minutes, with or without a background command.

Here: the same schema and the same two statements on a scratch database (no
product database is touched), for a chat whose last turn was long (the turn
that started the command), against the same read with the kinds in the SQL.
"""
import json, os, sqlite3, time

_t = os.path.join(os.environ.get("T8D_TARGET", os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..", "..", "target"))), "review_t8d_step3")
os.makedirs(_t, exist_ok=True)
db = os.path.join(_t, "scan-%d.sqlite" % os.getpid())
c = sqlite3.connect(db)
c.execute("CREATE TABLE events (session_id TEXT NOT NULL, seq INTEGER NOT NULL, ts_ms INTEGER NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY (session_id, seq)) WITHOUT ROWID")
FLOOR = "SELECT COALESCE(MAX(seq), 0) FROM events WHERE session_id = ?1 AND kind IN ('turn_started', 'callback_refused')"
AFTER = "SELECT seq, ts_ms, kind, payload FROM events WHERE session_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"
KINDS = "SELECT seq, ts_ms, kind, payload FROM events WHERE session_id = ?1 AND seq > ?2 AND kind = 'background_ended' ORDER BY seq"
print("plan of the floor:", [r[-1] for r in c.execute("EXPLAIN QUERY PLAN " + FLOOR.replace("?1", "'x'")).fetchall()])


def fill(sid, steps, out_kib, deltas):
    """One turn: `steps` tool calls of `out_kib` KiB of output, `deltas`
    stream events a step, then the turn's end and one background end."""
    seq, rows = 0, []
    def add(kind, payload):
        nonlocal seq
        seq += 1
        rows.append((sid, seq, 0, kind, json.dumps(payload)))
    add("session_created", {})
    add("turn_started", {"turn_id": "t"})
    add("user_message", {"turn_id": "t", "text": "build it"})
    out = "x" * (out_kib * 1024)
    for i in range(steps):
        for _ in range(deltas):
            add("tool_output_delta", {"call_id": "c%d" % i, "text": "x" * 200})
        add("tool_call_finished", {"call_id": "c%d" % i, "ok": True, "output": out})
    add("turn_ended", {"turn_id": "t", "reason": "completed"})
    add("background_ended", {"bg_id": "bg-1", "reason": "exited", "confirmed": "ended", "ran_ms": 1, "output_bytes": 0})
    c.executemany("INSERT INTO events VALUES (?,?,?,?,?)", rows)
    c.commit()
    return seq


def check(sid, q_after):
    t0 = time.perf_counter()
    floor = c.execute(FLOOR, (sid,)).fetchone()[0]
    rows = c.execute(q_after, (sid, floor, 2**62) if "LIMIT" in q_after else (sid, floor)).fetchall()
    read = sum(len(r[3]) for r in rows)
    parsed = [json.loads(r[3]) for r in rows]
    kept = [r for r in rows if r[2] == "background_ended"]
    return (time.perf_counter() - t0) * 1000, len(rows), read, len(kept)


for i, (steps, kib, deltas) in enumerate([(20, 8, 10), (200, 32, 40), (600, 64, 60)]):
    sid = "01a1d400-0000-7000-8000-00000000000%d" % i
    n = fill(sid, steps, kib, deltas)
    check(sid, AFTER)
    ms, rows, read, kept = check(sid, AFTER)
    ms2, rows2, read2, kept2 = check(sid, KINDS)
    print("a last turn of %3d steps (%2d KiB a result, %d stream events a step; %d events): "
          "one check reads %d rows, %.1f MiB of payload, parses them, keeps %d: %.0f ms; "
          "with the kind in the SQL: %d row, %d bytes, %.1f ms"
          % (steps, kib, deltas, n, rows, read / 2**20, kept, ms, rows2, read2, ms2))
    print("    while its node is away for an hour: 240 checks, %.1f GiB read under the mirror's lock for this one chat"
          % (240 * read / 2**30))
c.close()
os.remove(db)
