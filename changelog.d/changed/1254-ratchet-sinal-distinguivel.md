- **O comentario do Quality Ratchet passa a dizer o que mudou NESTA PR, nao so
  vs baseline (#1254).** Como o comparador so olhava `.quality/baseline.json`
  (congelado em 2026-05-05), toda PR recebia o mesmo texto — a #1235 foi
  acusada de `files_over_700` 34 → 36 sem ter tocado em nada. `compare.py`
  ganha `--base <merge-base-metrics.json>`: com ele cada linha da tabela traz a
  coluna `Δ nesta PR` (current menos merge-base) e o relatorio abre com o
  veredito `Sem regressao nova nesta PR` ou `REGRESSAO NOVA nesta PR: <metrica>
  <delta>`, listando so o que piorou dentro da propria PR; a comparacao contra
  o baseline continua, rotulada `vs baseline (<data>) — ver #1254`, e cada
  regressao dela diz se e nova, pre-existente no merge-base ou nao mensuravel
  la (metrica nao coletada no base — a cobertura no CI, cujo `lcov.info` so
  existe no checkout da PR); nesse terceiro caso o veredito e ⚠️, nao ✅,
  porque o relatorio nao afirma pre-existencia do que nunca mediu. O
  `quality-ratchet.yml` coleta as metricas do `pull_request.base.sha` num
  worktree separado (`GARRAIA_REPO_ROOT=/tmp/base`, com os parsers da propria
  PR) e passa `--base` so em `pull_request`; em `push` para `main` o relatorio
  e byte-identico ao de antes. Exit code nao muda: `report-only` segue sempre 0
  e `enforce` segue decidido so pelo baseline. Quando `frozenAt` do baseline
  esta a mais de 90 dias de `collected_at` do current — calculo sobre os dois
  campos, sem relogio, deterministico — entra a linha WARN `baseline_age_days`
  apontando a #1254. O `baseline.json` nao foi tocado: re-baseline continua
  sendo decisao do dono.
