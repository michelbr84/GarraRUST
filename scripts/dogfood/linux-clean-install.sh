#!/usr/bin/env bash
# Dogfood Linux em container limpo (#1426 / #1439, linhas D1 e D9 da matriz
# de `docs/releasing.md` §1.5).
#
# O CI prova que o codigo compila; este script prova que o PACOTE instala e
# funciona para uma pessoa numa maquina sem nada de desenvolvimento:
#
#   1. obtem um .deb x86_64 do GarraIA (CLI) — artefato do release.yml ou
#      build local empacotado com o mesmo nfpm/packaging/nfpm.yaml da release;
#   2. sobe um `ubuntu:24.04` cru e instala o .deb com apt;
#   3. `garraia --version`, `garraia doctor --json` (exit 0, JSON valido),
#      `garraia doctor whatsapp --json` (exit 69 sem WhatsApp, linha
#      `whatsapp.linked` presente);
#   4. configura o Ollama do host como provedor (sem chave paga), sobe o
#      gateway em background na porta 3899, espera `/api/health`, cria uma
#      sessao REST e exige uma resposta real do modelo;
#   5. `garraia restart`, prova que a sessao e a config sobreviveram, e
#      encerra com `garraia stop`;
#   6. grava tudo em `dogfood/linux/<AAAA-MM-DD-HHMM>/` (gitignored) e
#      imprime PASSOU/FALHOU por passo.
#
# Rede: o container roda com `--network host`, entao o gateway que binda em
# 127.0.0.1:3899 la dentro e o mesmo loopback do host — o curl/jq ficam DO
# LADO DE FORA e o container continua sem nenhuma ferramenta extra. Pelo
# mesmo motivo o Ollama do host e alcancado em 127.0.0.1:11434 de dentro.
#
# Nunca toca a 3888 nem o ~/.config/garraia do host: a config vive dentro
# do container (descartado no fim) e a porta e recusada se for 3888.
#
# Uso:
#   scripts/dogfood/linux-clean-install.sh                 # artefato > build local
#   scripts/dogfood/linux-clean-install.sh --source local  # o checkout atual (candidato)
#   scripts/dogfood/linux-clean-install.sh --run-id <id>   # um run especifico do release.yml
#   scripts/dogfood/linux-clean-install.sh --deb ./garraia-linux-x86_64.deb
#
# Para o gate de release, o que se testa e o CANDIDATO: rode com
# `--source local` no checkout da PR de release, ou com `--run-id` do
# `workflow_dispatch` de teste. O default (`auto`) baixa o pacote do ultimo
# run verde do release.yml — util para reproduzir um bug de campo, mas ele
# testa a release anterior, nao a que vai sair.
#
# Requisitos no host: docker, curl, jq; gh (so para baixar artefato);
# cargo (so para o build local). Ollama em 127.0.0.1:11434 com o modelo
# (default `qwen3.5:0.8b`).

set -euo pipefail

# ─── Parametros ─────────────────────────────────────────────────────────────

REPO="michelbr84/GarraRUST"
IMAGE="${DOGFOOD_IMAGE:-ubuntu:24.04}"
PORT="${DOGFOOD_PORT:-3899}"
MODEL="${DOGFOOD_MODEL:-qwen3.5:0.8b}"
# Visto DE DENTRO do container (com --network host e o loopback do host).
OLLAMA_BASE_URL="${DOGFOOD_OLLAMA_BASE_URL:-http://127.0.0.1:11434/v1}"
OLLAMA_TAGS_URL="${OLLAMA_BASE_URL%/v1}/api/tags"
SOURCE="${DOGFOOD_SOURCE:-auto}"     # auto | artifact | local
RUN_ID="${DOGFOOD_RUN_ID:-}"
DEB_PATH="${DOGFOOD_DEB:-}"
KEEP_CONTAINER=0
HEALTH_TIMEOUT_SECS="${DOGFOOD_HEALTH_TIMEOUT:-90}"
MODEL_TIMEOUT_SECS="${DOGFOOD_MODEL_TIMEOUT:-300}"
PROMPT="${DOGFOOD_PROMPT:-responda só com a palavra pronto}"

# Mesmo pino do job package-linux do release.yml — um nfpm "latest" movel
# nunca deve mudar o pacote que este script produz.
NFPM_VERSION="2.47.0"
NFPM_SHA256="0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783"
ARTIFACT_NAME="garraia-linux-packages"
DEB_BASENAME="garraia-linux-x86_64.deb"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
EVIDENCE_ROOT="${DOGFOOD_EVIDENCE_ROOT:-${REPO_ROOT}/dogfood/linux}"

