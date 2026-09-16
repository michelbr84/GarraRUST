- **Os 8 testes de integracao S3 passavam sem nunca subir o MinIO (#1230).**
  O `testcontainers-modules` 0.15 fixa a imagem `minio/minio` do Docker Hub, e
  esse repositorio foi removido de la — o registry responde "object not found"
  para o repositorio inteiro e o pull morre com "pull access denied". Como
  `tests/s3_integration.rs` trata "container nao subiu" como motivo para pular,
  a suite inteira saia verde sem tocar o backend S3 uma unica vez: multipart de
  24 MiB, HMAC de integridade, SSE e presigned URLs eram afirmacoes sem teste
  por tras. O teste agora sobrescreve so o registry (`quay.io/minio/minio`, o
  mesmo tag `RELEASE.2025-02-28T09-55-16Z` que o modulo fixa, publicado pela
  propria MinIO), e o CI passa a rodar esse step com `GARRAIA_REQUIRE_DOCKER=1`
  — a variavel que o teste ja honrava e que transforma o skip em falha. Um
  `docker pull` explicito antes do step faz uma nova remocao de imagem aparecer
  como erro de registry no log, em vez de timeout opaco de testcontainer. O
  `docker-compose.minio.yml` de dev vai junto para o quay.io, com o servidor
  fixado no mesmo release que o CI usa.
