- **Multipart acima de 16 MiB passa a ter checksum SHA-256 verificado por
  parte pelo servidor (#1229).** O `put_stream` do `S3Compatible` nao
  declarava algoritmo nenhum no `create_multipart_upload`, deixava o SDK
  carimbar um CRC32 default em cada `upload_part` e mandava so ETags no
  `complete`: tres pontos do mesmo upload discordando sobre o que deveria ser
  verificado, num caminho que nenhum teste jamais exercitou de verdade
  (#1230). Agora os tres declaram SHA-256, o mesmo algoritmo que o `put` de
  uma parte so ja usava, e o servidor recusa a parte cujos bytes nao batam com
  o digest declarado. `S3Compatible::new` tambem passa a FIXAR
  `request_checksum_calculation` em `WhenSupported` depois de herdar a config
  do ambiente, para que `AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_REQUIRED` no
  deploy nao consiga rebaixar a garantia — os checksums que importam sao
  explicitos por operacao e o SDK ja os respeita, entao o pin cobre as
  operacoes sem algoritmo nomeado e a regressao futura em que alguem remova o
  `.checksum_sha256(...)`. O checksum composto do S3 (`<digest>-<n>`) e o ETag
  do multipart continuam nunca aparecendo como `etag_sha256`: esse campo e
  sempre o SHA-256 do arquivo inteiro.

  Junto vem o que faltava para conseguir enxergar falha nesse caminho. O
  mapeamento de erro do backend S3 descartava codigo e mensagem do servidor —
  o `Display` de `SdkError` rende literalmente "service error", entao um
  `NotImplemented` (HTTP 501) chegava ao log como
  `Backend("s3 put_object: service error")` e nao dizia nada. Agora carrega
  codigo, mensagem e status HTTP, sem expor URI assinada nem header de
  credencial; de quebra, um 404 de `HEAD` sem codigo de erro no corpo passa a
  virar `NotFound` pelo status, para `exists` responder `false` em vez de
  erro. E o teste de SSE parou de so confiar no eco do MinIO: le de volta o
  `x-amz-server-side-encryption` do objeto gravado, ou cai.
