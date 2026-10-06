- `quality-report.md` trazia `coverage_pct: None` em todo run, e a causa nao
  era falta de instrumentacao: o job `ratchet` (timeout 10 min, ~3 min de
  trabalho) sempre olha antes do job `coverage` do `ci.yml` (timeout 45 min,
  21-30 min de execucao) terminar, e o filtro `conclusion == "success"`
  descartava o run ainda em andamento. A busca passa a varrer os ultimos 20
  runs por um `completed`+`success` e, quando o SHA da PR ainda nao fechou,
  cai para o ultimo run verde de `main`, gravando a procedencia do
  `lcov.info` no step summary (com `NOT this PR` explicito no fallback).
  Continua best-effort: nenhuma falha ali derruba o job. Novo baseline
  versionado em `docs/coverage-baseline-2026-10.md` fixa os numeros reais —
  71,99% de linha no agregado, por crate, com a politica de exclusao aceita
  e a meta do ROADMAP reavaliada. (#1566)
