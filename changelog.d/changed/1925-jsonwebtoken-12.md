- **`jsonwebtoken` sobe de 11.1 para 12.0 (#1925).** A 12 recusa chave HMAC
  vazia ja na construcao (`EncodingKey`/`DecodingKey::from_secret` passam a
  devolver `Result`), rejeita extensoes `crit` desconhecidas e fecha o resto da
  confusao de tipo em `sub`/`iss`/`aud`. Os quatro pontos que montam chave a
  partir de segredo propagam o erro em vez de `unwrap`: `JwtIssuer::new` devolve
  `AuthError::Config` e o `mobile_auth` (extractor e emissao) responde o mesmo
  **503 fail-closed** de auth nao configurada. Nenhum caminho alcancavel muda
  hoje — o piso de 32 bytes do `AuthConfig` ja barrava segredo vazio antes.
