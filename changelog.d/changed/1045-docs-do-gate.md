- docs: `hardening-gateway.md`, `auth-config.md`, `mobile-qa-checklist.md` e o
  README do app passam a descrever o gate do REST como ele ficou: com
  `gateway.api_key` configurada, `/api/*` exige `Authorization: Bearer`, com
  `/api/health`, `/api/capabilities` e `/api/auth-check` abertas para o
  onboarding funcionar antes de haver chave. Sem chave configurada, nada
  muda. (#1045)
