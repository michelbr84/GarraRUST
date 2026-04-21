# Plan 0031 — Compliance docs batch (STRIDE + DPIA + Runbook)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues fechadas:** GAR-398, GAR-399, GAR-409
**Branch:** `feat/0031-compliance-docs-batch`

---

## 1. Goal

Entregar os 3 artefatos documentais de compliance + segurança que o ROADMAP Fase 5 lista como **pré-requisito de GA** (Fase 6): threat model STRIDE, DPIA (LGPD/GDPR), e runbook de incidentes ANPD/GDPR com janela de 72h. Os três são doc-only, zero código, e representam backlog hygiene de alta alavancagem: destravam auditoria externa + review legal pré-GA + auditoria ANPD.

## 2. Non-goals

- **Não implementa** os endpoints de direito do titular (GAR-400 — endpoints `export/delete/anonymize` — fica para plano futuro).
- **Não roda** tabletop exercise (GAR-409 acceptance criteria menciona isso; tabletop é facilitado por humano).
- **Não substitui** review legal externo (DPIA diz "review legal antes do GA" — humano).
- **Não toca** em código Rust/Flutter.

## 3. Scope

Arquivos criados:
- `docs/security/threat-model.md` (GAR-398) — STRIDE por componente
- `docs/compliance/dpia.md` (GAR-399) — Data Protection Impact Assessment
- `docs/compliance/incident-response.md` (GAR-409) — Runbook ANPD/GDPR 72h

Arquivos atualizados:
- `plans/README.md` — adiciona entrada 0031.
- `docs/compliance/README.md` — cria índice se não existir; linka DPIA + incident-response.
- `docs/security/README.md` — cria índice se não existir; linka threat-model.

## 4. Acceptance criteria

1. **STRIDE**: cada um dos 6 componentes (Gateway HTTP/WS, garraia-auth, garraia-storage placeholder, garraia-plugins WASM, garraia-channels, mobile apps) tem matriz preenchida com Spoofing/Tampering/Repudiation/Info disclosure/DoS/Elevation + mitigação atual + mitigação planejada.
2. **DPIA**: inventário de dados pessoais por tabela (users, messages, files, memory_items, sessions, user_identities, group_members, audit_events), finalidade, base legal (LGPD art. 7 / GDPR art. 6), retenção, riscos, mitigações, fluxo de direitos do titular, RoPA (Registro de Operações de Tratamento).
3. **Runbook ANPD**: 6 fases (detecção → triagem → contenção → notificação → comunicação → post-mortem) com SLAs explícitos + templates de notificação ANPD/autoridade UE + critérios de decisão "notificar ou não".
4. `docs/compliance/README.md` + `docs/security/README.md` funcionam como índices navegáveis.
5. Zero referência a código não-existente; onde mencionar código futuro (GAR-400, GAR-410), linkar para a issue Linear.
6. Review: `@security-auditor` APPROVE (threat model é alto leverage para segurança).

## 5. Work breakdown

| Task | Arquivo | Estimativa |
|---|---|---|
| T1 | `plans/0031-compliance-docs-batch.md` | 5 min |
| T2 | `docs/security/threat-model.md` | 25 min |
| T3 | `docs/compliance/dpia.md` | 25 min |
| T4 | `docs/compliance/incident-response.md` | 20 min |
| T5 | `docs/security/README.md` + `docs/compliance/README.md` | 5 min |
| T6 | `plans/README.md` update | 3 min |
| T7 | Review + commit + PR | 10 min |

Total: ~90 min.

## 6. Verification

- Cada doc tem seção "Status" explícito (draft v1 / needs legal review / etc.).
- Tabelas usam Markdown GFM válido.
- Links internos funcionam (`[ADR 0005]` etc.).
- Termos LGPD/GDPR citados com artigo numerado (art. 7, art. 18, art. 46 LGPD; art. 6, art. 17, art. 32 GDPR).

## 7. Rollback plan

100% reversível via revert. Doc-only, zero state externo.

## 8. Risk assessment

| Risco | Likelihood | Impact | Mitigação |
|---|---|---|---|
| DPIA contém afirmação errada sobre base legal | Médio | Alto | Marcar como "draft v1, pending legal review" em cabeçalho. |
| Threat model sugere mitigação incorreta | Baixo | Médio | security-auditor review obrigatório. |
| Runbook prevê SLA irrealista de 72h para operação solo | Médio | Baixo | Ajustar para realidade atual (single-operator) + documentar upgrade path quando equipe crescer. |

## 9. Open questions

Nenhuma — drafts v1 com disclaimers explícitos de "pending review".
