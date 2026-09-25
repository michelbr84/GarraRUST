- **CI: `docker pull` do MinIO ganha retry com backoff (#1456).** O step `Pre-pull
  MinIO image for testcontainers` (`ci.yml`, job `storage-s3`) rodava um `docker pull`
  unico contra o quay.io; um hiccup transiente de registry (observado na PR #1455,
  `unauthorized: access to the requested resource is not authorized`, o mesmo commit
  passando no run anterior de `main`) derrubava o job inteiro, inclusive os steps de
  `storage-s3` que nem tocam MinIO. Agora sao 3 tentativas com backoff curto antes de
  falhar o job — nenhum gate fica mais fraco, so mais resiliente a uma falha de
  registry de poucos segundos.
