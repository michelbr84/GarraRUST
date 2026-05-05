#!/usr/bin/env python3
"""Parse `cargo clippy --message-format=json` JSONL stream → warning counts.

Usage:
    cargo clippy --message-format=json | python3 parse-clippy.py
    python3 parse-clippy.py path/to/clippy.jsonl

Output JSON shape:
    {
      "clippy_warnings": 2,
      "non_clippy_warnings": 1,
      "errors": 0,
      "status": "present"
    }

Filtering: a "clippy warning" is a compiler-message with level=warning AND a
code starting with "clippy::". Other warnings (unused_variables etc.) go to
non_clippy_warnings; rust errors go to errors. clippy is fast-track CI gate
already (`cargo clippy -- -D warnings`), so non-zero clippy_warnings on PR-1
shouldn't be possible — the ratchet just observes.

Missing input file or unparseable input yields status="not_collected_this_run".
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def parse_jsonl_stream(text: str) -> dict:
    clippy_warnings = 0
    non_clippy_warnings = 0
    errors = 0

    for raw in text.splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            obj = json.loads(raw)
        except json.JSONDecodeError:
            # cargo clippy may emit non-JSON lines (e.g. final summary). Skip.
            continue
        if obj.get("reason") != "compiler-message":
            continue
        msg = obj.get("message") or {}
        level = msg.get("level")
        code = (msg.get("code") or {}).get("code") or ""
        if level == "warning":
            if code.startswith("clippy::"):
                clippy_warnings += 1
            else:
                non_clippy_warnings += 1
        elif level == "error":
            errors += 1

    return {
        "clippy_warnings": clippy_warnings,
        "non_clippy_warnings": non_clippy_warnings,
        "errors": errors,
        "status": "present",
    }


def empty_summary(reason: str) -> dict:
    return {
        "clippy_warnings": 0,
        "non_clippy_warnings": 0,
        "errors": 0,
        "status": "not_collected_this_run",
        "reason": reason,
    }


def main(argv: list[str]) -> int:
    if len(argv) > 2:
        print(f"usage: {argv[0]} [path/to/clippy.jsonl]", file=sys.stderr)
        return 2

    if len(argv) == 2:
        path = Path(argv[1])
        if not path.exists() or path.stat().st_size == 0:
            json.dump(empty_summary("file_missing"), sys.stdout, indent=2, sort_keys=True)
            print()
            return 0
        text = path.read_text(encoding="utf-8")
    else:
        text = sys.stdin.read()
        if not text.strip():
            json.dump(empty_summary("stdin_empty"), sys.stdout, indent=2, sort_keys=True)
            print()
            return 0

    summary = parse_jsonl_stream(text)
    json.dump(summary, sys.stdout, indent=2, sort_keys=True)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
