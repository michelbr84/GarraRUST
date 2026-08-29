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
    echo "ZEROCLAW_REF=${ZEROCLAW_REF:-<default branch>}"
    echo "rustc=$(rustc --version 2>/dev/null || echo missing)"
    echo "cargo=$(cargo --version 2>/dev/null || echo missing)"
    echo "node=$(node --version 2>/dev/null || echo missing)"
    echo "npm=$(npm --version 2>/dev/null || echo missing)"
    echo "hyperfine=$(hyperfine --version 2>/dev/null || echo missing)"
  } > "$DATE_DIR/environment.txt"
}

run_garraia() {
  precheck_garraia
  # O pacote é `garraia`, mas o binário que ele produz chama-se `garra`
  # ([[bin]] name em crates/garraia-cli/Cargo.toml) — `--bin garraia` não
  # existe e `target/release/garraia` nunca é gerado.
  ( cd ../.. && cargo build --release -p garraia )
  ls -lh ../../target/release/garra | tee "$RAW/garraia-binsize.log"
  hyperfine --warmup 3 --runs 20 \
    --export-json "$RAW/garraia-hyperfine.json" \
    '../../target/release/garra --help' \
    | tee "$RAW/garraia-hyperfine.log"
  /usr/bin/time -v ../../target/release/garra --help \
    2> "$RAW/garraia-time.log" || true
}

run_openclaw() {
  precheck_openclaw
  local prefix
  prefix="$(mktemp -d)/npm"
  mkdir -p "$prefix"
  export npm_config_prefix="$prefix"
  export PATH="$prefix/bin:$PATH"
  # Aspas duplas de propósito: expandir $prefix AGORA. Com aspas simples o
  # trap roda no EXIT do script, quando a local já saiu de escopo — sob
  # `set -u` isso vira "prefix: unbound variable" e o temp dir vaza.
  trap "rm -rf '$(dirname "$prefix")'" EXIT
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
  # Mesma razão do run_openclaw: expandir $tmp no set do trap, não no EXIT.
  trap "rm -rf '$tmp'" EXIT
  # Sem ZEROCLAW_REF, clona o branch DEFAULT do upstream (que é `master`,
  # não `main` — `--branch main` fatalizava o clone).
  if [ -n "${ZEROCLAW_REF:-}" ]; then
    git clone --depth 1 --branch "$ZEROCLAW_REF" \
      https://github.com/zeroclaw-labs/zeroclaw "$tmp/zeroclaw" \
      2>&1 | tee "$RAW/zeroclaw-clone.log"
  else
    git clone --depth 1 \
      https://github.com/zeroclaw-labs/zeroclaw "$tmp/zeroclaw" \
      2>&1 | tee "$RAW/zeroclaw-clone.log"
  fi
  git -C "$tmp/zeroclaw" rev-parse HEAD | tee -a "$RAW/zeroclaw-clone.log"
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

# ---------------------------------------------------------------------------
# Cenários 004/005 — auditoria de segurança por inspeção de checkouts pinados.
# Método claude-stack-lab (claims-table): cada claim vira {target, name, ok,
# detail} verificado mecanicamente, com a saída bruta preservada em raw/.
# Declarações: scenarios/004-credentials-at-rest.md, scenarios/005-attack-surface.md.
# Checkouts dos concorrentes via env: OPENCLAW_CHECKOUT / ZEROCLAW_CHECKOUT
# (ausente => claims daquele target ficam "skipped", nunca inventados).
# ---------------------------------------------------------------------------

GARRAIA_ROOT="$(cd ../.. && pwd)"
SCEN_DIR=""
CLAIMS_TSV=""

start_scenario() {
  SCEN_DIR="$DATE_DIR/$1"
  mkdir -p "$SCEN_DIR/raw"
  CLAIMS_TSV="$(mktemp)"
  {
    echo "# pinned checkouts ($1)"
    echo "GARRAIA=$(git -C "$GARRAIA_ROOT" rev-parse HEAD)"
    [ -d "${OPENCLAW_CHECKOUT:-}" ] && echo "OPENCLAW=$(git -C "$OPENCLAW_CHECKOUT" rev-parse HEAD)"
    [ -d "${ZEROCLAW_CHECKOUT:-}" ] && echo "ZEROCLAW=$(git -C "$ZEROCLAW_CHECKOUT" rev-parse HEAD)"
  } >> "$DATE_DIR/environment.txt"
}

# claim <target> <workdir> <mode: present|zero|number> <slug> <name> <command>
claim() {
  local target=$1 workdir=$2 mode=$3 slug=$4 name=$5 cmd=$6
  local out ok
  if [ -z "$workdir" ] || [ ! -d "$workdir" ]; then
    printf '%s\t%s\tskipped\tcheckout not provided\n' "$target" "$name" >> "$CLAIMS_TSV"
    return 0
  fi
  out="$( (cd "$workdir" && bash -c "$cmd") 2>&1 || true)"
  printf '%s\n' "$out" > "$SCEN_DIR/raw/${slug}.txt"
  case "$mode" in
    present) [ -n "$out" ] && ok=pass || ok=fail ;;
    zero)    [ "$out" = "0" ] && ok=pass || ok=fail ;;
    number)  printf '%s' "$out" | grep -qE '^[0-9]+$' && ok=pass || ok=fail ;;
  esac
  printf '%s\t%s\t%s\t%s\n' "$target" "$name" "$ok" \
    "$(printf '%s' "$out" | head -1 | cut -c1-200)" >> "$CLAIMS_TSV"
}

