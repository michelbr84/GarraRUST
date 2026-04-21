# docs/security/

Documentação técnica de segurança do GarraIA.

## Conteúdo

| Arquivo | Descrição | Status |
|---|---|---|
| [`threat-model.md`](threat-model.md) | STRIDE threat model por componente (Gateway, auth, storage, plugins, channels, mobile) | Draft v1 (next review trimestral) |

## Ver também

- Compliance (LGPD/GDPR): [`../compliance/`](../compliance/README.md)
- ADRs com decisões de segurança:
  - [ADR 0003](../adr/0003-database-for-workspace.md) — Postgres + RLS multi-tenant
  - [ADR 0004](../adr/0004-object-storage.md) — Object storage + 11 políticas de segurança
  - [ADR 0005](../adr/0005-identity-provider.md) — Identity provider (Argon2id + HS256 + BYPASSRLS roles)
- Plans de hardening recentes:
  - [Plan 0021](../../plans/0021-gar-425-workspace-security-hardening.md) — workspace security
  - [Plan 0022](../../plans/0022-gar-426-workspace-security-part-2.md) — rate-limit + audit robustness
  - [Plan 0023](../../plans/0023-gar-427-xff-api-session-ip.md) — XFF fail-closed
  - [Plan 0024](../../plans/0024-gar-412-metrics-endpoint-auth.md) — /metrics auth
  - [Plan 0025](../../plans/0025-gar-411-telemetry-hardening.md) — telemetry REDACT_HEADERS
  - [Plan 0026](../../plans/0026-gar-411-telemetry-part-2.md) — cargo-audit nightly + IAP headers