usage() {
    sed -n '2,45p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --deb)      DEB_PATH="$2"; shift 2 ;;
        --source)   SOURCE="$2"; shift 2 ;;
        --run-id)   RUN_ID="$2"; shift 2 ;;
        --image)    IMAGE="$2"; shift 2 ;;
        --port)     PORT="$2"; shift 2 ;;
        --model)    MODEL="$2"; shift 2 ;;
        --keep)     KEEP_CONTAINER=1; shift ;;
        -h|--help)  usage; exit 0 ;;
        *) echo "argumento desconhecido: $1" >&2; usage >&2; exit 64 ;;
    esac
done

case "$SOURCE" in
    auto|artifact|local) ;;
    *) echo "--source precisa ser auto, artifact ou local (recebi '$SOURCE')" >&2; exit 64 ;;
esac

if [ "$PORT" = "3888" ]; then
    echo "recusado: a 3888 e a porta do GarraIA do host; use outra (default 3899)." >&2
    exit 64
fi

# ─── Evidencia e log ────────────────────────────────────────────────────────

STAMP="$(date +%Y-%m-%d-%H%M)"
EVIDENCE="${EVIDENCE_ROOT}/${STAMP}"
mkdir -p "$EVIDENCE"
RUN_LOG="${EVIDENCE}/run.log"
# Tudo que o script imprime vai tambem para run.log.
exec > >(tee -a "$RUN_LOG") 2>&1

BASE_URL="http://127.0.0.1:${PORT}"
CONTAINER="garraia-dogfood-$$"
CID=""
STARTED_AT=$(date +%s)

# Resultado por passo (ordem de execucao preservada).
STEP_IDS=()
STEP_DESCS=()
STEP_RESULTS=()
STEP_SECS=()
GATE_FAILED=0

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }

record() { # record <id> <descricao> <PASSOU|FALHOU|PULADO> <segundos>
    STEP_IDS+=("$1"); STEP_DESCS+=("$2"); STEP_RESULTS+=("$3"); STEP_SECS+=("$4")
}

# run_step <id> <descricao> <gate:0|1> <funcao>
# Um passo "gate" que falha pula tudo que vem depois (sem daemon nao ha REST
# que testar); os demais so registram FALHOU e seguem.
run_step() {
    local id="$1" desc="$2" gate="$3" fn="$4"
    if [ "$GATE_FAILED" = 1 ]; then
        record "$id" "$desc" "PULADO" 0
        return 0
    fi
    log "── ${id}: ${desc}"
    local t0 rc
    t0=$(date +%s)
    set +e
    "$fn"
    rc=$?
    set -e
    local dt=$(( $(date +%s) - t0 ))
    if [ "$rc" -eq 0 ]; then
        record "$id" "$desc" "PASSOU" "$dt"
        log "   PASSOU (${dt}s)"
    else
        record "$id" "$desc" "FALHOU" "$dt"
        log "   FALHOU (rc=${rc}, ${dt}s)"
        [ "$gate" = 1 ] && GATE_FAILED=1
    fi
    return 0
}

# ─── Helpers de container ───────────────────────────────────────────────────

# Toda invocacao da CLI dentro do container leva PORT: `stop`, `restart`,
# `status` e `doctor` resolvem a porta do daemon por env > default (#1261),
# e o default e a 3888 que este script promete nunca tocar.
in_ctr() {
    docker exec -e "PORT=${PORT}" -e DEBIAN_FRONTEND=noninteractive "$CID" "$@"
}

cleanup() {
    local rc=$?
    if [ -n "$CID" ]; then
        if [ "$KEEP_CONTAINER" = 1 ] && [ "$rc" -ne 0 ]; then
            log "container mantido para inspecao (--keep): docker exec -it ${CONTAINER} bash"
            log "quando terminar: docker rm -f ${CONTAINER}"
        else
            # Melhor esforco: derruba o daemon antes de matar o container, para
            # o log terminar com um shutdown limpo e nao com um SIGKILL.
            in_ctr garraia stop >/dev/null 2>&1 || true
            docker rm -f "$CID" >/dev/null 2>&1 || true
        fi
    fi
}
trap cleanup EXIT

