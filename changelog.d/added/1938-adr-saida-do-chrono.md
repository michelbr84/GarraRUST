- **ADR 0028 (proposto) mapeia a saida do `chrono`/`chrono-tz` unmaintained (#1938).**
  O `chrono` continua no `Cargo.lock` mesmo com o nosso codigo migrado: o
  `teloxide-core 0.13`, o `rmcp 3.9` (via `schemars`/`chrono04`) e o `rrule 0.14`
  o exigem sem feature para desligar, e o `rrule` tambem exige o `chrono-tz`.
  A excecao que vence em 2026-12-31 nao pode ser retirada por esforco local; o
  ADR propoe renova-la com essa evidencia (decisao do dono), reduzir ja as
  arestas que so nos escolhemos (`lopdf`, `poise`, `croner`) e, quando o upstream
  liberar, migrar para `jiff` com um newtype unico na fronteira do `sqlx`, que
  nao suporta `jiff`. Nenhum codigo, `deny.toml` ou `.cargo/audit.toml` muda.
