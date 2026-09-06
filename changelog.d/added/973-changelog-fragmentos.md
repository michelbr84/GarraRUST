- **Fragmentos de changelog em `changelog.d/` (#973).** Todo par de PRs
  paralelos colidia na secao `[Unreleased]` do `CHANGELOG.md` — duas vezes so
  na sessao de 2026-09-05, e numa delas as entradas foram parar dentro da
  versao ja publicada porque o contexto do hunk sobreviveu ao rename da secao.
  A causa e estrutural: dois PRs que anexam linhas na mesma secao do mesmo
  arquivo conflitam sempre. Agora cada PR deixa um arquivo proprio em
  `changelog.d/<secao>/<numero>-<slug>.md`, e `scripts/changelog/assemble.py`
  junta tudo no passo de release — respeitando secao existente e nunca
  escrevendo dentro de uma versao ja publicada.
