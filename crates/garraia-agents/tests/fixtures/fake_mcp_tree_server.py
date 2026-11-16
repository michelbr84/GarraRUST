#!/usr/bin/env python3
"""Stdio MCP server that owns a process tree (issue #1913).

Unlike `fake_mcp_server.py`, this one answers `tools/call` concurrently and
launches real processes, so a test can observe what happens to the tree when
a call times out, is cancelled, or the server disconnects or crashes.

  --log PATH          append one event per line (see below)
  --descendentes A,B  long-lived descendants started at boot:
                        filho   a direct child (`sh`) ...
                        neto    ... and the grandchild it launches (`sleep`)
                        sessao  a child in a NEW session (setsid): outside the
                                server's process group on purpose
                        pipe    a child that inherits the server's stdout, so
                                the transport sees no EOF when the server dies
  --ignore-cancel     log `notifications/cancelled` but keep the work running
  --teimoso           ignore stdin EOF and SIGTERM (only SIGKILL stops it)
  --mudo              start the descendants, then never answer `initialize`

Tools:
  trabalho {segundos}  launches `sleep <segundos>`, answers "pong" when it ends
  morrer               exits immediately with status 1, descendants left behind

Log lines (space separated):
  descendente <tipo> <pid>
  inicio <request-id> <pid> <segundos>  the per-call worker started
  cancelado <request-id>         notifications/cancelled arrived
  fim <request-id> <returncode>  the per-call worker ended (and was reaped)

Deliberately dependency-free: only the stdlib.
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import threading

PROTOCOL_VERSION = "2025-06-18"

out_lock = threading.Lock()
log_lock = threading.Lock()
workers = {}
workers_lock = threading.Lock()
log_path = None


def write(msg):
    with out_lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()


def result(req_id, payload):
    write({"jsonrpc": "2.0", "id": req_id, "result": payload})


def log(*parts):
    if not log_path:
        return
    with log_lock:
        with open(log_path, "a", encoding="utf-8") as fh:
            fh.write(" ".join(str(p) for p in parts) + "\n")


def quiet():
    return {
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
    }


def start_descendants(kinds):
    if "filho" in kinds or "neto" in kinds:
        # The shell prints the grandchild's pid on a private pipe, then waits.
        sh = subprocess.Popen(
            ["sh", "-c", "sleep 600 & echo $!; wait"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        neto = sh.stdout.readline().decode().strip()
        log("descendente", "filho", sh.pid)
        log("descendente", "neto", neto)
    if "sessao" in kinds:
        proc = subprocess.Popen(["sleep", "600"], start_new_session=True, **quiet())
        log("descendente", "sessao", proc.pid)
    if "pipe" in kinds:
        # stdout NOT redirected: this child keeps the server's stdout open.
        proc = subprocess.Popen(
            ["sleep", "600"], stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        log("descendente", "pipe", proc.pid)


def run_work(req_id, seconds):
    proc = subprocess.Popen(["sleep", str(seconds)], **quiet())
    with workers_lock:
        workers[req_id] = proc
    log("inicio", req_id, proc.pid, seconds)
    code = proc.wait()
    with workers_lock:
        workers.pop(req_id, None)
    log("fim", req_id, code)
    if code == 0:
        result(req_id, {
            "content": [{"type": "text", "text": "pong"}],
            "isError": False,
        })


def main():
    global log_path
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", default="")
    ap.add_argument("--descendentes", default="")
    ap.add_argument("--ignore-cancel", action="store_true")
    ap.add_argument("--teimoso", action="store_true")
    ap.add_argument("--mudo", action="store_true")
    args = ap.parse_args()
    log_path = args.log or None

    if args.teimoso:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)

    start_descendants([k for k in args.descendentes.split(",") if k])

    if args.mudo:
        threading.Event().wait()

    tools = [
        {
            "name": "trabalho",
            "description": "Runs a worker process for a while.",
            "inputSchema": {
                "type": "object",
                "properties": {"segundos": {"type": "number"}},
            },
        },
        {
            "name": "morrer",
            "description": "Exits the server abruptly.",
            "inputSchema": {"type": "object", "properties": {}},
        },
    ]

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
        params = msg.get("params") or {}

        if method == "initialize":
            result(req_id, {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "fake-mcp-tree-server", "version": "0.1.0"},
            })
        elif method == "tools/list":
            result(req_id, {"tools": tools})
        elif method == "tools/call":
            if params.get("name") == "morrer":
                os._exit(1)
            seconds = (params.get("arguments") or {}).get("segundos", 1)
            threading.Thread(
                target=run_work, args=(req_id, seconds), daemon=True
            ).start()
        elif method == "notifications/cancelled":
            cancelled = params.get("requestId")
            log("cancelado", cancelled)
            if not args.ignore_cancel:
                with workers_lock:
                    proc = workers.get(cancelled)
                if proc is not None:
                    proc.kill()

    if args.teimoso:
        threading.Event().wait()


if __name__ == "__main__":
    main()
