#!/usr/bin/env python3
"""Minimal stdio MCP server used to exercise GarraIA's connection lifecycle.

Speaks just enough JSON-RPC for `initialize`, `tools/list` and `tools/call`.
Flags let a test drive the failure modes that matter:

  --crash-after-calls N  exit(1) right after answering the Nth tools/call
  --ignore-eof           keep running after stdin closes (tests bounded shutdown)
  --hang-on-call         never answer tools/call (tests the per-call timeout)
  --tool-reply TEXT      text returned by the echo tool (default "pong")

Deliberately dependency-free: only the stdlib, so it runs anywhere CI runs.
"""

import argparse
import json
import sys
import threading

PROTOCOL_VERSION = "2025-06-18"


def write(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def result(req_id, payload):
    write({"jsonrpc": "2.0", "id": req_id, "result": payload})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--crash-after-calls", type=int, default=0)
    ap.add_argument("--ignore-eof", action="store_true")
    ap.add_argument("--hang-on-call", action="store_true")
    ap.add_argument("--tool-reply", default="pong")
    args = ap.parse_args()

    calls = 0
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = msg.get("method")
        req_id = msg.get("id")

        if method == "initialize":
            result(req_id, {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "fake-mcp-server", "version": "0.1.0"},
            })
        elif method == "tools/list":
            result(req_id, {"tools": [{
                "name": "echo",
                "description": "Echoes a fixed reply.",
                "inputSchema": {"type": "object", "properties": {}},
            }]})
        elif method == "tools/call":
            if args.hang_on_call:
                # Block forever without closing the transport.
                threading.Event().wait()
            calls += 1
            result(req_id, {
                "content": [{"type": "text", "text": args.tool_reply}],
                "isError": False,
            })
            if args.crash_after_calls and calls >= args.crash_after_calls:
                sys.stdout.flush()
                sys.exit(1)
        elif req_id is not None:
            # Unknown request: answer so the client never blocks.
            result(req_id, {})

    if args.ignore_eof:
        # Simulates a server that does not exit when stdin closes; the client
        # must not wait for it forever.
        threading.Event().wait()


if __name__ == "__main__":
    main()
