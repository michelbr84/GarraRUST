- **Cobertura HTTP de anonimização de titular (#1802).** O endpoint destrutivo
  `POST /v1/me/anonymize` agora tem um cenário de integração contra PostgreSQL
  real: `401` sem JWT, `204` sem corpo/PII, e-mail e `provider_sub` com o token
  determinístico completo, status `anonymized`, sessão revogada, convites
  pendente e histórico migrados, auditoria `account.anonymized`, `409` na
  segunda chamada e isolamento de um segundo titular. A suíte usa o router
  autenticado e o harness `test-helpers`, não mock de SQL.