# ─── Passos ─────────────────────────────────────────────────────────────────

step_prereqs() {
    local missing=0
    for tool in docker curl jq; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            log "   falta '$tool' no host"; missing=1
        fi
    done
    [ "$missing" = 0 ] || return 1
    docker version --format 'docker client {{.Client.Version}} / server {{.Server.Version}}' \
        > "${EVIDENCE}/docker-version.txt" 2>&1 || { log "   docker nao responde"; return 1; }

    # A porta tem de estar livre NO HOST: com --network host, e la que o
    # daemon vai bindar.
    if (command -v ss >/dev/null 2>&1 && ss -ltn 2>/dev/null | grep -qE "[.:]${PORT}[[:space:]]") \
       || curl -fsS --max-time 2 "${BASE_URL}/api/health" >/dev/null 2>&1; then
        log "   porta ${PORT} ja esta em uso no host"; return 1
    fi

    # Ollama do host com o modelo pedido. A URL de tags e derivada da base
    # OpenAI-compativel que vai para a config (…/v1 → …/api/tags).
    local tags
    if ! tags=$(curl -fsS --max-time 5 "$OLLAMA_TAGS_URL"); then
        log "   Ollama nao respondeu em ${OLLAMA_TAGS_URL}"; return 1
    fi
    if ! printf '%s' "$tags" | jq -e --arg m "$MODEL" '.models[] | select(.name == $m)' >/dev/null; then
        log "   modelo '${MODEL}' nao esta no Ollama (ollama pull ${MODEL})"
        printf '%s' "$tags" | jq -r '.models[].name' | sed 's/^/     disponivel: /'
        return 1
    fi
    printf '%s' "$tags" | jq --arg m "$MODEL" '{model: $m, present: true}' > "${EVIDENCE}/ollama.json"
    return 0
}

# Baixa o artefato garraia-linux-packages de um run do release.yml.
# Nunca DISPARA o workflow: so le o que um run passado deixou.
fetch_artifact() {
    command -v gh >/dev/null 2>&1 || { log "   gh nao instalado; sem artefato"; return 1; }
    local run_id="$RUN_ID"
    if [ -z "$run_id" ]; then
        run_id=$(gh run list -R "$REPO" --workflow release.yml --status success --limit 1 \
            --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)
        [ -n "$run_id" ] || { log "   nenhum run verde do release.yml visivel"; return 1; }
    fi
    local dl="${EVIDENCE}/artifact"
    mkdir -p "$dl"
    log "   gh run download ${run_id} -n ${ARTIFACT_NAME}"
    if ! gh run download "$run_id" -R "$REPO" -n "$ARTIFACT_NAME" -D "$dl" > "${EVIDENCE}/gh-download.log" 2>&1; then
        log "   download falhou (ver gh-download.log): $(tail -1 "${EVIDENCE}/gh-download.log")"
        return 1
    fi
    local deb
    deb=$(find "$dl" -name "$DEB_BASENAME" -type f | head -1)
    [ -n "$deb" ] || { log "   artefato sem ${DEB_BASENAME}"; return 1; }
    gh run view "$run_id" -R "$REPO" --json databaseId,headBranch,event,createdAt,conclusion \
        > "${EVIDENCE}/artifact-run.json" 2>/dev/null || true
    # So o .deb x86_64 interessa; o resto do artefato (rpm, AppImage, arm64)
    # nao precisa ficar na evidencia.
    mv "$deb" "${EVIDENCE}/${DEB_BASENAME}"
    rm -rf "$dl"
    DEB_PATH="${EVIDENCE}/${DEB_BASENAME}"
    PKG_SOURCE="artifact run ${run_id} (${ARTIFACT_NAME})"
    return 0
}

resolve_nfpm() {
    if [ -n "${NFPM_BIN:-}" ] && [ -x "$NFPM_BIN" ]; then
        echo "$NFPM_BIN"; return 0
    fi
    if command -v nfpm >/dev/null 2>&1; then
        command -v nfpm; return 0
    fi
    local cache="${XDG_CACHE_HOME:-$HOME/.cache}/garraia-dogfood/nfpm-${NFPM_VERSION}"
    if [ ! -x "${cache}/nfpm" ]; then
        mkdir -p "$cache"
        local tgz="${cache}/nfpm.tar.gz"
        curl -fsSL --retry 3 --retry-delay 2 -o "$tgz" \
            "https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz" \
            || return 1
        echo "${NFPM_SHA256}  ${tgz}" | sha256sum -c - >/dev/null || { rm -f "$tgz"; return 1; }
        tar -xzf "$tgz" -C "$cache" nfpm
        rm -f "$tgz"
    fi
    echo "${cache}/nfpm"
}

