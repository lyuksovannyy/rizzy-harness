#!/usr/bin/env python3
"""A tiny MCP server over stdio for the harness's tests (ADOPTION_PLAN X.3).

Newline-delimited JSON-RPC 2.0: answers `initialize`, `ping`, `tools/list`
(paged when asked) and `tools/call`; ignores notifications; answers any
other request with "method not found". Flags:

  --tools N        N more tools, t0 .. t(N-1)
  --big-schema     one more tool whose input schema is over 8,000 bytes
  --page N         list N tools per page (with a cursor)
  --hang-init      never answer `initialize`
  --slow-init MS   answer `initialize` after MS milliseconds
  --started PATH   append "started <pid>" to PATH when it starts
  --detach         leave its process group and session at once (`setsid`), so
                   that a stop of the group the harness started does not
                   reach it: it keeps answering until its input closes
  --die-init MS    exit 1 after MS milliseconds, during `initialize`
  --calls PATH     append "call <tool>" to PATH for every `tools/call` it
                   receives (before it answers)
  --marker WORD    ignored (lets a test find the process in /proc)
  --say-env NAME   print "token <value of NAME>" to stderr and exit 1 at once
  --tool-names A,B the tools are these (echo's behaviour), no others
  --linger SEC     after its input ends, keep running SEC seconds (a server
                   that does not stop with its pipe)

Tools: echo, env, hang, huge, flood, die, image, fail, pid, cwd, slow
(answers after `ms` milliseconds from a thread of its own, so answers can
come in another order than the calls).
"""

import json
import os
import sys
import threading
import time

args = sys.argv[1:]


def flag(name):
    return name in args


def value(name, default=None):
    if name in args:
        return args[args.index(name) + 1]
    return default


if flag("--detach"):
    os.setsid()

if value("--started"):
    with open(value("--started"), "a") as f:
        f.write("started %d\n" % os.getpid())

if value("--say-env"):
    sys.stderr.write("auth failed for token %s\n" % os.environ.get(value("--say-env"), ""))
    sys.stderr.flush()
    sys.exit(1)

out = sys.stdout.buffer


LOCK = threading.Lock()


def send(msg):
    with LOCK:
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
    {"name": "cwd", "description": "Its working folder.", "inputSchema": obj()},
    {
        "name": "slow",
        "description": "Repeat the text after ms milliseconds.",
        "inputSchema": obj({"text": {"type": "string"}, "ms": {"type": "integer"}}),
    },
]
for i in range(int(value("--tools", "0"))):
    TOOLS.append({"name": "t%d" % i, "description": "Tool %d." % i, "inputSchema": obj()})
if value("--tool-names"):
    TOOLS = [
        {"name": n, "description": "Named.", "inputSchema": obj({"text": {"type": "string"}})}
        for n in value("--tool-names").split(",")
    ]
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
    if value("--tool-names"):
        return text("served by %s tool %s" % (value("--marker", "?"), name))
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
    if name == "cwd":
        return text("cwd=%s" % os.getcwd())
    if name == "pid":
        return text("pid=%d ns=%s" % (os.getpid(), os.readlink("/proc/self/ns/pid")))
    return text("no tool " + name, error=True)


page = int(value("--page", "0"))


def log_call(params):
    with open(value("--calls"), "a") as f:
        f.write("call %s\n" % params.get("name"))

for line in sys.stdin.buffer:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    if "id" not in msg or "method" not in msg:
        continue  # a notification, or an answer to us
    mid, method, params = msg["id"], msg["method"], msg.get("params") or {}
    if method == "tools/call" and value("--calls"):
        log_call(params)
    if method == "initialize":
        if flag("--hang-init"):
            continue
        if value("--die-init"):
            time.sleep(int(value("--die-init")) / 1000.0)
            sys.exit(1)
        if value("--slow-init"):
            time.sleep(int(value("--slow-init")) / 1000.0)
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
    elif method == "tools/call" and params.get("name") == "slow":
        a = params.get("arguments") or {}

        def later(mid=mid, a=a):
            time.sleep(int(a.get("ms", 0)) / 1000.0)
            send({"jsonrpc": "2.0", "id": mid, "result": text(str(a.get("text", "")))})

        threading.Thread(target=later, daemon=True).start()
    elif method == "tools/call":
        r = call(params.get("name"), params.get("arguments") or {})
        if r is not None:
            send({"jsonrpc": "2.0", "id": mid, "result": r})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})

if value("--linger"):
    time.sleep(float(value("--linger")))
