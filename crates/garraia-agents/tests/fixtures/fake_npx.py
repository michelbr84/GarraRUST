#!/usr/bin/env python3
"""Fake `npx` used to reproduce issue #1346 (corrupted npx cache).

Copied by the test into a temp dir under the exact name `npx`, so the gateway
sees an `npx` command. It mimics the part of npx that matters:

  npx -y <pkg>[@version] [args...]

runs the package from `<npm cache>/_npx/0123456789abcdef/`, where the cache is
`$npm_config_cache` or `$HOME/.npm`.

Modes (env FAKE_NPX_MODE):
  cache (default)
      entry missing          -> "install" it (package.json + the module
                                file) and exec the fake MCP server
      entry complete         -> exec the fake MCP server
      entry without module   -> print the real Node ERR_MODULE_NOT_FOUND trace
                                for zod/v4/mini/external.js, exit 1
                                (a half-installed entry: the field case)
  point-at
      always print that trace naming $FAKE_NPX_POINT_AT as the entry, exit 1
      (a hostile/buggy child pointing the gateway at some other directory)
  fail
      print an unrelated error, exit 1

Every invocation appends one line to $FAKE_NPX_LOG when set, so a test can
count spawns. The MCP server itself is $FAKE_MCP_SERVER (fake_mcp_server.py).
Stdlib only.
"""

import json
import os
import sys

HASH = "0123456789abcdef"
MODULE = os.path.join("node_modules", "zod", "v4", "mini", "external.js")


def package_name(spec):
    if spec.startswith("@"):
        at = spec.find("@", 1)
        return spec if at < 0 else spec[:at]
    return spec.split("@")[0]


def trace(entry):
    missing = os.path.join(entry, MODULE)
    importer = os.path.join(entry, "node_modules", "zod", "v4", "mini", "index.js")
    sys.stderr.write(
        "node:internal/modules/esm/resolve:275\n"
        "    throw new ERR_MODULE_NOT_FOUND(\n"
        "          ^\n\n"
        f"Error [ERR_MODULE_NOT_FOUND]: Cannot find module '{missing}' imported from {importer}\n"
        "    at finalizeResolution (node:internal/modules/esm/resolve:275:11)\n"
        "    at moduleResolve (node:internal/modules/esm/resolve:861:10) {\n"
        "  code: 'ERR_MODULE_NOT_FOUND',\n"
        f"  url: 'file://{missing}'\n"
        "}\n\nNode.js v22.22.3\n"
    )
    sys.stderr.flush()
    sys.exit(1)


def main():
    log = os.environ.get("FAKE_NPX_LOG")
    if log:
        with open(log, "a") as f:
            f.write(" ".join(sys.argv[1:]) + "\n")

    positional = [a for a in sys.argv[1:] if not a.startswith("-")]
    if not positional:
        sys.stderr.write("npx: missing package\n")
        sys.exit(1)
    pkg = package_name(positional[0])

    mode = os.environ.get("FAKE_NPX_MODE", "cache")
    if mode == "fail":
        sys.stderr.write("Error: something unrelated went wrong\n")
        sys.exit(1)
    if mode == "point-at":
        trace(os.environ["FAKE_NPX_POINT_AT"])

    cache = os.environ.get("npm_config_cache") or os.path.join(os.environ["HOME"], ".npm")
    entry = os.path.join(cache, "_npx", HASH)
    if os.path.isdir(entry):
        if not os.path.isfile(os.path.join(entry, MODULE)):
            trace(entry)
    else:
        os.makedirs(os.path.dirname(os.path.join(entry, MODULE)))
        with open(os.path.join(entry, "package.json"), "w") as f:
            json.dump({"dependencies": {pkg: "^2026.8.31"}}, f)
        with open(os.path.join(entry, MODULE), "w") as f:
            f.write("export {};\n")

    server = os.environ["FAKE_MCP_SERVER"]
    os.execv(sys.executable, [sys.executable, server])


if __name__ == "__main__":
    main()