finish_scenario() {
  python3 - "$1" "$SCEN_DIR" "$CLAIMS_TSV" <<'PYEOF'
import sys, json, datetime, socket
scenario, scen_dir, tsv = sys.argv[1], sys.argv[2], sys.argv[3]
claims = []
for line in open(tsv):
    target, name, ok, detail = line.rstrip("\n").split("\t", 3)
    claims.append({"target": target, "name": name, "ok": ok, "detail": detail})
status = "fail" if any(c["ok"] == "fail" for c in claims) else "pass"
summary = {
    "scenario": scenario,
    "run_at_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "host": socket.gethostname(),
    "status": status,
    "skipped": sum(1 for c in claims if c["ok"] == "skipped"),
    "claims": claims,
}
with open(f"{scen_dir}/summary.json", "w") as f:
    json.dump(summary, f, ensure_ascii=False, indent=2)
print(f"{scenario}: {status} ({len(claims)} claims, {summary['skipped']} skipped) -> {scen_dir}/summary.json")
PYEOF
  rm -f "$CLAIMS_TSV"
}

run_scenario_004() {
  require python3 || exit 64
  start_scenario "004-credentials-at-rest"
  claim garraia  "$GARRAIA_ROOT" present garraia-aes256gcm \
    "Credential vault uses AES-256-GCM" \
    "grep -n 'AES_256_GCM' crates/garraia-security/src/credentials.rs"
  claim garraia  "$GARRAIA_ROOT" present garraia-pbkdf2 \
    "Vault key derived with PBKDF2 600k iterations" \
    "grep -n 'PBKDF2_ITERATIONS' crates/garraia-security/src/credentials.rs"
  claim garraia  "$GARRAIA_ROOT" present garraia-mcp-plaintext-caveat \
    "Honest caveat: MCP secrets fall back to plaintext with warning when vault passphrase is absent" \
    "grep -rn 'plaintext' crates/garraia-gateway/src/mcp/persistence.rs"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" present openclaw-not-encrypted \
    "Docs state secret store is not encrypted at rest (POSIX perms only)" \
    "grep -n 'not encrypted at rest' docs/gateway/secrets.md"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" present openclaw-secretrefs \
    "Fairness: SecretRefs + 1Password/Vault integrations exist as opt-in" \
    "grep -n 'SecretRefs' docs/gateway/secrets.md && ls -d extensions/onepassword extensions/vault"
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" present zeroclaw-chacha \
    "Secrets encrypted at rest by default (ChaCha20-Poly1305)" \
    "grep -n 'ChaCha20-Poly1305' crates/zeroclaw-config/src/secrets.rs"
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" zero zeroclaw-no-keyring \
    "Honest caveat: no OS keyring crate — master key on same filesystem as ciphertext" \
    "grep -c '^name = \"keyring\"' Cargo.lock || true"
  finish_scenario "004-credentials-at-rest"
}