# Build local + empacotamento com o MESMO nfpm.yaml da release, para o
# pacote testado ter os mesmos metadados, symlink `garra` e depends.
build_local() {
    command -v cargo >/dev/null 2>&1 || { log "   cargo nao instalado; sem build local"; return 1; }
    local target_dir="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}"
    log "   cargo build --release --package garraia --bin garra (target: ${target_dir})"
    (cd "$REPO_ROOT" && cargo build --release --package garraia --bin garra) \
        > "${EVIDENCE}/cargo-build.log" 2>&1 \
        || { log "   build falhou (ver cargo-build.log)"; return 1; }
    local bin="${target_dir}/release/garra"
    [ -x "$bin" ] || { log "   binario nao apareceu em ${bin}"; return 1; }

    local version
    version=$(cd "$REPO_ROOT" && cargo metadata --no-deps --format-version 1 \
        | jq -r '.packages[] | select(.name == "garraia") | .version')
    [ -n "$version" ] || { log "   nao consegui ler a versao do pacote garraia"; return 1; }

    local nfpm
    nfpm=$(resolve_nfpm) || { log "   nfpm indisponivel (download/checksum falhou)"; return 1; }
    # `nfpm --version` imprime um banner ASCII; a versao esta em `GitVersion:`.
    log "   nfpm $("$nfpm" --version 2>/dev/null | awk '/^GitVersion:/{print $2}') → ${DEB_BASENAME} (v${version})"
    # nfpm.yaml referencia LICENSE e README.md relativos ao cwd.
    (cd "$REPO_ROOT" && GARRAIA_PKG_VERSION="$version" GARRAIA_PKG_ARCH=amd64 GARRAIA_PKG_BIN="$bin" \
        "$nfpm" package -f packaging/nfpm.yaml -p deb -t "${EVIDENCE}/${DEB_BASENAME}") \
        > "${EVIDENCE}/nfpm.log" 2>&1 \
        || { log "   nfpm falhou (ver nfpm.log)"; return 1; }
    DEB_PATH="${EVIDENCE}/${DEB_BASENAME}"
    PKG_SOURCE="build local (git $(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo '?'), nfpm ${NFPM_VERSION})"
    return 0
}

PKG_SOURCE=""
step_package() {
    if [ -n "$DEB_PATH" ]; then
        [ -f "$DEB_PATH" ] || { log "   --deb aponta para arquivo inexistente: ${DEB_PATH}"; return 1; }
        PKG_SOURCE="fornecido (--deb ${DEB_PATH})"
        cp "$DEB_PATH" "${EVIDENCE}/${DEB_BASENAME}"
        DEB_PATH="${EVIDENCE}/${DEB_BASENAME}"
    else
        case "$SOURCE" in
            artifact) fetch_artifact || return 1 ;;
            local)    build_local || return 1 ;;
            auto)
                if ! fetch_artifact; then
                    log "   sem artefato acessivel; caindo para o build local"
                    build_local || return 1
                fi ;;
        esac
    fi
    {
        echo "fonte: ${PKG_SOURCE}"
        echo "arquivo: ${DEB_PATH}"
        echo "sha256: $(sha256sum "$DEB_PATH" | cut -d' ' -f1)"
        echo "tamanho: $(stat -c%s "$DEB_PATH") bytes"
        if command -v dpkg-deb >/dev/null 2>&1; then
            echo "--- dpkg-deb --info ---"
            dpkg-deb --info "$DEB_PATH"
            echo "--- dpkg-deb --contents ---"
            dpkg-deb --contents "$DEB_PATH"
        fi
    } > "${EVIDENCE}/pacote.txt"
    log "   ${PKG_SOURCE}"
    return 0
}

