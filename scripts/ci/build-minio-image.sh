#!/usr/bin/env bash
# scripts/ci/build-minio-image.sh — materializa, SEM puxar de registry nenhum,
# a imagem do MinIO que o testcontainer de
# `crates/garraia-storage/tests/s3_integration.rs` e o `docker-compose.minio.yml`
# procuram.
#
# Por que existe (#1458)
#
#   Nao ha mais registry gratuito servindo `minio/minio`: o Docker Hub removeu
#   o repositorio (#1230), o quay.io passou a exigir login para TODAS as tags
#   em 2026-09-24 (401 sustentado, nao flake — retry nao resolve) e o
#   `dl.min.io` responde 410 para os binarios de release. O que continua
#   publico e o FONTE, no GitHub, com as tags de release intactas.
#
# O que faz
#
#   1. Compila o `minio` a partir da tag upstream fixada, dentro de um container
#      `golang` (nao exige Go no host), conferindo que a tag ainda aponta para o
#      commit esperado — uma tag movida upstream e recusada, nao compilada.
#   2. Embala o binario estatico em `debian:bookworm-slim`.
#   3. Etiqueta a imagem com o `nome:tag` que o teste pede
#      (`quay.io/minio/minio:RELEASE.…`). O testcontainers 0.27 so faz `pull`
#      quando o `create` do container devolve 404 — uma imagem local com o
#      mesmo `nome:tag` e usada sem tocar a rede.
#
# Idempotente: imagem ja presente → sai 0 sem fazer nada (`FORCE=1` refaz).
# O binario compilado e reaproveitado de `MINIO_BIN_DIR`; no CI esse diretorio
# vai para o `actions/cache`, chaveado pela tag, entao so o primeiro run depois
# de um bump paga o `go build` (~30 s de parede em 20 nucleos, ~3 min de CPU).
#
# Variaveis (todas opcionais):
#   MINIO_TAG      tag upstream (default: a mesma que o teste fixa)
#   MINIO_COMMIT   commit que a tag deve apontar (conferido antes do build)
#   MINIO_IMAGE    nome da imagem a etiquetar
#   MINIO_BIN_DIR  onde o binario compilado fica/e reaproveitado
#   GOLANG_IMAGE   imagem usada para compilar
#   BASE_IMAGE     imagem base da imagem final
#   FORCE=1        recompila e reconstroi mesmo que tudo exista
set -euo pipefail

MINIO_TAG="${MINIO_TAG:-RELEASE.2025-02-28T09-55-16Z}"
MINIO_COMMIT="${MINIO_COMMIT:-8c2c92f7afdc8386b000c0cb57ecec2ee1f5bcb0}"
MINIO_IMAGE="${MINIO_IMAGE:-quay.io/minio/minio}"
MINIO_BIN_DIR="${MINIO_BIN_DIR:-${TMPDIR:-/tmp}/garraia-minio-bin}"
GOLANG_IMAGE="${GOLANG_IMAGE:-golang:1.23-bookworm}"
BASE_IMAGE="${BASE_IMAGE:-debian:bookworm-slim}"
FORCE="${FORCE:-0}"

REF="${MINIO_IMAGE}:${MINIO_TAG}"
BIN="${MINIO_BIN_DIR}/minio"

log() { printf '==> %s\n' "$*" >&2; }
die() { printf 'erro: %s\n' "$*" >&2; exit 1; }

case "$MINIO_TAG" in
    RELEASE.????-??-??T??-??-??Z) ;;
    *) die "MINIO_TAG fora do formato RELEASE.AAAA-MM-DDTHH-MM-SSZ: $MINIO_TAG" ;;
esac
case "$MINIO_COMMIT" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) ;;
    *) die "MINIO_COMMIT nao parece um SHA: $MINIO_COMMIT" ;;
esac

command -v docker >/dev/null 2>&1 || die "docker nao encontrado na PATH"
docker info >/dev/null 2>&1 || die "o daemon do Docker nao respondeu"

if [ "$FORCE" != "1" ] && docker image inspect "$REF" >/dev/null 2>&1; then
    log "imagem $REF ja existe localmente; nada a fazer (FORCE=1 refaz)"
    exit 0
fi

mkdir -p "$MINIO_BIN_DIR"

if [ "$FORCE" = "1" ] || [ ! -x "$BIN" ]; then
    log "compilando minio $MINIO_TAG ($MINIO_COMMIT) a partir do fonte, em $GOLANG_IMAGE"
    # O container e root; o `chown` no fim devolve o binario ao usuario do
    # host, senao o cache do CI e o `docker build` ficam com um arquivo root.
    docker run --rm \
        -v "${MINIO_BIN_DIR}:/out" \
        -e MINIO_TAG="$MINIO_TAG" \
        -e MINIO_COMMIT="$MINIO_COMMIT" \
        -e HOST_UID="$(id -u)" \
        -e HOST_GID="$(id -g)" \
        "$GOLANG_IMAGE" bash -euo pipefail -c '
            git clone --quiet --depth 1 --branch "$MINIO_TAG" https://github.com/minio/minio /src
            cd /src
            head=$(git rev-parse HEAD)
            if [ "$head" != "$MINIO_COMMIT" ]; then
                echo "erro: a tag $MINIO_TAG aponta para $head, esperado $MINIO_COMMIT — tag movida upstream; recusando" >&2
                exit 1
            fi
            export CGO_ENABLED=0 GOTOOLCHAIN=auto
            # RELEASE.2025-02-28T09-55-16Z -> 2025-02-28T09:55:16Z, o mesmo
            # formato que buildscripts/gen-ldflags.go grava em cmd.Version.
            stamp="${MINIO_TAG#RELEASE.}"
            release_time="${stamp:0:13}:${stamp:14:2}:${stamp:17}"
            go build -trimpath \
                -ldflags "-s -w \
                    -X github.com/minio/minio/cmd.Version=${release_time} \
                    -X github.com/minio/minio/cmd.ReleaseTag=${MINIO_TAG} \
                    -X github.com/minio/minio/cmd.CommitID=${MINIO_COMMIT} \
                    -X github.com/minio/minio/cmd.ShortCommitID=${MINIO_COMMIT:0:12}" \
                -o /out/minio .
            chown "${HOST_UID}:${HOST_GID}" /out/minio
        '
else
    log "reaproveitando o binario em $BIN"
fi

[ -x "$BIN" ] || die "o build terminou sem deixar $BIN"

log "embalando em $BASE_IMAGE como $REF"
# Contexto = so o diretorio do binario. Entrypoint direto no `minio`: o
# `docker-entrypoint.sh` da imagem oficial so trata *_FILE e troca de usuario,
# que nem o testcontainer nem o compose de dev usam.
docker build --quiet -t "$REF" -f - "$MINIO_BIN_DIR" <<EOF >/dev/null
FROM ${BASE_IMAGE}
COPY minio /usr/bin/minio
EXPOSE 9000 9001
VOLUME ["/data"]
ENTRYPOINT ["/usr/bin/minio"]
EOF

# A imagem tem de dizer, ela mesma, que e a tag e o commit pedidos.
version="$(docker run --rm "$REF" --version 2>&1 | head -n 1)"
case "$version" in
    *"$MINIO_TAG"*"$MINIO_COMMIT"*) log "ok: $version" ;;
    *) die "a imagem nao reporta a tag/commit esperados: $version" ;;
esac
docker image inspect --format 'imagem {{.RepoTags}} id={{.Id}}' "$REF" >&2
