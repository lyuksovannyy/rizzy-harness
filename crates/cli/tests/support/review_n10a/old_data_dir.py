#!/usr/bin/env python3
"""L-h, "rows created before the column existed": what a data directory made
before `sessions.exec_mode` does on this binary. The store has no migration
and no schema version (`CREATE TABLE IF NOT EXISTS`), so the column is simply
not there. Made here by dropping the column from a directory this binary
wrote (master.db, node.db, chat.db), which is what an older binary leaves.
"""
import os, signal, sqlite3, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from n10a_common import *


def main():
    root = scratch("old_data")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        sid = s.session(ws)
        created(s, sid)
        s.signal(signal.SIGTERM)
        if s.exited(30) is None:
            s.kill()
        print("sqlite %s" % sqlite3.sqlite_version)
        for db in ("master.db", "node.db", "chat.db"):
            c = sqlite3.connect(os.path.join(data, db))
            c.execute("ALTER TABLE sessions DROP COLUMN exec_mode")
            c.commit()
            c.close()
        try:
            s2 = Server(data)
        except RuntimeError as e:
            print("the server does not start on it:\n   " + "\n   ".join(str(e).splitlines()[-6:]))
            return
        procs.append(s2)
        print("the server starts on it (no message about the schema: %s)"
              % (not any("exec_mode" in l or "schema" in l for _, l in s2.err)))
        for name, method, path, body in [
            ("list chats", "GET", "/api/sessions", None),
            ("open the chat", "GET", "/api/sessions/%s" % sid, None),
            ("prompt it", "POST", "/api/sessions/%s/prompt" % sid, {"text": "hello"}),
            ("a new chat", "POST", "/api/sessions", {"cwd": ws}),
        ]:
            c, v = s2.call(method, path, body)
            print("  %-14s %s %s" % (name, c, short(v, 130)))
        time.sleep(1)
        errs = [l for _, l in s2.err if "exec_mode" in l]
        print("  log lines naming the column: %d%s" % (len(errs), "" if not errs else "; e.g. " + short(errs[0], 200)))
    finally:
        left = cleanup(procs, root)
        if left:
            print("  (cleanup ended %d leftover processes of this run)" % len(left))


if __name__ == "__main__":
    main()