step_container() {
    docker pull -q "$IMAGE" > "${EVIDENCE}/docker-pull.log" 2>&1 || true
    # `sleep infinity` segura o container para os `docker exec` sucessivos;
    # `--init` poe um init de verdade como PID 1 — sem ele o `sleep` nao
    # colhe filhos, o daemon que o `stop` derruba vira zumbi e `kill(pid, 0)`
    # segue dizendo que ele existe (uma maquina real tem systemd/init para
    # isso); `--rm` para nao sobrar nada; `--network host` pelas razoes do
    # cabecalho.
    CID=$(docker run -d --rm --init --name "$CONTAINER" --network host \
        -v "${EVIDENCE}/${DEB_BASENAME}:/pkg/${DEB_BASENAME}:ro" \
        "$IMAGE" sleep infinity) || return 1
    docker exec "$CID" bash -c 'source /etc/os-release && echo "$PRETTY_NAME"; ldd --version | head -1; echo "ferramentas de dev? $(command -v cargo gcc node npm python3 || echo nenhuma)"' \
        > "${EVIDENCE}/container.txt" 2>&1
    cat "${EVIDENCE}/container.txt" | sed 's/^/   /'
    return 0
}

step_install() {
    # O que um usuario faz: apt-get install ./pacote.deb. Sem `apt-get update`
    # primeiro, porque o unico depends e libc6 e ele ja esta na imagem; se o
    # apt reclamar mesmo assim, atualiza os indices e tenta de novo.
    if ! in_ctr bash -c "apt-get install -y /pkg/${DEB_BASENAME}" > "${EVIDENCE}/apt-install.log" 2>&1; then
        log "   apt-get install sem indices falhou; tentando com apt-get update"
        in_ctr bash -c "apt-get update && apt-get install -y /pkg/${DEB_BASENAME}" >> "${EVIDENCE}/apt-install.log" 2>&1 \
            || { tail -5 "${EVIDENCE}/apt-install.log" | sed 's/^/   /'; return 1; }
    fi
    in_ctr dpkg -s garraia > "${EVIDENCE}/dpkg-s.txt" 2>&1 || return 1
    in_ctr bash -c 'ls -l /usr/bin/garraia /usr/bin/garra' | sed 's/^/   /'
    grep -E '^(Package|Version|Architecture|Depends):' "${EVIDENCE}/dpkg-s.txt" | sed 's/^/   /'
    return 0
}

step_version() {
    in_ctr garraia --version > "${EVIDENCE}/versao.txt" 2>&1 || return 1
    sed 's/^/   /' "${EVIDENCE}/versao.txt"
    # O symlink `garra` que o nfpm.yaml declara tem de responder tambem.
    in_ctr garra --version >/dev/null 2>&1 || { log "   /usr/bin/garra nao responde"; return 1; }
    grep -qE '[0-9]+\.[0-9]+\.[0-9]+' "${EVIDENCE}/versao.txt"
}

step_doctor() {
    local rc=0
    in_ctr garraia doctor --json > "${EVIDENCE}/doctor.json" 2> "${EVIDENCE}/doctor.stderr" || rc=$?
    echo "exit_code=${rc}" > "${EVIDENCE}/doctor.exit"
    jq -e . "${EVIDENCE}/doctor.json" >/dev/null 2>&1 || { log "   stdout nao e JSON valido"; return 1; }
    log "   exit=${rc} ok=$(jq -r .ok "${EVIDENCE}/doctor.json") status=$(jq -r '.report.config_check.status // "n/a"' "${EVIDENCE}/doctor.json") config_dir=$(jq -r .report.config_dir "${EVIDENCE}/doctor.json")"
    [ "$rc" -eq 0 ] || return 1
    [ "$(jq -r .exit_code "${EVIDENCE}/doctor.json")" = "0" ]
}

step_doctor_whatsapp() {
    local rc=0
    in_ctr garraia doctor whatsapp --json > "${EVIDENCE}/doctor-whatsapp.json" 2> "${EVIDENCE}/doctor-whatsapp.stderr" || rc=$?
    echo "exit_code=${rc}" > "${EVIDENCE}/doctor-whatsapp.exit"
    if grep -q "unexpected argument 'whatsapp'" "${EVIDENCE}/doctor-whatsapp.stderr" 2>/dev/null; then
        log "   esta versao nao tem \`doctor whatsapp\` (entrou com #1419): pacote anterior ao subcomando"
        return 1
    fi
    jq -e . "${EVIDENCE}/doctor-whatsapp.json" >/dev/null 2>&1 || { log "   stdout nao e JSON valido"; return 1; }
    local linked
    linked=$(jq -c '.report.checks[] | select(.id == "whatsapp.linked") | {status, detail}' "${EVIDENCE}/doctor-whatsapp.json")
    log "   exit=${rc} whatsapp.linked=${linked:-AUSENTE}"
    [ -n "$linked" ] || { log "   linha whatsapp.linked ausente"; return 1; }
    # Instalacao sem WhatsApp: EX_UNAVAILABLE (69), e o JSON diz o mesmo.
    [ "$rc" -eq 69 ] || { log "   esperava exit 69 numa instalacao sem WhatsApp"; return 1; }
    [ "$(jq -r .exit_code "${EVIDENCE}/doctor-whatsapp.json")" = "69" ]
}

