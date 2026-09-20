#!/usr/bin/env python3
"""Minimal stdio MCP server used to exercise GarraIA's connection lifecycle.

Speaks just enough JSON-RPC for `initialize`, `tools/list` and `tools/call`.
Flags let a test drive the failure modes that matter:

  --crash-after-calls N  exit(1) right after answering the Nth tools/call
  --ignore-eof           keep running after stdin closes (tests bounded shutdown)
  --hang-on-call         never answer tools/call (tests the per-call timeout)
  --tool-reply TEXT      text returned by every tool (default "pong")
  --tool-reply-bytes N   tool reply of N bytes generated server-side ("x" * N).
                         Exists because Linux caps a single argv element at
                         ~128 KiB, so a 256 KiB+ payload cannot ride in
                         --tool-reply (issue #1243 fatia 1: the context cap).
  --tools A,B            comma-separated tool names to advertise (default "echo"),
                         so a test can exercise an allowlist that permits one
                         tool and blocks another
  --expose-env           add an `env_report` tool that reports the child's own
                         environment (issue #1075 continuation: pins that MCP
                         children no longer inherit the gateway's secrets).
                         Off by default so the default tool list stays at one.

Deliberately dependency-free: only the stdlib, so it runs anywhere CI runs.
"""

import argparse
import json
import os
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
    ap.add_argument("--tool-reply-bytes", type=int, default=0)
    ap.add_argument("--tools", default="echo")
    ap.add_argument("--expose-env", action="store_true")
    args = ap.parse_args()

    tools = [
        {
            "name": name,
            "description": "Echoes a fixed reply.",
            "inputSchema": {"type": "object", "properties": {}},
        }
        for name in args.tools.split(",")
        if name
    ]
    if args.expose_env:
        tools.append({
            "name": "env_report",
            "description": "Reports this child's own environment.",
            "inputSchema": {"type": "object", "properties": {}},
        })

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
            result(req_id, {"tools": tools})
        elif method == "tools/call":
            if args.hang_on_call:
                # Block forever without closing the transport.
                threading.Event().wait()
            calls += 1
            if msg.get("params", {}).get("name") == "env_report":
                # Names always; values only for the test-scoped prefix, so the
                # fixture can never print a real credential into CI logs.
                payload = {
                    "names": sorted(os.environ),
                    "values": {
                        k: v for k, v in os.environ.items()
                        if k.startswith("GARRAIA_TEST_")
                    },
                }
                result(req_id, {
                    "content": [{"type": "text", "text": json.dumps(payload)}],
                    "isError": False,
                })
                continue
            result(req_id, {
                "content": [{"type": "text",
                             "text": ("x" * args.tool_reply_bytes) if args.tool_reply_bytes
                                     else args.tool_reply}],
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
