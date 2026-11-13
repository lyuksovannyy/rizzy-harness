#!/usr/bin/env python3
"""A.19 re-check, A19-8's fix: a refused message's uploads are put back
(`Store::attachments_give_back`). What the put-back does besides.

  cap     a user holds the most unsent uploads allowed (20). A message
          that will be refused (its chat has a turn running) takes five of
          them; while the node is asked, five new uploads go in; the refusal
          puts the five back. Repeated: how many unsent uploads the user
          ends with.
  owner   the API token (which may use any upload) sends a user's upload in
          a message that is refused: whose the upload is afterwards, and
          whether its owner can still send it.
  twice   one upload named by two messages at once, one refused and one
          taken: how many chats got it.
"""
import os, signal, sqlite3, sys, threading, time, urllib.request, urllib.error
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *


def upload(s, headers, name, body=b"some text"):
    req = urllib.request.Request("http://%s/api/attachments?name=%s" % (s.addr, name), data=body, method="POST")
    req.add_header("Origin", "http://" + s.addr)
    for k, v in headers.items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def rows(data):
    db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "master.db"), uri=True)
    try:
        return db.execute("SELECT attachment_id, owner_id FROM attachments").fetchall()
    finally:
        db.close()


def main():
    root = scratch("uploads")
    data, ws = os.path.join(root, "data"), os.path.join(root, "ws")
    os.makedirs(ws, exist_ok=True)
    write_config(data, ws)
    procs = []
    try:
        s = Server(data)
        procs.append(s)
        alex = Browser.claim(s, "alex")
        h = alex._headers()
        c, v = alex.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        busy = v["session_id"]
        c, v = alex.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
        idle = v["session_id"]
        alex.call("POST", "/api/sessions/%s/prompt" % busy, {"text": "slow: 300000"})
        time.sleep(0.5)

        # ---- cap ----
        ids = []
        while True:
            c, v = upload(s, h, "f%d.txt" % len(ids))
            if c != 200:
                print("cap: upload %d is refused: %s %s" % (len(ids) + 1, c, str(v)[:90]))
                break
            ids.append(v["attachment_id"])
        print("cap: unsent uploads held: %d" % len(rows(data)))
        refused = None
        for cycle in range(12):
            batch = [r[0] for r in rows(data)][:5]
            got = {}
            def send():
                got["a"] = alex.call("POST", "/api/sessions/%s/prompt" % busy, {"text": "x", "attachments": batch})
            t = threading.Thread(target=send)
            t.start()
            n = 0
            by = time.monotonic() + 0.5
            while t.is_alive() and time.monotonic() < by:
                c, v = upload(s, h, "more%d-%d.txt" % (cycle, n))
                n += c == 200
            t.join()
            refused = got["a"]
        print("cap: each refused message answered: %s %s" % (refused[0], str(refused[1])[:70]))
        print("cap: unsent uploads held after 12 refused messages: %d (the limit is 20)" % len(rows(data)))

        # ---- owner ----
        mine = rows(data)[0]
        c, v = s.call("POST", "/api/sessions/%s/prompt" % busy, {"text": "x", "attachments": [mine[0]]})
        after = [r for r in rows(data) if r[0] == mine[0]]
        print("owner: the upload belonged to %s; the API token's refused message (%s) left it owned by %s"
              % ("alex" if mine[1] else mine[1], c, ("alex" if after[0][1] else after[0][1]) if after else "nobody (gone)"))
        c, v = alex.call("POST", "/api/sessions/%s/prompt" % idle, {"text": "slow: 5", "attachments": [mine[0]]})
        print("owner: alex then sends it: %s %s" % (c, str(v)[:80]))

        # ---- twice ----
        time.sleep(1.0)
        took = 0
        for i in range(10):
            one = [r for r in rows(data) if r[1]][0][0]
            c2, v2 = alex.call("POST", "/api/sessions", {"cwd": ws, "approval_mode": "auto"})
            fresh = v2["session_id"]
            got = {}
            ta = threading.Thread(target=lambda: got.__setitem__("refused", alex.call(
                "POST", "/api/sessions/%s/prompt" % busy, {"text": "x", "attachments": [one]})))
            tb = threading.Thread(target=lambda: got.__setitem__("taken", alex.call(
                "POST", "/api/sessions/%s/prompt" % fresh, {"text": "slow: 3", "attachments": [one]})))
            ta.start(); tb.start(); ta.join(); tb.join()
            time.sleep(0.3)
            left = [r for r in rows(data) if r[0] == one]
            if got["taken"][0] == 200 and left:
                took += 1
                print("twice: try %d: the upload went out in a chat (200) AND is still unsent" % i)
        print("twice: of 10 tries, an upload both sent and still held: %d" % took)
        s.signal(signal.SIGTERM)
        s.exited(100)
    finally:
        k = cleanup(procs, needle=root)
        print("cleanup killed: %s" % (k or "nothing"))

main()
