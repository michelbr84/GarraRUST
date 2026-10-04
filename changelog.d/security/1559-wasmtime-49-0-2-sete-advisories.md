- **wasmtime 49.0.1 -> 49.0.2 fecha sete advisories, e o piso de MSRV NAO sobe
  (#1559).** Tres no `wasmtime` — `RUSTSEC-2026-0325` e `RUSTSEC-2026-0326`
  (corrupcao da heap do GC: import de tag WebAssembly com tipo errado, e
  rooting ausente para valores do GC vivos atraves de `try_call`) e
  `RUSTSEC-2026-0327` (contagem de resultados do callback async-lifted de
  componente sem validacao, com estouro de buffer na pilha nativa) — e quatro
  no `wasmtime-wasi`: `RUSTSEC-2026-0321` (o `poll_oneoff` do WASI preview 0
  burlava a contabilidade de combustivel), `RUSTSEC-2026-0322` (memoria do host
  alocada em excesso quando o guest nao tem stdio), `RUSTSEC-2026-0323`
  (`fd_readdir` copiava padding de struct nao inicializado para a memoria do
  guest) e `RUSTSEC-2026-0324` (guest derrubava o host com timestamp de
  filesystem anterior a epoch no wasip3). O `Security - cargo audit` agendado
  ficou vermelho em 2026-10-03 com os sete.
  Patch de lockfile apenas: o `Cargo.toml` ja pedia `wasmtime = "49"`, entao
  nao houve mudanca de manifesto nem migracao de API no
  `garraia-plugins/src/runtime.rs`. E o primeiro bump de `wasmtime` em seis que
  **nao** eleva o `rust-version` — a arvore do 49.0.2 (cranelift 0.136.2,
  pulley/wiggle 49.0.2) segue declarando `rustc 1.96.0`, igual a do 49.0.1,
  entao o job `MSRV check (1.96)` e o piso do workspace ficam como estao.
