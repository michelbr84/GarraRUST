# GarraIA — Data Protection Impact Assessment (DPIA)

- **Status:** Draft v1 (2026-04-21) — **pending external legal review antes do GA**
- **Owner:** @michelbr84
- **Issue:** [GAR-399](https://linear.app/chatgpt25/issue/GAR-399)
- **Plan:** [`plans/0031-compliance-docs-batch.md`](../../plans/0031-compliance-docs-batch.md)
- **Scope:** GarraIA stack (gateway + workspace + mobile + desktop) em deployment self-host OU cloud que processa dados pessoais de usuários brasileiros (LGPD) e/ou europeus (GDPR).
- **Supersedes:** none
- **Next review:** antes do GA da Fase 6 + anualmente + após cada novo tipo de dado pessoal coletado.

---

## Aviso legal

Este DPIA é um **draft técnico auto-gerado** (Claude-produzido em sessão autônoma) como baseline de análise. **Não substitui review legal profissional.** Antes do GA (Fase 6):

1. Revisão por advogado especializado em LGPD (Brasil) e/ou GDPR (UE).
2. Validação com o DPO (Encarregado pelo Tratamento de Dados Pessoais — LGPD art. 41) quando a função for designada.
3. Consulta à ANPD se a operação for considerada "alto risco" (LGPD art. 38).

Referências usadas:
- LGPD (Lei 13.709/2018) texto completo: <https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm>
- GDPR (Regulation 2016/679) texto completo: <https://gdpr-info.eu/>
- Guia DPIA ANPD 2024: <https://www.gov.br/anpd/pt-br/documentos-e-publicacoes/guias-orientativos>
- Guia DPIA WP29 → EDPB: <https://ec.europa.eu/newsroom/article29/items/611236>

---

## 1. Identificação da atividade de tratamento

| Item | Descrição |
|---|---|
| **Nome da atividade** | GarraIA Group Workspace — gateway de IA multi-canal + mobile client + desktop |
| **Controlador (LGPD art. 5 VI / GDPR art. 4.7)** | Operador da instalação self-host OU entidade que opera o endpoint cloud. **Em deployment self-host, o próprio usuário assume o papel de controlador.** Em cloud hospedado pelo projeto, o projeto é controlador. |
| **Operador (LGPD art. 5 VII / GDPR art. 4.8)** | GarraIA (enquanto software), Cloudflare (R2 — se adotado), AWS (se adotado), outros cloud providers S3-compat. |
| **Finalidade (LGPD art. 6 I / GDPR art. 5.1.b)** | Prover assistente de IA compartilhado por grupo/família/equipe com memória contextual, arquivos compartilhados e histórico de conversas. |
| **Base legal (LGPD art. 7 / GDPR art. 6)** | **Execução de contrato** (art. 7 V LGPD / art. 6.1.b GDPR) — usuário se cadastra e aceita TOS. **Consentimento** (art. 7 I LGPD / art. 6.1.a GDPR) para uso de dados em treinamento (opt-in explícito, não presumido). |
| **Duração do tratamento** | Até revogação de consentimento, pedido de exclusão (LGPD art. 18 V / GDPR art. 17), ou fim da conta. |
| **Países de tratamento** | Definido pela escolha de cloud provider do operador. Self-host: país do usuário. Cloud: informar via `docs/storage.md` deploy guide. Transferência internacional documentada separadamente se ocorrer. |

---

## 2. Inventário de dados pessoais por tabela

Fonte: schema Postgres em `crates/garraia-workspace/migrations/*.sql` + schema SQLite legacy em `crates/garraia-db/migrations/*.sql`.

### 2.1 `users` (tenant root)

| Coluna | Tipo de dado | Classificação LGPD | Base legal | Retenção |
|---|---|---|---|---|
| `id` | UUID | Identificador interno | Execução de contrato | Até erasure request |
| `email` | Pessoal (art. 5 I LGPD) | **Dado comum** | Execução de contrato | Até erasure request |
| `display_name` | Pessoal | Dado comum | Execução de contrato | Até erasure request |
| `created_at` | Pessoal (proxy de atividade) | Dado comum | Legítimo interesse (audit) | 6 anos após close (retention de audit fiscal) |
| `deleted_at` | Pessoal | Dado comum | Obrigação legal (LGPD art. 16) | Mantido como tombstone após soft-delete |

### 2.2 `user_identities`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `user_id` | FK | Identificador | Execução de contrato | Até erasure |
| `provider` | Enum (internal, oauth-google, etc.) | Não-pessoal | — | Até erasure |
| `identity_ref` | Email ou sub ID do provider | **Pessoal** | Execução de contrato | Até erasure |
| `password_hash` | Argon2id / PBKDF2 | **Credencial (não-reversível)** | Segurança (LGPD art. 46) | Até erasure |
| `hash_upgraded_at` | Timestamp | Metadata | Audit | Até erasure |

### 2.3 `sessions`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `user_id` | FK | Identificador | Execução de contrato | Sliding window |
| `refresh_token_hash` | HMAC-SHA256 | **Credencial** | Segurança | Até logout / expire |
| `ip_inet` | INET | **Pessoal (pode identificar em conjunto)** | Legítimo interesse (antifraude) [**LIA pendente**](#lia-legitimate-interests-assessment-pendente) | 90 dias após session close |
| `user_agent` | TEXT | **Pessoal (proxy fingerprint)** | Legítimo interesse (antifraude) [**LIA pendente**](#lia-legitimate-interests-assessment-pendente) | 90 dias |
| `created_at`, `last_seen_at`, `expires_at` | Timestamp | Metadata | Audit | 90 dias |

### 2.4 `group_members`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `user_id` | FK | Identificador | Execução de contrato | Até leave/erasure |
| `group_id` | FK | Não-pessoal | — | Lifetime do grupo |
| `role` | Enum (owner/admin/member/guest) | Permissão | — | — |
| `status` | Enum (active/removed) | Metadata | Audit | — |

### 2.5 `messages` (FTS indexed)

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `user_id` | FK | Identificador | Execução de contrato | Configurável por grupo |
| `body` | TEXT | **Pessoal + potencial sensível** (LGPD art. 5 II) — pode conter dados de saúde, orientação sexual, políticos, etc., dependendo da conversa | Consentimento + execução de contrato | `groups.settings_jsonb.retention_days` (default 730 dias = 2 anos) |
| `body_fts` | tsvector | Derivado | — | Junto com body |
| `tool_calls` | JSONB | Metadata | Legítimo interesse | Com body |

**Atenção especial**: mensagens podem conter **dados pessoais sensíveis** (LGPD art. 5 II / GDPR art. 9). O operador **DEVE** documentar em TOS que sensíveis podem emergir em conversas e obter **consentimento específico** (LGPD art. 11 / GDPR art. 9.2.a) quando aplicável.

### 2.6 `memory_items` + `memory_embeddings`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `content` | TEXT | **Pessoal (derivado de messages)** | Consentimento | Mesma retenção de messages OR erasure explícita |
| `embedding` | vector(768) | **Derivado** | Consentimento | Mesma |
| `scope` | Enum (user/group/chat) | Metadata | — | — |

### 2.7 `files` + `file_versions` (quando shipado, ADR 0004)

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `name`, `mime`, `size_bytes` | Metadata | Pessoal (associa user) | Execução de contrato | Até erasure |
| `object_key` (em storage) | Conteúdo | **Pode ser pessoal ou sensível** | Consentimento | Até erasure |
| `checksum_sha256`, `integrity_hmac` | Hash | Metadata | Segurança | Até erasure |

### 2.8 `audit_events`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `caller_id` | FK | Identificador | Audit | 6 anos (ver §"Justificativa do período de 6 anos" abaixo) ou até erasure do user (pseudonimização) |
| `group_id` | FK | — | — | — |
| `action` | Enum (`invite.accepted`, `member.role_changed`, `member.removed`, futuros `file.*`) | Metadata | Audit | 6 anos |
| `metadata` | JSONB | Variável — pode conter IP, target_user | Legítimo interesse | 6 anos |

**Justificativa do período de 6 anos para `audit_events`:**
- Código Civil brasileiro art. 205: prescrição geral de 10 anos para pretensões pessoais; adotamos 6 anos como baseline pragmático alinhado com retenção fiscal (Lei 8.212/91 art. 46 + Decreto 3.048/99 art. 225 — obrigações previdenciárias).
- GDPR art. 5.1.e: retention "no longer than necessary". Operador DEVE reavaliar caso-a-caso; o default de 6 anos deve ser documentado no TOS e sujeito a review legal.
- Pseudonimização (substituir `caller_id` por token) é obrigatória após erasure do user, mantendo os eventos operacionais para forensics de grupo sem re-identificação.

### 2.9 `mobile_users` (legacy, SQLite)

Em uso atual (GAR-335). **Migrar para schema `users` + `user_identities` quando plano `garraia-cli migrate workspace` (GAR-413) executar.** Mesma classificação que §2.1+2.2.

Colunas específicas:

| Coluna | Classificação | Base legal | Retenção |
|---|---|---|---|
| `email` | Dado comum | Execução de contrato | Até erasure |
| `password_hash` | Credencial (não-reversível) | Segurança | Até erasure |
| `salt` | Credencial auxiliar (PBKDF2 salt externo) | Segurança | Até erasure (deletar junto com password_hash) |

### 2.10 `api_keys`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `user_id` | FK | Identificador | Execução de contrato | Até revoke / erasure |
| `key_hash` | Argon2id hash | **Credencial** | Segurança | Até revoke |
| `scopes` | JSONB | Permissões | — | Até revoke |
| `name` | TEXT | Pessoal (label de usuário) | Execução de contrato | Até revoke |
| `created_at`, `last_used_at`, `revoked_at` | Timestamp | Metadata | Audit | Até erasure do user |

**Observação**: api_keys são credenciais de longo prazo. Revogação NÃO apaga `last_used_at` (mantido para forensics pelo período de retention de audit), mas apaga `key_hash` (não reversível).

### 2.11 `group_invites`

| Coluna | Tipo | Classificação | Base legal | Retenção |
|---|---|---|---|---|
| `group_id` | FK | Não-pessoal | — | Lifetime do grupo |
| `invited_email` | Email | **Pessoal** (de titular que pode nem ser user ainda) | Execução de contrato + legítimo interesse (onboarding) | 7 dias pós-expiry do invite OU aceitação (drop email após accept) |
| `token_hash` | Argon2id hash | **Credencial** (acesso ao grupo) | Segurança | Até expiry / accepted_at |
| `invited_by_user_id` | FK | Identificador do inviter | Execução de contrato | Até erasure do inviter (tombstone) |
| `expires_at`, `accepted_at` | Timestamp | Metadata | Audit | Até purge (7 dias pós-terminal) |

**Atenção**: `invited_email` pode referir-se a pessoa que **nunca se tornou usuário** — o direito ao apagamento se aplica mesmo sem conta criada.

### 2.12 `task_lists`, `tasks`, `task_assignees`, `task_comments`, `task_activity` (migration 006)

| Tabela / Coluna | Classificação | Base legal | Retenção |
|---|---|---|---|
| `tasks.title`, `tasks.description` | **Pessoal + potencial sensível** (pode mencionar saúde, família, finanças pessoais) | Consentimento + execução de contrato | Configurável por grupo (mesma regra de messages) |
| `task_assignees.user_id` | Identificador | Execução de contrato | Lifetime da task |
| `task_comments.body` | **Pessoal + potencial sensível** (conversa sobre task) | Consentimento + contrato | Mesma de messages |
| `task_activity.changes` JSONB | **Pessoal derivado** (histórico de quem mudou o quê) | Audit | Lifetime da task |

Tasks compartilham surface de privacidade com `messages`: conteúdo livre, risco de dados sensíveis emergentes. **Right-to-erasure** deve cobrir todas as 5 tabelas (com anonimização de user ids via token em `task_activity`).

### 2.13 `roles`, `permissions`, `role_permissions`, `groups`

Tabelas de metadados não-pessoais **por si só**. `groups.settings_jsonb` pode conter policies de retention, digest, idioma — sem PII direta. Entram no inventário pelo papel de "contexto de tratamento" de dados pessoais vinculados via `group_members`.

---

## 3. Fluxo de dados

```
[user]
  │ HTTPS (TLS 1.3 target GAR-383)
  ▼
[garraia-gateway]
  ├─► Postgres: inserts em users/messages/memory (RLS enforced)
  ├─► Redis (optional, planned): rate-limit buckets + session cache
  ├─► LLM provider (OpenAI/Anthropic/Ollama): mensagem é enviada como parte de payload — provider pode reter se política deles permitir (user informado via TOS).
  ├─► Object storage (S3/MinIO/LocalFs) — encrypted at rest SSE-S3
  └─► Audit log (Postgres audit_events)

[mobile app]
  │ HTTPS
  ▼
[garraia-gateway]
  └─► mesma árvore
```

### Transferência internacional (LGPD art. 33 / GDPR Chapter V)

- **Self-host**: transferência depende da escolha de cloud do usuário. Se LLM provider é OpenAI/Anthropic (EUA), há transferência. Usuário deve ser informado.
- **Cloud hosted pelo projeto**: se gateway roda em provider EU (ex.: Cloudflare EU regions), residência é EU. LLM provider transfere para EUA — informar + usar Standard Contractual Clauses (SCCs) do provider.

---

## 4. Riscos identificados + mitigações

### 4.1 Vazamento de dados em conversa (I — Information disclosure)

- **Cenário**: mensagem contendo dado sensível (ex.: saúde) é enviada ao LLM provider terceiro (OpenAI/Anthropic). Provider pode reter conforme política interna.
- **Risco**: alto impacto, média likelihood.
- **Mitigação atual**: usuário informado via TOS; providers com "do not retain" opt-in disponível (OpenAI API tier) devem ser preferidos. Local-first (Ollama, mistral.rs — ADR 0001) elimina esse caminho.
- **Mitigação planejada**: `garraia-config` ganha flag `agent.sensitive_data_filter = true` que aplica `presidio` ou regex PII scrub antes de enviar ao provider (plan futuro Fase 5).

### 4.2 Cross-tenant leakage via authorization bug (E + I)

- **Cenário**: bug em handler ou policy RLS permite user do grupo A ler messages do grupo B.
- **Risco**: alto impacto, baixa likelihood.
- **Mitigação atual**: RLS FORCE em 10 tabelas; authz matrix HTTP (plan 0014) com 15+ cenários; compile-time non-interchangeable `AppPool`/`LoginPool`/`SignupPool` newtypes; `SET LOCAL app.current_user_id` transacional.
- **Mitigação planejada**: fuzzing contínuo de authz matrix (GAR-402 cargo-fuzz expand).

### 4.3 Credential stuffing / password brute force (S + E)

- **Cenário**: atacante usa lista de password leaks para tentar login em massa.
- **Risco**: médio impacto, alta likelihood (attacks contínuos).
- **Mitigação atual**: rate limit per-IP + per-user (plan 0022); Argon2id torna offline-hash-cracking caro.
- **Mitigação planejada**: verificar contra HIBP Pwned Passwords API em signup (opt-in).

### 4.4 Right-to-erasure não cumprido (direitos do titular)

- **Cenário**: usuário pede erasure (LGPD art. 18 V / GDPR art. 17) mas sistema retém dados por padrão (ex.: v1 de files, audit trail).
- **Risco**: alto impacto legal, baixa likelihood.
- **Mitigação atual**: ADR 0004 §Versionamento explicita que erasure tem flag `--include-origin`; audit_events separado do user pode ser mantido pseudonimizado via tokenização.
- **Mitigação planejada**: `POST /v1/me:anonymize` endpoint (GAR-400) que substitui PII por tokens sem apagar histórico do grupo.

### 4.5 Secrets em log / debug output (I)

- **Cenário**: `GARRAIA_JWT_SECRET` ou `OPENAI_API_KEY` leak via log de erro.
- **Risco**: crítico impacto, baixa likelihood.
- **Mitigação atual**: `SecretString` wrapper em `garraia-config`; `RedactedStorageError`; `REDACT_HEADERS` em HTTP middleware (plan 0025/0026).
- **Mitigação planejada**: GAR-410 (CredentialVault single source) elimina reads diretos de env vars.

### 4.6 Perda de disponibilidade (D)

- **Cenário**: Postgres outage, provider LLM outage, rate limit bypass.
- **Risco**: médio impacto, média likelihood.
- **Mitigação atual**: `/metrics` com cardinality guard (plan 0025); idempotent init em telemetry (plan 0025); fail-soft em gateway startup quando componente opcional falha.
- **Mitigação planejada**: multi-region replicas + health probes em Fase 6.

---

## 5. Fluxo de direitos do titular (LGPD art. 18 / GDPR Chapter III)

### 5.1 Confirmação de existência + acesso (LGPD art. 18 I, II / GDPR art. 15)

- **Endpoint**: `GET /v1/me` — já existente (plans 0015, 0016).
- **Export completo**: `GET /v1/me:export` — **pendente** (GAR-400). Deve retornar zip com messages/files/memory/tasks/audit em ≤ 30s para 10k messages.
- **SLA**: 15 dias corridos (LGPD art. 19) / 1 mês (GDPR art. 12.3).

### 5.2 Correção (LGPD art. 18 III / GDPR art. 16)

- **Endpoint**: `PATCH /v1/me` — **pendente**.
- Correção de `display_name` + `email` (com re-verificação).

### 5.3 Anonimização / bloqueio / eliminação (LGPD art. 18 IV, V / GDPR art. 17, 18)

- **Endpoint anonimização**: `POST /v1/me:anonymize` — **pendente** (GAR-400). Substitui PII em audit_events por tokens, preserva histórico agregado do grupo.
- **Endpoint delete**: `DELETE /v1/me` — **pendente** (GAR-400). Soft-delete com tombstone + hard-delete após 30 dias + purge de files com `--include-origin` (ADR 0004).
- **SLA**: 15 dias (LGPD) / 1 mês (GDPR).

### 5.4 Portabilidade (LGPD art. 18 V / GDPR art. 20)

- **Endpoint**: mesmo `GET /v1/me:export` (GAR-400). Formato: JSON estruturado + arquivos originais (zip).

### 5.5 Informações sobre compartilhamento (LGPD art. 18 VII)

- Documentar em TOS quais providers LLM são acionados por request. Expor em `/v1/me/providers:used` (list de providers que processaram dados do user nos últimos 30 dias) — plano futuro.

### 5.6 Revogação de consentimento (LGPD art. 8 §5 / GDPR art. 7.3)

- **Endpoint**: `DELETE /v1/me/consent/{scope}` — plano futuro. Scopes: `training`, `analytics`, `profiling`.

---

## 6. RoPA (Registro de Operações de Tratamento — LGPD art. 37 / GDPR art. 30)

Tabela consolidada mínima (para expansão pelo DPO):

| Operação | Finalidade | Base legal | Dados tratados | Destinatários | Retenção | Transferência internacional |
|---|---|---|---|---|---|---|
| Cadastro de usuário | Execução de contrato | Art. 7 V LGPD / 6.1.b GDPR | email, password_hash, display_name | Próprio sistema | Até erasure | Não |
| Envio de mensagem para LLM | Prestação de serviço de IA | Consentimento + contrato | body da message, system prompt | LLM provider (OpenAI/Anthropic/Ollama/OpenRouter) | Retenção da message + política do provider | Sim se provider é US (OpenAI/Anthropic) — SCCs |
| Armazenamento de file | Prestação de serviço | Contrato | binary + metadata | Object storage (LocalFs/S3/MinIO) | Até erasure | Depende do provider |
| Audit log | Obrigação legal + segurança | Art. 7 II + VI LGPD | user_id, action, metadata (ip, UA) | Próprio sistema | 6 anos | Não |
| Autenticação | Segurança | Art. 7 VI LGPD + art. 46 | email, password_hash, ip, UA | Próprio sistema | Até erasure | Não |

---

## 6.1. LIA (Legitimate Interests Assessment) pendente

O DPIA invoca "legítimo interesse" (LGPD art. 7 IX / GDPR art. 6.1.f) como base legal para retenção de `ip_inet` + `user_agent` em `sessions` (finalidade antifraude). **Antes do GA**, o operador deve conduzir um LIA formal conforme orientação da EDPB:

1. **Purpose test** — finalidade é legítima? (antifraude → sim, há interesse legítimo de proteção do serviço contra credential stuffing e session hijack).
2. **Necessity test** — o tratamento é necessário? (IP + UA são sinais mínimos para detectar fraude; alternativas como fingerprinting de device seriam mais invasivas).
3. **Balancing test** — o interesse do operador supera o direito à privacidade do titular? (documentar em `docs/compliance/lia-sessions.md` a criar, especialmente em relação a retention de 90 dias).

Sem o LIA documentado, a invocação de legítimo interesse é frágil perante auditoria ANPD. Este DPIA v1 **marca** mas **não substitui** o LIA.

## 6.2. Contrato de processamento de dados (DPA / Cláusulas de processamento)

**Cenário cloud-hosted** (projeto opera gateway como serviço para terceiros):

- GarraIA (projeto) atua como **operador** (LGPD art. 5 VII / GDPR art. 4.8, "processor").
- Cliente/Operador da instância cloud é **controlador**.
- GDPR art. 28 requer **contrato escrito** (DPA) entre controlador e processor detalhando: duração, natureza, finalidade, categorias de dados, obrigações de confidencialidade, sub-processors, assistência ao controlador, retorno/destruição de dados, audit rights.
- Template de DPA é **pendente** (a criar em `docs/legal/dpa-template.md` antes de qualquer oferta cloud comercial).

**Cenário self-host** (operador instala GarraIA em infra própria):

- Operador self-host **é o controlador**. Não há relação de processor com o projeto GarraIA — apenas o fornecimento de software.
- Providers upstream (OpenAI, Anthropic, Cloudflare, AWS) continuam sendo sub-processors do operador self-host; contratos separados com cada um aplicam-se.

## 7. Medidas técnicas e administrativas (LGPD art. 46 / GDPR art. 32)

**Técnicas:**
- TLS 1.3 (pending GAR-383) para dados em trânsito.
- AES-256-GCM no CredentialVault para secrets em repouso.
- SSE-S3 / disk-level encryption para object storage (ADR 0004 §Security #2).
- Argon2id para credenciais (ADR 0005).
- HMAC-SHA256 para integrity anti-tampering (ADR 0004 §Security #4).
- RLS FORCE + compile-time pool newtypes.
- Audit logs.
- Rate limiting + XFF fail-closed (plan 0022).
- `REDACT_HEADERS` em telemetria (plan 0025/0026).

**Administrativas:**
- ADRs formais para decisões arquiteturais (CLAUDE.md regra 8).
- Code review + security-auditor agent reviews em PRs (rule 10).
- Threat model trimestral ([`threat-model.md`](../security/threat-model.md)).
- Runbook de incidentes ([`incident-response.md`](incident-response.md)).
- Cargo-audit nightly (plan 0026).
- Plans aprovados antes de código ([`plans/README.md`](../../plans/README.md) §Regras).

---

## 8. Status de compliance

| Item | LGPD | GDPR | Status | Blocker |
|---|---|---|---|---|
| Base legal documentada | ✅ | ✅ | Draft | Legal review |
| TOS explicita base legal | ⚠️ | ⚠️ | Pendente | Template de TOS |
| Direito de acesso implementado | ⚠️ | ⚠️ | Parcial (`/v1/me` existe) | `:export` (GAR-400) |
| Direito de exclusão implementado | ❌ | ❌ | Pendente | GAR-400 |
| Direito de portabilidade | ❌ | ❌ | Pendente | GAR-400 |
| RoPA publicado | ✅ v1 | ✅ v1 | Draft aqui | Expansão por DPO |
| Runbook incidentes 72h | ✅ v1 | ✅ v1 | [incident-response.md](incident-response.md) | Tabletop exercise |
| Threat model documentado | ✅ | ✅ | [threat-model.md](../security/threat-model.md) | Re-review trimestral |
| DPO designado | ❌ | ❌ | Pendente | Decisão organizacional |
| DPIA revisado por legal | ❌ | ❌ | Pendente | Contratação legal |

---

## 9. Próximos passos

1. **Contratar review legal** (advogado LGPD/GDPR) para validar este DPIA antes do GA.
2. **Designar DPO** conforme LGPD art. 41 (pessoa física ou jurídica).
3. **Implementar GAR-400** (endpoints de export/delete/anonymize) — destrava compliance operacional.
4. **Publicar TOS + Privacy Policy** em `garraia.org/legal`.
5. **Executar tabletop exercise** do runbook de incidentes (GAR-409) pelo menos uma vez antes do GA.
6. **Revisar este DPIA trimestralmente** — próxima: 2026-07-21.

---

## Referências

- LGPD (Lei 13.709/2018): <https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm>
- GDPR: <https://gdpr-info.eu/>
- ANPD Guia DPIA: <https://www.gov.br/anpd/pt-br>
- EDPB Guidelines on DPIA: <https://edpb.europa.eu/our-work-tools/our-documents/guidelines/guidelines-042017-assessment-impact-data-protection_en>
- ADR 0003 Postgres multi-tenant: [`../adr/0003-database-for-workspace.md`](../adr/0003-database-for-workspace.md)
- ADR 0004 Object storage: [`../adr/0004-object-storage.md`](../adr/0004-object-storage.md)
- ADR 0005 Identity Provider: [`../adr/0005-identity-provider.md`](../adr/0005-identity-provider.md)
- Threat model: [`../security/threat-model.md`](../security/threat-model.md)
- Incident response runbook: [`incident-response.md`](incident-response.md)
