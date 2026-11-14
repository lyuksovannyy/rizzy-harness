#!/usr/bin/env python3
"""A tiny MCP server over stdio for the harness's tests (ADOPTION_PLAN X.3).

Newline-delimited JSON-RPC 2.0: answers `initialize`, `ping`, `tools/list`
(paged when asked) and `tools/call`; ignores notifications; answers any
other request with "method not found". Flags:

  --tools N        N more tools, t0 .. t(N-1)
  --big-schema     one more tool whose input schema is over 8,000 bytes
  --page N         list N tools per page (with a cursor)
  --hang-init      never answer `initialize`
  --started PATH   append "started <pid>" to PATH when it starts
  --marker WORD    ignored (lets a test find the process in /proc)

Tools: echo, env, hang, huge, flood, die, image, fail, pid.
"""

import json
import os
import sys

args = sys.argv[1:]


def flag(name):
    return name in args


def value(name, default=None):
    if name in args:
        return args[args.index(name) + 1]
    return default


if value("--started"):
    with open(value("--started"), "a") as f:
        f.write("started %d\n" % os.getpid())

out = sys.stdout.buffer


def send(msg):
    out.write(json.dumps(msg).encode() + b"\n")
    out.flush()


def obj(props=None):
    return {"type": "object", "properties": props or {}}


TOOLS = [
    {
        "name": "echo",
        "description": "Repeat the text.",
        "inputSchema": obj({"text": {"type": "string"}}),
        # A server's claim; the harness must not believe it.
        "annotations": {"readOnlyHint": True},
    },
    {"name": "env", "description": "Print the environment.", "inputSchema": obj()},
    {"name": "hang", "description": "Never answer.", "inputSchema": obj()},
    {"name": "huge", "description": "A very long result.", "inputSchema": obj()},
    {"name": "flood", "description": "A message over the cap.", "inputSchema": obj()},
    {"name": "die", "description": "Exit.", "inputSchema": obj()},
    {"name": "image", "description": "An image.", "inputSchema": obj()},
    {"name": "fail", "description": "An error result.", "inputSchema": obj()},
    {"name": "pid", "description": "Its process.", "inputSchema": obj()},
]
for i in range(int(value("--tools", "0"))):
    TOOLS.append({"name": "t%d" % i, "description": "Tool %d." % i, "inputSchema": obj()})
if flag("--big-schema"):
    TOOLS.append(
        {
            "name": "big",
            "description": "Too big.",
            "inputSchema": obj({"p%d" % i: {"type": "string", "description": "x" * 50} for i in range(200)}),
        }
    )


def text(t, error=False):
    r = {"content": [{"type": "text", "text": t}]}
    if error:
        r["isError"] = True
    return r


def call(name, arguments):
    if name == "echo":
        return text(str(arguments.get("text", "")))
    if name == "env":
        return text("\n".join("%s=%s" % kv for kv in sorted(os.environ.items())))
    if name == "hang":
        return None
    if name == "huge":
        return text("h" * 1_000_000)
    if name == "flood":
        out.write(b'{"jsonrpc":"2.0","id":0,"result":"' + b"f" * (9 << 20))
        out.flush()
        return None
    if name == "die":
        sys.stderr.write("fake_mcp: told to die\n")
        sys.stderr.flush()
        os._exit(3)
    if name == "image":
        return {"content": [{"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"}]}
    if name == "fail":
        return text("it went wrong", error=True)
    if name == "pid":
        return text("pid=%d ns=%s" % (os.getpid(), os.readlink("/proc/self/ns/pid")))
    return text("no tool " + name, error=True)


page = int(value("--page", "0"))

for line in sys.stdin.buffer:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    if "id" not in msg or "method" not in msg:
        continue  # a notification, or an answer to us
    mid, method, params = msg["id"], msg["method"], msg.get("params") or {}
    if method == "initialize":
        if flag("--hang-init"):
            continue
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "protocolVersion": params.get("protocolVersion", "2025-06-18"),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fake", "version": "0"},
                },
            }
        )
    elif method == "ping":
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "tools/list":
        if page:
            start = int(params.get("cursor") or 0)
            result = {"tools": TOOLS[start : start + page]}
            if start + page < len(TOOLS):
                result["nextCursor"] = str(start + page)
        else:
            result = {"tools": TOOLS}
        send({"jsonrpc": "2.0", "id": mid, "result": result})
    elif method == "tools/call":
        r = call(params.get("name"), params.get("arguments") or {})
        if r is not None:
            send({"jsonrpc": "2.0", "id": mid, "result": r})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
