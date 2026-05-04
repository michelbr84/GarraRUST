#!/usr/bin/env bash
# Reproducible benchmark harness for GarraIA vs OpenClaw vs ZeroClaw.
# Measures: binary size, peak RSS during `--help`, cold start of `--help`.
# Does NOT install OpenClaw globally — uses an isolated npm prefix in mktemp.
# GarraIA is built from the current checkout (HEAD); only competitor refs
# are pinned via env vars OPENCLAW_REF / ZEROCLAW_REF.

set -euo pipefail

require() {
  local cmd=$1
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required tool: $cmd" >&2
    return 1
  fi
}

# /usr/bin/time is a different binary from the shell builtin `time`.
require_gnu_time() {
  if [ ! -x /usr/bin/time ]; then
    echo "missing required tool: /usr/bin/time (GNU time, not shell builtin)" >&2
    echo "  on Debian/Ubuntu: sudo apt-get install time" >&2
    return 1
  fi
}

precheck_common() {
  local missing=0
  require git       || missing=1
  require hyperfine || missing=1
  require_gnu_time  || missing=1
  if [ "$missing" -ne 0 ]; then
    exit 64
  fi
}

precheck_garraia() { require cargo || exit 64; }
precheck_openclaw() { require npm   || exit 64; }
precheck_zeroclaw() { require cargo || exit 64; require git || exit 64; }

HOST_SHORT="$(hostname -s 2>/dev/null || echo unknown)"
DATE_FLORIDA="$(TZ=America/New_York date +%Y-%m-%d)"
DATE_DIR="results/${DATE_FLORIDA}-${HOST_SHORT}"
RAW="$DATE_DIR/raw"

mkdir -p "$RAW"

write_environment() {
  {
    echo "# run started (UTC)"
    date -u +"%Y-%m-%dT%H:%M:%SZ"
    echo "# run started (America/New_York)"
    TZ=America/New_York date +"%Y-%m-%dT%H:%M:%S%z"
    echo
    echo "# uname"
    uname -a
    echo
    echo "# /proc/cpuinfo (head)"
    if [ -r /proc/cpuinfo ]; then
      head -25 /proc/cpuinfo
    elif command -v sysctl >/dev/null 2>&1; then
      sysctl -n machdep.cpu.brand_string 2>/dev/null || true
    fi
    echo
    echo "# /proc/meminfo (head)"
    if [ -r /proc/meminfo ]; then
      head -3 /proc/meminfo
    elif command -v vm_stat >/dev/null 2>&1; then
      vm_stat | head -5
    fi
    echo
    echo "# versions"
    echo "GARRAIA_REF=$(git rev-parse HEAD)  # checkout atual"
    echo "OPENCLAW_REF=${OPENCLAW_REF:-latest}"
    echo "ZEROCLAW_REF=${ZEROCLAW_REF:-main}"
    echo "rustc=$(rustc --version 2>/dev/null || echo missing)"
    echo "cargo=$(cargo --version 2>/dev/null || echo missing)"
    echo "node=$(node --version 2>/dev/null || echo missing)"
    echo "npm=$(npm --version 2>/dev/null || echo missing)"
    echo "hyperfine=$(hyperfine --version 2>/dev/null || echo missing)"
  } > "$DATE_DIR/environment.txt"
}

run_garraia() {
  precheck_garraia
  ( cd ../.. && cargo build --release --bin garraia )
  ls -lh ../../target/release/garraia | tee "$RAW/garraia-binsize.log"
  hyperfine --warmup 3 --runs 20 \
    --export-json "$RAW/garraia-hyperfine.json" \
    '../../target/release/garraia --help' \
    | tee "$RAW/garraia-hyperfine.log"
  /usr/bin/time -v ../../target/release/garraia --help \
    2> "$RAW/garraia-time.log" || true
}

run_openclaw() {
  precheck_openclaw
  local prefix
  prefix="$(mktemp -d)/npm"
  mkdir -p "$prefix"
  export npm_config_prefix="$prefix"
  export PATH="$prefix/bin:$PATH"
  trap 'rm -rf "$(dirname "$prefix")"' EXIT
  npm install -g "openclaw@${OPENCLAW_REF:-latest}" 2>&1 \
    | tee "$RAW/openclaw-install.log"
  du -sh "$prefix/lib/node_modules/openclaw" 2>/dev/null \
    | tee "$RAW/openclaw-binsize.log" \
    || echo "openclaw not found in $prefix" | tee "$RAW/openclaw-binsize.log"
  hyperfine --warmup 3 --runs 20 \
    --export-json "$RAW/openclaw-hyperfine.json" \
    'openclaw --help' \
    | tee "$RAW/openclaw-hyperfine.log"
  /usr/bin/time -v openclaw --help \
    2> "$RAW/openclaw-time.log" || true
}

run_zeroclaw() {
  precheck_zeroclaw
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  git clone --depth 1 --branch "${ZEROCLAW_REF:-main}" \
    https://github.com/zeroclaw-labs/zeroclaw "$tmp/zeroclaw" \
    2>&1 | tee "$RAW/zeroclaw-clone.log"
  ( cd "$tmp/zeroclaw" && cargo build --release ) \
    2>&1 | tee "$RAW/zeroclaw-build.log"
  ls -lh "$tmp/zeroclaw/target/release/zeroclaw" \
    | tee "$RAW/zeroclaw-binsize.log"
  hyperfine --warmup 3 --runs 20 \
    --export-json "$RAW/zeroclaw-hyperfine.json" \
    "$tmp/zeroclaw/target/release/zeroclaw --help" \
    | tee "$RAW/zeroclaw-hyperfine.log"
  /usr/bin/time -v "$tmp/zeroclaw/target/release/zeroclaw" --help \
    2> "$RAW/zeroclaw-time.log" || true
}

precheck_common
write_environment

case "${1:-}" in
  --all)      run_garraia; run_openclaw; run_zeroclaw ;;
  --garraia)  run_garraia ;;
  --openclaw) run_openclaw ;;
  --zeroclaw) run_zeroclaw ;;
  *)          echo "usage: $0 --all | --garraia | --openclaw | --zeroclaw" >&2
              exit 64 ;;
esac

echo "done. see $DATE_DIR/"