step_set_model() {
    # Os defaults do set-model sao exatamente o Ollama OpenAI-compativel em
    # 127.0.0.1:11434/v1 — que, com --network host, e o Ollama do host.
    in_ctr garraia config set-model --model "$MODEL" --base-url "$OLLAMA_BASE_URL" \
        > "${EVIDENCE}/set-model.txt" 2>&1 || { sed 's/^/   /' "${EVIDENCE}/set-model.txt"; return 1; }
    sed 's/^/   /' "${EVIDENCE}/set-model.txt"
    return 0
}

wait_healthy() { # wait_healthy <arquivo-de-saida>
    local out="$1" deadline=$(( $(date +%s) + HEALTH_TIMEOUT_SECS )) body status
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if body=$(curl -fsS --max-time 5 "${BASE_URL}/api/health" 2>/dev/null); then
            status=$(printf '%s' "$body" | jq -r '.status // empty' 2>/dev/null || true)
            if [ "$status" = "healthy" ]; then
                printf '%s\n' "$body" > "$out"
                return 0
            fi
        fi
        sleep 1
    done
    # Estourou: guarda o que quer que o gateway tenha dito por ultimo.
    printf '%s\n' "${body:-}" > "$out"
    log "   /api/health nao ficou healthy em ${HEALTH_TIMEOUT_SECS}s (ultimo: ${status:-sem resposta})"
    [ -n "${body:-}" ] && printf '%s' "$body" | jq -r '.warnings[]? | "     aviso: " + .' 2>/dev/null
    return 1
}

step_start() {
    in_ctr garraia start -d --port "$PORT" > "${EVIDENCE}/start.txt" 2>&1 || { sed 's/^/   /' "${EVIDENCE}/start.txt"; return 1; }
    sed 's/^/   /' "${EVIDENCE}/start.txt"
    wait_healthy "${EVIDENCE}/health-inicial.json" || return 1
    log "   healthy: provider=$(jq -r .provider "${EVIDENCE}/health-inicial.json") model=$(jq -r .model "${EVIDENCE}/health-inicial.json") version=$(jq -r .version "${EVIDENCE}/health-inicial.json")"
    # D9: o que o Web Console mostra numa instalacao limpa. Fica na evidencia
    # inteiro; aqui so se exige que responda e se listam os checks fora de
    # `ok`, para quem le o resumo ver de cara se ha aviso espurio.
    local code
    code=$(curl -sS --max-time 30 -o "${EVIDENCE}/diagnostics.json" -w '%{http_code}' "${BASE_URL}/api/diagnostics") || return 1
    [ "$code" = "200" ] || { log "   GET /api/diagnostics → HTTP ${code}"; return 1; }
    log "   /api/diagnostics: status=$(jq -r .status "${EVIDENCE}/diagnostics.json") ($(jq -r '.checks | length' "${EVIDENCE}/diagnostics.json") checks)"
    jq -r '.checks[] | select(.status != "ok") | "     " + .id + " = " + (.status|tostring) + ": " + .detail' "${EVIDENCE}/diagnostics.json"
    return 0
}

