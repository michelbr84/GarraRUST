- **Exportar e importar a politica de acesso do WhatsApp, sem segredo (#1435).**
  `garraia whatsapp access export [--out <arquivo>] [--redact-pii]` e
  `GET /admin/api/whatsapp/access/export` produzem um documento versionado
  `garraia.access-policy` (v1) montado campo a campo por allowlist da politica
  (admissao, default, niveis, grupos, bloqueios): nunca serializando a secao,
  entao token, cofre e material de sessao ficam de fora por construcao.
  `--redact-pii` mascara as identidades para os quatro ultimos digitos e marca
  o export como nao-importavel. `garraia whatsapp access import <arquivo>
  [--confirm-widening] [--dry-run]` e `POST /admin/api/whatsapp/access/import`
  validam formato e versao (formato desconhecido e versao de major futuro sao
  recusados), recusam export redigido, substituem a politica inteira (limpando
  o legado e preservando `enabled`/`default_mode`/sessao), mostram o impacto
  por principal pelo mesmo motor do `--dry-run` e exigem confirmacao explicita
  para qualquer alargamento. A escrita e atomica com backup
  `config.yml.import-bak` para rollback, e o import vai para o audit como a
  acao `import`.
