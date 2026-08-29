# docs/security/

Documentação técnica de segurança do GarraIA.

## Conteúdo

| Arquivo | Descrição | Status |
|---|---|---|
| [`threat-model.md`](threat-model.md) | STRIDE threat model por componente (Gateway, auth, storage, plugins, channels, mobile) | Draft v1 (next review trimestral) |
| [`codeql-setup.md`](codeql-setup.md) | Setup avançado do CodeQL (3 linguagens, config, toggle do default setup, onda de 2026-08-28, triagem programática) | Ativo desde 2026-04-30 |
| [`.github/workflows/codeql-triage.yml`](../../.github/workflows/codeql-triage.yml) + [`scripts/security/codeql-alert-report.py`](../../scripts/security/codeql-alert-report.py) | Lê os alertas de code scanning de dentro do repo (`security-events: read`) e agrega por regra, separando produção de teste | `workflow_dispatch`, desde 2026-08-29 |
| [`codeql-suppressions.md`](codeql-suppressions.md) (+ [`.json`](codeql-suppressions.json)) | Ledger versionado de supressões (dismissal via API + script de reapply) — 22 entradas | ⚠️ Re-audit de 90 dias vencido em 2026-08-01 |
| [`secret-scanning-runbook.md`](secret-scanning-runbook.md) | Resposta a segredos vazados em commits | Ativo |
| [`protect-main-ruleset.md`](protect-main-ruleset.md) (+ [`.json`](protect-main-ruleset.json)) | Ruleset do `main` (PR obrigatório, 4 checks, zero bypass) — fonte de verdade versionada | Ativo |
| [`hardening-decisions.md`](hardening-decisions.md) | Decisões de postura de segurança | Ativo |
| [`codeql-suppressions.json`](codeql-suppressions.json) → script: [`scripts/security/codeql-reapply-dismissals.sh`](../../scripts/security/codeql-reapply-dismissals.sh) | Reaplicação idempotente dos dismissals | Manual (agenda em GAR-491.2) |

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
