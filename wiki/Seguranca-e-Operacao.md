# Segurança e Operação

## Reportar vulnerabilidade

**Não abra issue pública.** Reporte de forma privada via [GitHub Security Advisories](https://github.com/michelbr84/GarraRUST/security/advisories/new) ou **security@garraia.org**. Política completa e o que incluir no reporte: [SECURITY.md](https://github.com/michelbr84/GarraRUST/blob/main/SECURITY.md).

## Modelo de segurança

- [Visão geral](https://github.com/michelbr84/GarraRUST/blob/main/docs/security.md) — cofre AES-256-GCM, allowlists por canal, pareamento, bind em localhost por padrão
- [Hardening do gateway + ferramentas de execução](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md) — perfis loopback/exposto, confirmação de comandos (safety_gate), modos/ToolPolicy, MCP com política ([receitas](https://github.com/michelbr84/GarraRUST/blob/main/docs/integrations/mcp-capacidades.md)) e [config de referência](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml)
- [Arquitetura security-first](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/architecture.md) · [Superfícies de ataque de agentes de IA](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/attack-surfaces.md) · [Checklist prático](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/checklist.md)
- [Threat model STRIDE](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [Decisões de hardening](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/hardening-decisions.md)

## Runbooks

| Situação | Runbook |
|---|---|
| Segredo vazou num commit | [secret-scanning-runbook.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/secret-scanning-runbook.md) |
| Incidente com dados pessoais (ANPD/GDPR, 72h) | [compliance/incident-response.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/compliance/incident-response.md) |
| Operação em produção | [production-runbook.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/production-runbook.md) |
| CodeQL (setup + supressões) | [codeql-setup.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/codeql-setup.md) · [codeql-suppressions.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/codeql-suppressions.md) |
| Proteção da branch main | [protect-main-ruleset.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/protect-main-ruleset.md) |

## Compliance e observabilidade

- [DPIA — LGPD/GDPR](https://github.com/michelbr84/GarraRUST/blob/main/docs/compliance/dpia.md)
- [Telemetria (OpenTelemetry + Prometheus)](https://github.com/michelbr84/GarraRUST/blob/main/docs/telemetry.md)
- [Deploy: Docker](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment.md) · [Runpod serverless](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment-runpod.md)