SESSION_ID=""
step_chat() {
    local resp
    resp=$(curl -sS --max-time 15 -X POST "${BASE_URL}/api/sessions" \
        -H 'Content-Type: application/json' -d '{}' -w '\n%{http_code}') || return 1
    printf '%s\n' "$resp" | head -n -1 > "${EVIDENCE}/sessao.json"
    local code; code=$(printf '%s' "$resp" | tail -n 1)
    SESSION_ID=$(jq -r '.session_id // empty' "${EVIDENCE}/sessao.json")
    [ "$code" = "201" ] || [ "$code" = "200" ] || { log "   POST /api/sessions → HTTP ${code}"; return 1; }
    [ -n "$SESSION_ID" ] || { log "   sem session_id"; return 1; }
    log "   sessao ${SESSION_ID} (HTTP ${code})"

    local t0; t0=$(date +%s)
    resp=$(curl -sS --max-time "$MODEL_TIMEOUT_SECS" -X POST "${BASE_URL}/api/sessions/${SESSION_ID}/messages" \
        -H 'Content-Type: application/json' \
        -d "$(jq -cn --arg c "$PROMPT" '{content: $c}')" -w '\n%{http_code}') || return 1
    printf '%s\n' "$resp" | head -n -1 > "${EVIDENCE}/resposta-modelo.json"
    code=$(printf '%s' "$resp" | tail -n 1)
    local content
    content=$(jq -r '.content // empty' "${EVIDENCE}/resposta-modelo.json" 2>/dev/null || true)
    log "   POST …/messages → HTTP ${code} em $(( $(date +%s) - t0 ))s"
    [ "$code" = "200" ] || { sed 's/^/   /' "${EVIDENCE}/resposta-modelo.json"; return 1; }
    [ -n "$content" ] || { log "   resposta sem texto"; return 1; }
    log "   modelo: $(printf '%s' "$content" | head -c 160 | tr '\n' ' ')"
    return 0
}

step_restart() {
    in_ctr garraia restart -d --port "$PORT" > "${EVIDENCE}/restart.txt" 2>&1 || { sed 's/^/   /' "${EVIDENCE}/restart.txt"; return 1; }
    sed 's/^/   /' "${EVIDENCE}/restart.txt"
    wait_healthy "${EVIDENCE}/health-pos-restart.json" || return 1
    # Uptime pequeno = e um processo NOVO, nao o antigo que sobreviveu.
    local up; up=$(jq -r '.uptime_secs // 0' "${EVIDENCE}/health-pos-restart.json")
    log "   healthy de novo (uptime ${up}s)"
    [ "$up" -lt "$HEALTH_TIMEOUT_SECS" ] || { log "   uptime nao zerou: o restart nao trocou o processo"; return 1; }
    return 0
}

step_persistence() {
    local ok=0
    # Config: o provider/modelo que o set-model gravou continua sendo o que
    # o gateway novo carregou.
    local prov model
    prov=$(jq -r .provider "${EVIDENCE}/health-pos-restart.json")
    model=$(jq -r .model "${EVIDENCE}/health-pos-restart.json")
    if [ "$model" = "$MODEL" ]; then
        log "   config sobreviveu: provider=${prov} model=${model}"
    else
        log "   config NAO sobreviveu: model=${model} (esperava ${MODEL})"; ok=1
    fi
    # Sessao: o historico da sessao criada ANTES do restart e readotado do
    # sessions.db pelo processo novo.
    local code
    code=$(curl -sS --max-time 15 -o "${EVIDENCE}/historico-pos-restart.json" -w '%{http_code}' \
        "${BASE_URL}/api/sessions/${SESSION_ID}/history") || return 1
    local n user_ok
    n=$(jq -r '.messages | length' "${EVIDENCE}/historico-pos-restart.json" 2>/dev/null || echo 0)
    user_ok=$(jq -r --arg p "$PROMPT" '[.messages[] | select(.role == "user" and .content == $p)] | length' \
        "${EVIDENCE}/historico-pos-restart.json" 2>/dev/null || echo 0)
    log "   GET …/history → HTTP ${code}, ${n} mensagens, pergunta original presente: ${user_ok}"
    [ "$code" = "200" ] && [ "$n" -ge 2 ] && [ "$user_ok" -ge 1 ] || ok=1
    in_ctr garraia status > "${EVIDENCE}/status.txt" 2>&1 || true
    return "$ok"
}

step_stop() {
    in_ctr garraia stop > "${EVIDENCE}/stop.txt" 2>&1 || { sed 's/^/   /' "${EVIDENCE}/stop.txt"; return 1; }
    sed 's/^/   /' "${EVIDENCE}/stop.txt"
    # A porta tem de fechar de verdade.
    local i
    for i in $(seq 1 15); do
        curl -fsS --max-time 2 "${BASE_URL}/api/health" >/dev/null 2>&1 || break
        sleep 1
    done
    if curl -fsS --max-time 2 "${BASE_URL}/api/health" >/dev/null 2>&1; then
        log "   a porta ${PORT} continua respondendo depois do stop"; return 1
    fi
    if in_ctr test -e /root/.config/garraia/garraia.pid; then
        log "   garraia.pid ficou para tras"; return 1
    fi
    return 0
}

