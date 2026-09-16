- **Suite de integracao S3 volta a exercitar um MinIO de verdade (#1230).**
  Os 8 testes de `crates/garraia-storage/tests/s3_integration.rs` fechavam
  verdes em milissegundos sem asserir nada: `minio/minio`, a imagem que
  `testcontainers-modules` 0.15 fixa, foi removida do Docker Hub por inteiro
  (o registry responde "object not found" para o repositorio, nao so para a
  tag), entao `start_minio()` sempre caia no branch de skip silencioso. O
  teste agora aponta para `quay.io/minio/minio` na mesma tag
  (`RELEASE.2025-02-28T09-55-16Z`) via `ImageExt::with_name`/`with_tag` —
  mesma imagem, mesmo conteudo, so troca o registry. O step `storage-s3` do
  CI liga `GARRAIA_REQUIRE_DOCKER=1`, que o teste ja honrava: dali em diante
  um verde nesse step so acontece se o container realmente subir e o
  round-trip rodar contra ele.
