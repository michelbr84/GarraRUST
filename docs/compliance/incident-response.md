# GarraIA — Incident Response Runbook (ANPD / GDPR 72h)

- **Status:** Draft v1 (2026-04-21) — **pending tabletop exercise antes do GA**
- **Owner:** @michelbr84
- **Issue:** [GAR-409](https://linear.app/chatgpt25/issue/GAR-409)
- **Plan:** [`plans/0031-compliance-docs-batch.md`](../../plans/0031-compliance-docs-batch.md)
- **Scope:** incidentes de segurança que impactem dados pessoais em GarraIA (vazamento, exposição indevida, ransomware, indisponibilidade prolongada).
- **Obrigações legais primárias:**
  - **LGPD art. 48** — notificação à ANPD "em prazo razoável"; prática = 48-72h conforme Guia ANPD.
  - **GDPR art. 33** — notificação à autoridade supervisora (DPA) em **72h** a partir do conhecimento.
  - **GDPR art. 34** — comunicação ao titular "sem demora injustificada" quando risco for alto.

---

## Aviso

Este runbook é um **draft técnico** para self-host e cloud deployments. Cada controlador (operador self-host ou operador cloud) deve adaptar à sua realidade organizacional. Escalation paths, contatos DPO e SLAs internos são lacunas a preencher pelo operador.

---

## 0. Preparação (baseline contínuo)

### 0.1 Contatos pré-definidos (preencher antes do GA)

| Papel | Nome | Email | Telefone | Escalation ordem |
|---|---|---|---|---|
| Incident Commander | _TBD_ | _TBD_ | _TBD_ | 1 |
| DPO (Encarregado) | _TBD_ | _TBD_ | _TBD_ | 2 |
| Security engineer on-call | _TBD_ | _TBD_ | _TBD_ | 1 |
| Legal counsel | _TBD_ | _TBD_ | _TBD_ | 3 |
| Communications lead | _TBD_ | _TBD_ | _TBD_ | 4 |
| CEO / fundador | @michelbr84 | michelbr84@users.noreply.github.com | _TBD_ | 2 |

**Autoridades:**
- **ANPD (Brasil)**: notificação via <https://www.gov.br/anpd/pt-br/canais_atendimento/agente-de-tratamento> (canal de comunicação de incidentes de segurança).
- **DPA UE** (se processando dados de europeus): notificação via portal da autoridade do país do estabelecimento principal.

### 0.2 Ferramentas pré-configuradas

- **Monitoring/Alerting:** Grafana + Prometheus (plans 0024/0025 `/metrics` auth + cardinality guards).
- **Log aggregation:** stdout + (opcional) SIEM externo — planejar integração.
- **Communication:** canal dedicado (ex.: Slack/Telegram `#garraia-incident`) criado pre-incident.
- **Status page:** <https://status.garraia.org> (provisionar pre-GA — external blocker).

### 0.3 Acessos de emergência

- Credentials de PostgreSQL `SUPERUSER` (break-glass) em CredentialVault — uso **apenas** sob declaração de incidente.
- Backups offsite rotativos — provider de backup + chave de restauração documentados em `docs/ops/backup.md` (a criar antes do GA).

### 0.4 Exercícios

- Tabletop exercise obrigatório **antes do GA** (acceptance criteria de GAR-409).
- Re-executar anualmente ou após cada mudança major de arquitetura.

---

## 1. Detecção (T0)

### 1.1 Fontes

| Sinal | Canal | Severidade inicial |
|---|---|---|
| Alerta Prometheus: `error_rate > 5%` por 5 min | Grafana → PagerDuty | P2 |
| Alerta: `auth_failure_rate_per_ip > threshold` sustentado | Grafana | P2 (possível brute force) |
| Alerta: `database_connection_pool_exhausted` | Grafana | P1 |
| Log pattern: `FATAL`, `SECURITY VIOLATION`, `RLS POLICY BYPASS` | Log grep | P1 |
| Report externo (usuário reclama, pesquisador de segurança, post em fórum) | `security@garraia.org` (provisionar) | Triagem em 1h, P? |
| Vulnerability disclosure (CVE, cargo-audit alert via GAR-411 L5) | GitHub Actions nightly | Triagem em 24h |
| Cloud provider notifica breach upstream | Email + portal | P1-imediato |

### 1.2 Registro inicial

Criar issue privada em tracker (Linear `GAR-INCIDENT-NNNN` ou label `security-incident`) com:

- T0 (timestamp de detecção).
- Fonte do sinal.
- Hipótese inicial.
- **Commander atribuído** (uma pessoa — não por comitê).

---

## 2. Triagem (T0 → T0+1h)

### 2.1 Classificação de severidade

| Nível | Critério | SLA de notificação |
|---|---|---|
| **P0 — Crítico** | Evidência de exposição de dados pessoais de ≥ 1 usuário confirmada | ANPD em 48h / DPA UE em 72h, titulares em até 72h |
| **P1 — Alto** | Risco alto de exposição; confirmada exposição sem PII; ransomware; serviço down > 1h | ANPD em 72h se PII afetada; tabletop protocol |
| **P2 — Médio** | Degradação de serviço; tentativa de intrusão sem sucesso | Log interno; não requer notificação externa |
| **P3 — Baixo** | Bug de segurança teórico (baixa likelihood) | Backlog |

### 2.2 Decisão "É um incidente de dados pessoais?"

Perguntas obrigatórias, **sim a qualquer uma = incidente de dados pessoais**:

1. Algum dado de tabela `users`, `user_identities`, `sessions`, `messages`, `memory_items`, `files`, `audit_events` pode ter sido exposto a terceiro não-autorizado?
2. Alguma `password_hash`, `refresh_token_hash`, `api_keys.key_hash` ou `group_invites.token_hash` foi extraída?
3. Alguma `body` de message ou `tasks.description` / `task_comments.body` foi acessível a user fora do grupo?
4. Algum `api_keys.scopes` ou `group_invites.invited_email` foi exposto em log público?
5. Algum presigned URL vazou em log público (S3 access log, CDN log)?
6. Alguma `ip_inet` de session foi exposta a terceiro?
7. Algum conteúdo enviado a LLM provider (OpenAI/Anthropic) pode ter vazado upstream (breach do provider)?

**Sim a qualquer** ⇒ P0 ou P1, ativar § 4 notificação.

### 2.3 Estabelecer war room

- Canal dedicado em Slack/Telegram.
- Doc compartilhado (Google Docs ou Notion) com timeline.
- Frequência de updates: a cada 30 min em P0, a cada 2h em P1.

---

## 3. Contenção (T0+1h → T0+24h)

### 3.1 Ações imediatas (por cenário)

| Cenário | Contenção imediata |
|---|---|
| Credential leak detectada (ex.: JWT_SECRET em commit público) | Rotacionar via CredentialVault (GAR-410 path); invalidar todos os refresh tokens; forçar re-auth de todos os users. |
| RLS bypass em produção | Revert do commit; restart de gateway; `SELECT pg_stat_activity` para kill de sessões suspeitas. |
| Brute-force sustentado | Bump rate-limit para `members_manage()` preset; ativar IP-block temporário via `GARRAIA_TRUSTED_PROXIES` + upstream WAF. |
| SQL injection confirmada | Revert; auditar `sqlx::query!` callsites; emergency `GRANT` revogação no role afetado. |
| Ransomware em servidor | Isolar máquina da rede; restaurar de backup offsite; rotacionar todas as credentials. |
| Presigned URL leak mass | Invalidar secret que assina URLs (rota via `GARRAIA_STORAGE_HMAC_SECRET`); reverter presigned URL cache em CDN. |
| LLM provider breach (OpenAI etc.) | Aguardar comunicação oficial do provider; notificar titulares se provider confirmar que messages de users nossos vazaram. |

### 3.2 Preservação de evidência

- **NÃO** deletar logs. Snapshot de `audit_events`, Grafana, container stdout.
- Snapshot de DB (pg_dump + nome timestamped).
- Snapshot de storage buckets se envolvido.
- Documentar **tudo** no doc da war room com timestamps.

---

## 4. Notificação (T0 → T0+72h)

### 4.1 Decisão "Notificar?"

**ANPD (LGPD art. 48 §1)**: notificar quando incidente "possa acarretar risco ou dano relevante aos titulares". Guia ANPD indica **toda suspeita fundada** deve ser notificada — prevalece **precautoriedade**.

**DPA UE (GDPR art. 33.1)**: notificar em 72h "exceto quando for improvável que resulte em risco para os direitos e liberdades das pessoas singulares". Improbabilidade de risco é exceção, não regra.

**Titulares (GDPR art. 34.1 / LGPD art. 48 §2)**: comunicar **quando risco for alto**. Exemplos de "alto risco":
- Password hashes vazados.
- Content de messages com PII sensível exposto.
- Identificação cruzada possível (ip_inet + user_id + messages).

### 4.2 Template de notificação ANPD

```
Assunto: [INCIDENTE DE SEGURANÇA] GarraIA — {YYYY-MM-DD HH:MM UTC}

Prezados,

Comunicamos incidente de segurança com dados pessoais conforme LGPD art. 48.

1. Identificação do controlador:
   - Nome: {Operador da instância GarraIA}
   - CNPJ/CPF: {TBD}
   - Email do DPO: {dpo@operator.example}

2. Descrição do incidente:
   - Data/hora de ocorrência (se conhecida): {T-breach}
   - Data/hora de detecção: {T0}
   - Descrição técnica: {descrição em português, sem jargon desnecessário}

3. Dados afetados:
   - Categorias: {email | password_hash | ip_inet | message body | etc.}
   - Número estimado de titulares afetados: {N}
   - Sensíveis (LGPD art. 5 II)?: {sim/não}

4. Consequências possíveis:
   - {risco específico enumerado}

5. Medidas técnicas e administrativas adotadas:
   - {ações de contenção, cronológicas}

6. Medidas previstas:
   - {comunicação aos titulares, follow-ups}

7. Contatos:
   - Incident Commander: {nome, email, telefone}
   - DPO: {nome, email}

Atenciosamente,
{Assinatura}
```

Enviar via canal oficial vigente da ANPD. URL atual (**verificar antes de usar — ANPD migrou canais em 2024 e pode migrar novamente**): <https://www.gov.br/anpd/pt-br/canais_atendimento/agente-de-tratamento>. Em caso de dúvida do URL, acessar <https://www.gov.br/anpd/> raiz e navegar até "Canais de atendimento → Agente de tratamento".

### 4.3 Template de notificação DPA UE

Use template oficial do país do estabelecimento principal. Conteúdo obrigatório (GDPR art. 33.3):

- Natureza da violação, categorias e número aproximado de titulares + registros afetados.
- Nome + contato do DPO.
- Consequências prováveis.
- Medidas adotadas + propostas.

### 4.5 Janela de 72h expirou com investigação ainda em curso

GDPR art. 33.4 explicitamente permite **notificação faseada** quando não é possível ter todas as informações nas 72h iniciais:

> "Where, and in so far as, it is not possible to provide the information at the same time, the information may be provided in phases without undue further delay."

**Procedimento**:

1. **Ainda em T0+72h**, notifique com informação incompleta + justificativa explícita:
   ```
   Esta é uma notificação inicial. Investigação técnica ainda está em curso.
   Categorias de dados afetadas e escopo exato serão complementados em até
   7 dias corridos, conforme GDPR art. 33.4 / posicionamento ANPD.
   ```
2. **NUNCA** atrase a notificação esperando dados completos. Atraso > 72h é violação por si só.
3. **Envie complemento(s)** em ≤ 7 dias, referenciando o protocolo inicial.
4. **Documente o motivo** da notificação parcial no timeline da war room (útil em auditoria post-hoc).

ANPD aceita mesma abordagem (LGPD art. 48 não especifica prazo rígido, mas "prazo razoável" favorece notificação inicial rápida com complementos).

### 4.4 Comunicação aos titulares (quando requerido)

Email (ou in-app banner) no idioma preferido do usuário. Template:

```
Assunto: Importante — Incidente de segurança em sua conta GarraIA

Olá {display_name},

Em {data}, detectamos um incidente de segurança que pode ter afetado sua conta.

O que aconteceu:
{descrição clara, não técnica}

O que foi afetado:
{lista de dados/categorias}

O que foi feito:
- {ações de contenção}

O que você deve fazer:
- {recomendações: trocar senha, revogar tokens, verificar 2FA, monitorar contas vinculadas}

Como entrar em contato:
- Questões gerais: {support@operator.example}
- Exercer direitos LGPD/GDPR: {dpo@operator.example}

Pedimos desculpas pelo transtorno. Transparência e proteção dos seus dados são prioridades para nós.

— Equipe GarraIA
```

---

## 5. Comunicação externa adicional

### 5.1 Status page

Atualizar <https://status.garraia.org> (pre-GA external blocker) com timeline da mitigação. Atualizações frequentes — não esconder.

### 5.2 Post público

Se incidente for P0 OU tiver atenção de imprensa/comunidade:

- Post em blog oficial (<https://garraia.org/blog>) com post-mortem quando seguro.
- Cross-post em Twitter/X + Mastodon + Fediverse.
- Tom: fatos, sem minimizar, ações concretas, lições aprendidas.

### 5.3 Press

Se imprensa contactar: redirecionar para communications lead. Nunca especular. Preparar statement com legal counsel.

---

## 6. Post-mortem (T0+1 semana)

### 6.1 Estrutura do post-mortem

Arquivo: `docs/incidents/YYYY-MM-DD-slug.md` (a criar em pasta dedicada quando primeiro incidente ocorrer).

Seções obrigatórias:

1. **Timeline** — cronologia detalhada com timestamps.
2. **Impact** — N usuários, N registros, financial impact se aplicável, reputation impact.
3. **Root cause** — ≥ 2 níveis de "5 whys". Nunca "human error" como cause — é sempre sistêmico.
4. **Detection** — quanto tempo passou até detectar? Como poderia ter detectado antes?
5. **Response** — o que funcionou, o que não funcionou.
6. **Action items** — lista numerada com owner + deadline. TODOS com issue Linear tracking.
7. **Lessons learned** — insights replicáveis.

### 6.2 Política de blame-free

Post-mortem é **blame-free**. Ninguém é "culpado". Se uma pessoa pode causar um incidente, o **sistema** é culpado por permitir.

### 6.3 Publicação

- **Interno**: sempre.
- **Público**: se P0/P1, publicar em blog com detalhes suficientes para comunidade aprender, sem expor PII ou vetores de attack ainda não-patchados.

---

## 7. SLAs consolidados

| Evento | SLA |
|---|---|
| Triagem de alerta P0 | 15 min |
| Triagem de report externo | 1 h |
| Contenção inicial P0 | 4 h |
| Notificação ANPD | 72 h (prática 48 h) |
| Notificação DPA UE | 72 h |
| Notificação titulares | 72 h (quando alto risco) |
| Post-mortem rascunho | 1 semana |
| Post-mortem publicado | 2 semanas |
| Action items críticos fechados | 30 dias |

---

## 8. Operador solo (self-host) — escalation simplificada

Para deployments self-host single-operator (família/entidade individual), o fluxo se comprime:

1. Operador é simultaneamente Commander + DPO + Security engineer.
2. Legal counsel externo opcional; notificação ANPD pode ser auto-serviço via canal público ANPD.
3. Titulares afetados = usuários do próprio grupo.

**Ajuste explícito de SLAs para operador solo** (sem aliviar obrigações legais externas):

| Evento | SLA team (§7) | SLA solo (relaxado) | Obrigação legal mantém? |
|---|---|---|---|
| Triagem de alerta P0 | 15 min | 1 h | Sim (legal não estipula) |
| Contenção inicial P0 | 4 h | 24 h | Sim (legal não estipula) |
| Notificação ANPD / DPA UE | 72 h | **72 h (sem relaxamento)** | **Sim — obrigação legal** |
| Notificação titulares | 72 h | **72 h (sem relaxamento)** | **Sim — obrigação legal** |

**Importante**: prazos legais (notificação ANPD, notificação titulares, direitos do titular) **não** são negociáveis por solo. SLAs internos de triagem e contenção podem ser relaxados (porque são baselines internos); mas a janela de 72h para autoridades é lei federal/europeia.

Operador self-host **deve**:
- Configurar backups offsite testados trimestralmente.
- Manter `cargo audit` nightly (plan 0026).
- Documentar sua própria timeline em caso de incidente.
- Ter template de notificação ANPD pré-preenchido em drive pessoal (não dependerá de sistema próprio que pode estar comprometido).

---

## 9. Tabletop exercise template

Cenários sugeridos (executar anualmente):

1. **"JWT_SECRET leaked em commit público"** — praticar rotação + invalidação + notificação.
2. **"RLS bypass descoberto por security researcher"** — praticar coordenação com pesquisador + disclosure coordination.
3. **"Ransomware em servidor Postgres primary"** — praticar restore de backup + business continuity.
4. **"LLM provider (OpenAI) anuncia breach envolvendo nossos dados"** — praticar cascade notification.
5. **"DDoS sustentado por 48h"** — praticar escalation para upstream WAF + comunicação.

Timebox: 2h por exercício. Captar lições em `docs/incidents/exercises/YYYY-MM-DD.md`.

---

## 10. Integração com threat model

Este runbook responde ao **§Threat inventory** do [`threat-model.md`](../security/threat-model.md). Toda ameaça de severidade "Alta" lá tem um cenário concreto aqui.

Quando threat model for revisado (trimestral ou após ADR novo), este runbook é re-auditado na mesma cadence.

---

## Referências

- LGPD art. 48: <https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm#art48>
- GDPR art. 33 + 34: <https://gdpr-info.eu/art-33-gdpr/>
- Guia ANPD sobre Comunicação de Incidente: <https://www.gov.br/anpd/pt-br/documentos-e-publicacoes/guias-orientativos>
- NIST SP 800-61 Rev 2 (Computer Security Incident Handling Guide): <https://csrc.nist.gov/publications/detail/sp/800-61/rev-2/final>
- ENISA Good Practice Guide on Reporting Security Incidents: <https://www.enisa.europa.eu/publications>
- DPIA: [`dpia.md`](dpia.md)
- Threat model: [`../security/threat-model.md`](../security/threat-model.md)
