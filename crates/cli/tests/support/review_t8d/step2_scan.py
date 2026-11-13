# Does the master's `background_events` read scan the whole session when a
# long chat has few background events? Reproduce the exact schema and query
# (crates/store/src/lib.rs: events PK (session_id,seq) WITHOUT ROWID, no
# index on kind) and time it under growth. No product DB is touched.
import os, sqlite3, time
_t = os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "..", "target")), "review_t8d_step2")
os.makedirs(_t, exist_ok=True)
db = os.path.join(_t, "scan.sqlite")
if os.path.exists(db): os.remove(db)
c = sqlite3.connect(db)
c.execute("CREATE TABLE events (session_id TEXT NOT NULL, seq INTEGER NOT NULL, ts_ms INTEGER NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY (session_id, seq)) WITHOUT ROWID")
sid = "01a1d400-0000-7000-8000-000000000001"
Q = ("SELECT seq, ts_ms, kind, payload FROM events WHERE session_id = ?1 "
     "AND kind IN ('background_started', 'background_ended') ORDER BY seq DESC LIMIT 4096")
print("plan:", [r[-1] for r in c.execute("EXPLAIN QUERY PLAN " + Q.replace("?1", "'x'")).fetchall()])
# A chat that started 8 background commands early, then ran a long time: 8
# background_started near the front, then a huge tail of ordinary events.
seq = 0
def add(kind, n):
    global seq
    rows = []
    for _ in range(n):
        seq += 1
        rows.append((sid, seq, 0, kind, "{}"))
    c.executemany("INSERT INTO events VALUES (?,?,?,?,?)", rows)
    c.commit()
add("background_started", 8)
for tail in (10_000, 100_000, 1_000_000, 3_000_000):
    have = seq
    add("assistant_delta", tail - (have - 8))
    # Warm, then time (the connection is the master's single mirror lock).
    c.execute(Q,(sid,)).fetchall()
    t0 = time.perf_counter(); rows = c.execute(Q,(sid,)).fetchall(); dt = (time.perf_counter() - t0) * 1000
    print("tail of %9d ordinary events after 8 background ones: the stop's read took %.1f ms, returned %d rows" % (tail, dt, len(rows)))
c.close(); os.remove(db)
