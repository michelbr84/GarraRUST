- **CI volta a subir o MinIO sem depender de registry (#1458).** O Docker Hub
  removeu `minio/minio` (#1230), o quay.io passou a exigir login para toda tag
  em 2026-09-24 e o `dl.min.io` responde 410 — o check obrigatorio `Clippy
  Linting` falhava no `docker pull` em toda PR, e o retry do #1457 nao tinha
  como ajudar. Agora `scripts/ci/build-minio-image.sh` compila o `minio` a
  partir da tag upstream fixada (`RELEASE.2025-02-28T09-55-16Z`, commit
  conferido antes do build, tag movida e recusada), embala em
  `debian:bookworm-slim` e etiqueta a imagem com o `nome:tag` que o
  testcontainer procura — que so faz pull quando a imagem nao existe
  localmente. O binario compilado fica no cache do Actions por tag, entao so
  o primeiro run depois de um bump paga o build. O `docker-compose.minio.yml`
  de dev usa o mesmo script e deixa de depender da imagem do `mc`, tambem
  fechada: o bucket passa a ser criado com o `aws-cli`.