collect_evidence() {
    [ -n "$CID" ] || return 0
    # O garraia.log ja sai pelo RedactingWriter; a config leva o placeholder
    # de chave do set-model e vai redigida mesmo assim, por principio.
    docker cp "${CID}:/root/.config/garraia/garraia.log" "${EVIDENCE}/garraia.log" 2>/dev/null || true
    docker cp "${CID}:/root/.config/garraia/config.yml" "${EVIDENCE}/config.redigido.yml" 2>/dev/null \
        && sed -i -E 's/^([[:space:]]*api_key:).*/\1 "<redigido>"/' "${EVIDENCE}/config.redigido.yml" || true
}

# ─── Execucao ───────────────────────────────────────────────────────────────

log "GarraIA dogfood Linux — container limpo"
log "imagem=${IMAGE} porta=${PORT} modelo=${MODEL} fonte=${SOURCE}${RUN_ID:+ run=${RUN_ID}}${DEB_PATH:+ deb=${DEB_PATH}}"
log "evidencia: ${EVIDENCE}"

run_step P0  "pre-requisitos do host (docker, curl, jq, porta livre, Ollama + modelo)" 1 step_prereqs
run_step P1  "obter o pacote .deb x86_64"                                              1 step_package
run_step P2  "subir container limpo (${IMAGE}, sem ferramentas de dev)"                1 step_container
run_step P3  "apt-get install ./garraia.deb"                                            1 step_install
run_step P4  "garraia --version (e o alias garra)"                                       0 step_version
run_step P5  "garraia doctor --json → exit 0 e JSON valido"                              0 step_doctor
run_step P6  "garraia doctor whatsapp --json → exit 69 e linha whatsapp.linked"          0 step_doctor_whatsapp
run_step P7  "config set-model → Ollama do host, sem chave paga"                         1 step_set_model
run_step P8  "garraia start -d :${PORT} → /api/health healthy"                           1 step_start
run_step P9  "sessao REST + mensagem → resposta real do modelo"                          1 step_chat
run_step P10 "garraia restart -d → healthy de novo, processo novo"                       1 step_restart
run_step P11 "sessao e config sobrevivem ao restart"                                     0 step_persistence
run_step P12 "garraia stop → porta fechada, pid file removido"                           0 step_stop

collect_evidence

# ─── Resumo ─────────────────────────────────────────────────────────────────

TOTAL_SECS=$(( $(date +%s) - STARTED_AT ))
FAILS=0
{
    echo "GarraIA dogfood Linux — ${STAMP}"
    echo "pacote: ${PKG_SOURCE:-nao obtido}"
    [ -s "${EVIDENCE}/versao.txt" ] && echo "versao instalada: $(cat "${EVIDENCE}/versao.txt")"
    echo "imagem: ${IMAGE}   porta: ${PORT}   modelo: ${MODEL}"
    echo
    for i in "${!STEP_IDS[@]}"; do
        printf '%-4s %-7s %4ss  %s\n' "${STEP_IDS[$i]}" "${STEP_RESULTS[$i]}" "${STEP_SECS[$i]}" "${STEP_DESCS[$i]}"
        [ "${STEP_RESULTS[$i]}" = "PASSOU" ] || FAILS=$((FAILS + 1))
    done
    echo
    echo "tempo total: ${TOTAL_SECS}s   passos nao-PASSOU: ${FAILS}"
    echo "evidencia: ${EVIDENCE}"
} | tee "${EVIDENCE}/resumo.txt"

# Versao legivel por maquina, para a PR de release colar sem retrabalho.
{
    printf '{"stamp":"%s","image":"%s","port":%s,"model":"%s","package":"%s","total_secs":%s,"steps":[' \
        "$STAMP" "$IMAGE" "$PORT" "$MODEL" "${PKG_SOURCE:-}" "$TOTAL_SECS"
    for i in "${!STEP_IDS[@]}"; do
        [ "$i" -gt 0 ] && printf ','
        jq -cn --arg id "${STEP_IDS[$i]}" --arg d "${STEP_DESCS[$i]}" --arg r "${STEP_RESULTS[$i]}" --argjson s "${STEP_SECS[$i]}" \
            '{id: $id, desc: $d, result: $r, secs: $s}'
    done
    printf ']}\n'
} | jq . > "${EVIDENCE}/resumo.json"

[ "$FAILS" -eq 0 ]
