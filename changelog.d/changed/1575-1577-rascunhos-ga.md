- **Rascunhos GA de documentos legais e operacionais (#1574, #1575, #1576, #1577).**
  Primeira leva dos documentos que faltavam para vender, todos marcados como
  rascunho/proposta — nenhum é válido sem a revisão/decisão humana indicada no
  próprio arquivo. `docs/legal/dpa-template.md` (template de DPA com
  suboperadores e processo de atualização), `docs/legal/privacy-policy-draft.md`
  (política de privacidade com os direitos implementados no gateway) e
  `docs/legal/tos-draft.md` (termos de serviço); `docs/operations/`
  com `support-policy.md`, `version-support-policy.md`,
  `deprecation-policy.md` e `sla-decision-brief.md` (decisão default registrada:
  self-host sem SLA, com opções para o maintainer); `docs/business/cost-model-inputs.md`
  (insumos BYOK de custo para a precificação, sem decidir preço). O DPIA ganha
  correção factual: os quatro endpoints de direitos do titular
  (`GET /v1/me/export`, `PATCH /v1/me`, `POST /v1/me/anonymize`,
  `DELETE /v1/me`) estão implementados no gateway — a lista de pendências do
  documento estava desatualizada.
