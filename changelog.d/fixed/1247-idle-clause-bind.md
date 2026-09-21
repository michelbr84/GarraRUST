- `SessionStore::validate_session_token` deixou de montar a clausula de idle
  via `format!` (regra 5): o timeout agora entra por bind (`'+' || ?2 ||
  ' seconds'`, coercao nativa do SQLite) e um teste varre o fonte para o
  padrao nao voltar, com teste de regressao do timeout positivo (#1247).
