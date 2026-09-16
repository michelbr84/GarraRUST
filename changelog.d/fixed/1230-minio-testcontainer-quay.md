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

  **E o primeiro run real encontrou o que o verde escondia:** 6 dos 8 testes
  falharam. O MinIO so aceita SSE-S3 com um KMS configurado, e como ADR 0004
  exige `x-amz-server-side-encryption: AES256` em todo `put`, TODA escrita
  voltava `NotImplemented` (HTTP 501) — o container de teste agora sobe com
  `MINIO_KMS_SECRET_KEY`, que liga o KMS embutido de chave unica (o
  `docker-compose.minio.yml` de dev vai junto, senao o gateway tambem nao
  gravaria um objeto contra o MinIO local); a obrigatoriedade do SSE em
  producao fica intacta. O teste de multipart pedia
  `application/octet-stream`, que nunca esteve na allow-list de MIME: passou a
  usar um tipo permitido (`video/mp4`), sem afrouxar a lista. O caminho
  multipart, que nunca havia rodado de verdade, nao declarava checksum algum
  na criacao, deixava o SDK carimbar um CRC32 default em cada parte e mandava
  so ETags na conclusao — tres pontos discordando sobre o mesmo upload. Agora
  declara SHA-256 nos tres, o mesmo algoritmo que o `put` de uma parte so ja
  usava. E o mapeamento de erro do backend S3 descartava o
  codigo e a mensagem do servidor (o log dizia apenas `s3 put_object: service
  error`): passa a carregar codigo, mensagem e status HTTP, sem expor URI
  assinada nem header de credencial. De quebra, um 404 de `HEAD` sem codigo de
  erro no corpo passa a virar `NotFound` pelo status, para `exists` responder
  `false` em vez de erro. O teste de SSE parou de so confiar no eco do MinIO e
  le de volta o `x-amz-server-side-encryption` do objeto.