run_scenario_005() {
  require python3 || exit 64
  start_scenario "005-attack-surface"
  claim garraia  "$GARRAIA_ROOT" present garraia-bind \
    "Default bind is 127.0.0.1" \
    "grep -n '127\\.0\\.0\\.1' crates/garraia-config/src/model.rs"
  claim garraia  "$GARRAIA_ROOT" present garraia-api-open-caveat \
    "Honest caveat: local API auth is opt-in (session_tokens_required default false)" \
    "grep -n 'session_tokens_required' crates/garraia-config/src/model.rs"
  claim garraia  "$GARRAIA_ROOT" present garraia-pairing \
    "Messaging channels are deny-by-default (restricted allowlist + pairing)" \
    "grep -rn 'restricted' crates/garraia-security/src/allowlist.rs"
  claim garraia  "$GARRAIA_ROOT" number garraia-lock-crates \
    "Resolved crates in Cargo.lock" \
    "grep -c '^name = ' Cargo.lock"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" present openclaw-loopback \
    "Default bind mode is loopback" \
    "grep -n 'loopback' src/gateway/net.ts"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" present openclaw-token-failclosed \
    "Gateway auth defaults to token mode and fails closed without a token" \
    "grep -n 'no token was configured' src/gateway/auth.ts"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" number openclaw-prod-deps \
    "Direct production dependencies in package.json" \
    "python3 -c \"import json;print(len(json.load(open('package.json'))['dependencies']))\""
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" present zeroclaw-pairing \
    "Pairing required by default (require_pairing: true)" \
    "grep -n 'require_pairing' crates/zeroclaw-config/src/schema.rs"
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" present zeroclaw-public-warn \
    "Honest caveat: public bind is warn-only (allow_public_bind), not refused" \
    "grep -n 'allow_public_bind' crates/zeroclaw-config/src/schema.rs"
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" number zeroclaw-lock-crates \
    "Resolved crates in Cargo.lock" \
    "grep -c '^name = ' Cargo.lock"
  claim garraia  "$GARRAIA_ROOT" present garraia-wasm-sandbox \
    "WASM plugin sandbox: memory caps + execution deadlines (wasmtime, opt-in feature)" \
    "grep -n 'StoreLimits\\|epoch_interruption' crates/garraia-plugins/src/runtime.rs"
  claim openclaw "${OPENCLAW_CHECKOUT:-}" present openclaw-plugins-inprocess \
    "Plugins load in-process as trusted code (their own threat model)" \
    "grep -n 'in-process' SECURITY.md"
  claim zeroclaw "${ZEROCLAW_CHECKOUT:-}" present zeroclaw-sig-disabled \
    "Plugin Ed25519 signing exists but defaults to Disabled" \
    "grep -n 'Disabled' crates/zeroclaw-plugins/src/signature.rs"
  finish_scenario "005-attack-surface"
}

precheck_common
write_environment

case "${1:-}" in
  --all)          run_garraia; run_openclaw; run_zeroclaw ;;
  --garraia)      run_garraia ;;
  --openclaw)     run_openclaw ;;
  --zeroclaw)     run_zeroclaw ;;
  --scenario-004) run_scenario_004 ;;
  --scenario-005) run_scenario_005 ;;
  --scenarios)    run_scenario_004; run_scenario_005 ;;
  *)              echo "usage: $0 --all | --garraia | --openclaw | --zeroclaw | --scenario-004 | --scenario-005 | --scenarios" >&2
                  exit 64 ;;
esac

echo "done. see $DATE_DIR/"
