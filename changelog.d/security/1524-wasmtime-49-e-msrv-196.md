- **wasmtime 48.0.2 → 49.0.1 fecha quatro advisories, e o piso de MSRV sobe para
  1.96 (#1524).** O 49.0.1 corrige `GHSA-m63x-6p34-q65x` e
  `GHSA-jqpg-j7w6-42pr` (combustivel gasto por callees de `call_ref` e por
  lifting de record dinamico deixava de ser contabilizado), `GHSA-c9gc-w9vx-w86p`
  (exaustao de memoria do host na escrita de corpo HTTP de saida) e
  `GHSA-j2g9-4prp-pf6h` (panico com datetime fora de faixa no
  `set-times`/`set-times-at` do filesystem WASI). Sem migracao de API no
  `garraia-plugins/src/runtime.rs` desta vez. A arvore do cranelift 0.136.1
  declara `rustc 1.96.0`, entao o `rust-version` do workspace e o job
  `MSRV check` acompanham — quinto bump com essa forma, e o piso real continua
  ditado pela arvore do wasmtime, nao pelas demais deps.
