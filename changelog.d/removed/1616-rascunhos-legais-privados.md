- **Rascunhos legais e precificacao saem do repositorio publico (#1616).**
  `docs/legal/` (ToS, Privacy, DPA, direitos do titular), `docs/src/pricing.md`
  e `docs/business/cost-model-inputs.md` deixam de ser versionados apos a
  auditoria de exposicao de 2026-10-09 ter encontrado documentos marcados
  "RASCUNHO — NAO PUBLICAR" e o modelo financeiro interno (precos propostos,
  margens, breakeven) na main. Os arquivos continuam no disco local, agora
  cobertos pelo .gitignore como privados por padrao; versoes finais revisadas
  publicam em garraia.org. Historico git e forks existentes nao sao afetados.
