- **Multipart upload nativo do S3 para arquivos acima de 16 MiB (#1214).**
  `S3Compatible::put_stream` bufferizava o corpo inteiro em memoria antes de
  subir, o que tornava um upload de 5 GiB (o teto do ledger `tus_uploads`,
  migration 014) inviavel. Agora `content_length <= 16 MiB` segue no caminho
  unico `put_object` e acima disso o upload vai por `create_multipart_upload`
  + partes de 8 MiB + `complete_multipart_upload`, derrubando o pico de
  memoria de 5 GiB para 8 MiB por upload em voo. SSE-S3 (AES256) e fixado na
  criacao e herdado por todas as partes, a allow-list de MIME roda antes da
  escolha do caminho, e o `etag_sha256` continua sendo o sha256 do conteudo
  completo — entao o HMAC de integridade sobre `{key}:{version_id}:{sha256}`
  vale igual nos dois caminhos. Qualquer falha — erro no stream, erro numa
  parte, erro no proprio `complete`, stream vazio ou stream que entrega menos
  bytes do que declarou — aborta o multipart, para que a chave nunca exponha
  conteudo parcial e nenhuma parte orfa fique sendo faturada. Acompanha um
  `docker-compose.minio.yml` de desenvolvimento (API :9000 e console :9001
  presos a 127.0.0.1, bucket `garraia-dev` criado por um one-shot `mc`).
