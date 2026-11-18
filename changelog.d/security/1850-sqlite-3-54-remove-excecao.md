- **SQLite embutido sobe para a versao corrigida e a excecao do RUSTSEC-2026-0459
  sai (#1850).** O `sqlx` 0.9.1 (2026-11-18) subiu o teto do `libsqlite3-sys`
  de `<0.38` para `<0.41`, que era o que prendia o lock na 0.37.0 (SQLite
  < 3.54, CVE-2026-119375: memoria de page cache nao inicializada ao abrir um
  banco com `-wal` irmao de page size diferente). `rusqlite` vai de 0.39 para
  0.43 e o lock passa a `libsqlite3-sys` 0.40.0. A entrada temporaria some de
  `deny.toml` e do espelho em `.cargo/audit.toml` no mesmo commit, como a
  decisao do dono de 2026-11-13 pedia: remover, nao renovar.
