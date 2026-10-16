- **`garra whatsapp link --machine`: pareamento em NDJSON (#1636).** Novo modo-
  maquina do `link`: nao exige terminal, nao le stdin e emite NDJSON no stdout
  (uma linha por evento, com `schema` e `event` — `status`, `qr`, `error`,
  `log`). O QR sai como matriz de `0`/`1` pelo mesmo motor de QR do terminal,
  via a nova `qr::modules` em `garraia-channels` (um motor so, sem risco de
  polaridade invertida); o payload cru nunca entra. A ordem que e contrato se
  mantem: `session_saved` antes de `channel_enabled`. Erros tambem saem como
  NDJSON e o exit code segue os mesmos sysexits do `link`. O `link` interativo
  fica inalterado. Para o app desktop consumir como sidecar; ver
  `docs/whatsapp.md` secao "Modo maquina". Refs #1636
