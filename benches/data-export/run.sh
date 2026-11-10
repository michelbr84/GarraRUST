#!/usr/bin/env bash
# Medicao reprodutivel da latencia de `GET /v1/me/export` (#1801).
#
# Roda o teste de integracao `rest_v1_me_export_perf` do `garraia-gateway`
# (router autenticado + pgvector/pg16 em testcontainer) e guarda, ao lado das
# amostras cruas, a maquina, as versoes e o SHA em que a medida foi tirada.
#
# Uso:
#   benches/data-export/run.sh            # perfil dev (o que o CI compila)
#   PERFIL=release benches/data-export/run.sh
#
# Env:
#   PERFIL    dev | release                         (padrao: dev)
#   MENSAGENS mensagens da titular na carga          (padrao: 10000)
#   RUNS      repeticoes quentes                     (padrao: 30)
#
# Qualquer `CARGO_PROFILE_*` exportado vale para o build e fica registrado no
# `environment.txt` — um numero de `release` sem LTO nao pode passar por um
# numero do binario de release.
#
# Requisitos: cargo, git e um Docker em execucao (o harness sobe o Postgres
# sozinho; nenhum banco externo, conta ou token e usado).

set -euo pipefail

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 64
  fi
}

require cargo
require git
require docker

if ! docker info >/dev/null 2>&1; then
  echo "docker nao esta em execucao: o harness precisa dele para o Postgres de teste" >&2
  exit 69
fi

PERFIL="${PERFIL:-dev}"
MENSAGENS="${MENSAGENS:-10000}"
RUNS="${RUNS:-30}"

case "$PERFIL" in
  dev)     PROFILE_FLAG=() ;;
  release) PROFILE_FLAG=(--release) ;;
  *) echo "PERFIL invalido: $PERFIL (use dev ou release)" >&2; exit 64 ;;
esac

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git -C "$HERE" rev-parse --show-toplevel)"

HOST_SHORT="$(hostname -s 2>/dev/null || echo unknown)"
DATE_FLORIDA="$(TZ=America/New_York date +%Y-%m-%d)"
OUT_DIR="$HERE/results/${DATE_FLORIDA}-${HOST_SHORT}"
RAW="$OUT_DIR/raw"
STEM="${PERFIL}-${MENSAGENS}msgs"

IMAGE="pgvector/pgvector:pg16"

# Antes de qualquer arquivo de resultado existir: o proprio registro nao pode
# fazer a arvore parecer suja.
GIT_SHA="$(git -C "$ROOT" rev-parse HEAD)"
# Resultados de execucoes anteriores, ainda nao commitados, nao contam: o que
# importa e se o CODIGO medido e o do SHA.
if [ -n "$(git -C "$ROOT" status --porcelain -- . ':(exclude)benches/data-export/results')" ]; then
  GIT_TREE="dirty"
else
  GIT_TREE="clean"
fi

mkdir -p "$RAW"

{
  echo "# run started (UTC)"
  date -u +"%Y-%m-%dT%H:%M:%SZ"
  echo "# run started (America/New_York)"
  TZ=America/New_York date +"%Y-%m-%dT%H:%M:%S%z"
  echo
  echo "# git"
  echo "sha: $GIT_SHA"
  echo "worktree: $GIT_TREE"
  echo
  echo "# cenario"
  echo "perfil: $PERFIL"
  echo "mensagens: $MENSAGENS"
  echo "repeticoes quentes: $RUNS"
  echo
  echo "# CARGO_PROFILE_* no ambiente"
  env | grep '^CARGO_PROFILE_' | sort || echo "(nenhum)"
  echo
  echo "# uname"
  uname -srmo
  echo
  echo "# cpu"
  if [ -r /proc/cpuinfo ]; then
    grep -m1 'model name' /proc/cpuinfo || true
    echo "logical cpus: $(grep -c '^processor' /proc/cpuinfo)"
  fi
  echo
  echo "# memoria"
  if [ -r /proc/meminfo ]; then
    grep -E '^(MemTotal|MemAvailable):' /proc/meminfo
  fi
  echo
  echo "# toolchain"
  rustc --version
  cargo --version
  echo
  echo "# docker"
  docker version --format 'server: {{.Server.Version}}'
  docker info --format 'storage driver: {{.Driver}}'
  echo
  echo "# imagem do Postgres"
  echo "$IMAGE"
  docker image inspect "$IMAGE" --format '{{index .RepoDigests 0}}' 2>/dev/null \
    || echo "(ainda nao baixada; o harness baixa na primeira execucao)"
} > "$OUT_DIR/environment-${STEM}.txt"

cd "$ROOT"

# Compila antes, fora do log: o registro cru guarda a execucao, nao o build.
cargo test -p garraia-gateway --features test-helpers "${PROFILE_FLAG[@]}" \
  --test rest_v1_me_export_perf --no-run

GARRAIA_EXPORT_PERF_MESSAGES="$MENSAGENS" \
GARRAIA_EXPORT_PERF_RUNS="$RUNS" \
GARRAIA_EXPORT_PERF_OUT="$RAW/${STEM}.json" \
  cargo test -p garraia-gateway --features test-helpers "${PROFILE_FLAG[@]}" \
    --test rest_v1_me_export_perf -- --nocapture --exact \
    export_de_10_mil_mensagens_cabe_na_referencia_e_respeita_o_isolamento \
  2>&1 | tee "$RAW/${STEM}.out.txt"

# (`.out.txt`, nao `.log`: o `.gitignore` da raiz ignora `*.log`.)
# `1 passed` tem de estar no log: teste filtrado ou zero casos nao e medida.
if ! grep -q 'test result: ok. 1 passed' "$RAW/${STEM}.out.txt"; then
  echo "o teste nao rodou (zero casos ou filtrado); nada foi medido" >&2
  exit 70
fi

# O digest so existe depois do primeiro pull; completa o registro.
if grep -q 'ainda nao baixada' "$OUT_DIR/environment-${STEM}.txt"; then
  {
    echo
    echo "# imagem do Postgres (depois da execucao)"
    docker image inspect "$IMAGE" --format '{{index .RepoDigests 0}}'
  } >> "$OUT_DIR/environment-${STEM}.txt"
fi

echo
echo "amostras: $RAW/${STEM}.json"
echo "ambiente: $OUT_DIR/environment-${STEM}.txt"
