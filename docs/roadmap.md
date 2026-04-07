# GarraIA — Roadmap Completo para Produto AAA

> De gateway multi-canal a plataforma AI lider de mercado.
> Ultima atualizacao: 2026-04-07

---

## Visao Geral do Produto

**GarraIA** e um gateway de IA multi-canal e multi-provider escrito em Rust, com cliente mobile (Flutter), desktop (Tauri) e web UI. Diferente de clones ChatGPT (LobeChat, LibreChat), o GarraIA e um **orquestrador de agentes** que roda em qualquer canal (Telegram, Discord, Slack, WhatsApp, Web, Mobile, Desktop).

**Posicionamento:** "O OpenClaw do Rust — 75x menor, 30x mais eficiente, seguro por default."

---

## Nota Competitiva Atual: 7.5/10

| Dimensao | GarraIA | LobeChat | LibreChat | Open WebUI | OpenClaw |
|----------|---------|----------|-----------|------------|----------|
| UI/UX | 6 | 9 | 8 | 8 | 6 |
| Multi-Provider | 9 | 9 | 9 | 8 | 8 |
| Plugins/Extensoes | 7 | 9 | 8 | 7 | 9 |
| Memoria/Contexto | 8 | 7 | 6 | 7 | 5 |
| Multi-Canal | 9 | 1 | 1 | 2 | 9 |
| Dev Experience | 7 | 8 | 8 | 6 | 5 |
| Comunidade | 3 | 8 | 7 | 9 | 10 |
| Producao | 7 | 8 | 8 | 9 | 6 |
| **MEDIA** | **7.0** | **7.9** | **7.1** | **7.3** | **7.1** |

**Meta:** Chegar a 8.5/10 ate 2027-Q1.

---

## Legenda de Status

- [x] Concluido e funcional
- [~] Backend pronto, frontend parcial ou nao integrado
- [ ] Nao implementado

---

## Fases de Execucao

### Fase 0 — Fixes Imediatos (Concluida 2026-04-06)

- [x] Fix Riverpod `Ref` (api_service.dart)
- [x] CSpell `.cspell.json` com dicionario do projeto
- [x] JDK 23 configurado (Gradle 8.14 OK)
- [x] ClaudeMaxPower: 4 hooks + 4 agents + 13 skills + permissions
- [x] OpenClaw: canal nativo `garraia-channels::openclaw` + tool bridge + 4 endpoints API
- [x] Roadmap estrategico atualizado

---

### Fase 1 — Web UI (Backend 2026-04-06, Frontend em progresso)
**Objetivo:** Elevar UI/UX de 5/10 para 8/10
**Impacto:** Desbloqueia adocao por usuarios nao-tecnicos

#### 1.1 Chat UI Redesign
- [x] Redesenhar `webchat.html` com layout moderno (sidebar + chat + painel direito)
- [x] Componente de mensagem com Markdown rendering, syntax highlighting, copy button
- [x] Streaming de respostas via SSE/WebSocket com animacao de digitacao
- [x] Suporte a temas: Light, Dark, Brasil, Dracula, Monokai, Nord
- [x] Responsivo (mobile-first design)

#### 1.2 Barra de Ferramentas do Chat
- [~] **Botao "+" (Recursos)** — HTML existe, event listeners parciais:
  - [~] Selecionar pasta/projeto para contexto (`working_dir`) — API pronta, UI parcial
  - [~] Criar novo projeto (nome + caminho) — API pronta, UI parcial
  - [~] Navegar pastas do sistema (file picker) — API pronta, UI parcial
  - [ ] Anexar arquivos/imagens ao chat — nao implementado
  - [~] Selecionar modo do agente (code, review, debug, auto) — API pronta, UI parcial
- [~] **Botao "Extensoes" (puzzle icon)** — carrega MCP/tools/skills:
  - [x] Listar MCP servers
  - [~] Listar tools (endpoint corrigido /api/mcp/tools)
  - [~] Listar skills
  - [ ] Gerenciar plugins WASM via UI
  - [ ] Toggle tool_sharing OpenClaw via UI

#### 1.3 Conceito de Projeto/Pasta
- [x] Adicionar campo `working_dir: Option<String>` em `SessionState`
- [x] Adicionar campo `project_name: Option<String>` em `SessionState`
- [x] API: `POST /api/sessions/with-project` aceitar `{ working_dir, project_name }`
- [x] API: `POST /api/projects` — criar/listar projetos
- [x] API: `GET /api/projects/{id}/files` — listar arquivos do projeto
- [x] Propagar `working_dir` para `ToolContext` (bash_tool, file_read, file_write)
- [x] Seguranca: paths nao podem escapar do `working_dir` (path traversal protection)
- [~] UI: Arvore de arquivos no painel lateral direito — estrutura existe, wiring parcial
- [~] UI: Breadcrumb mostrando projeto/pasta atual — estrutura existe, wiring parcial

#### 1.4 Skins/Temas
- [x] Sistema de skins via CSS variables
- [x] API: `GET /api/skins` — listar skins disponiveis
- [x] API: `POST /api/skins` — salvar skin customizada
- [x] Skin editor visual (color picker + preview) — funcional
- [x] Pack de skins: Dracula, Monokai, Nord, Light, Dark, Brasil
- [x] Skins exportaveis/importaveis como JSON

#### 1.5 Admin Panel Upgrade
- [x] Dashboard com metricas (sessoes ativas, providers, alertas)
- [x] Graficos de uso por provider/canal (Chart.js)
- [~] Gerenciamento de usuarios com RBAC — backend pronto, UI basica
- [~] Configuracao de canais via UI — formulario existe, sem persistencia
- [x] Log viewer com filtros e busca

---

### Fase 2 — Projetos e Contexto (Backend concluido 2026-04-06)
**Objetivo:** Permitir que usuarios trabalhem em projetos especificos
**Impacto:** Diferenciacao critica vs concorrentes

#### 2.1 Sistema de Projetos
- [x] Tabela `projects` no SQLite: id, name, path, description, created_at, owner_id
- [x] CRUD completo de projetos (project_store.rs, 15 testes)
- [x] Projeto associado a sessao (1:N — muitas sessoes por projeto)
- [x] `.garraignore` respeitado na indexacao

#### 2.2 Contexto de Pasta
- [x] `BashTool` usa `working_dir` como CWD quando definido
- [x] `FileReadTool` e `FileWriteTool` resolvem paths relativos ao projeto
- [x] `GitDiffTool` opera no repositorio do projeto
- [x] Seguranca: `ProjectToolContext` com path traversal protection (5 testes)

#### 2.3 RAG por Projeto
- [x] Tabela `project_files` com embedding BLOB e content_hash
- [x] Embedding incremental (needs_reindex via content_hash)
- [x] Busca semantica via cosine similarity (search_project_files)
- [~] UI: Upload de documentos via drag-and-drop — nao wired

#### 2.4 Templates de Projeto
- [x] Tabela `project_templates` com system_prompt, tools_enabled, default_mode
- [x] CRUD de templates + create_project_from_template
- [~] UI para selecionar/criar templates — nao implementado

---

### Fase 3 — Plugin Marketplace e MCP (Backend concluido 2026-04-06)
**Objetivo:** Elevar Plugins de 6/10 para 8/10
**Impacto:** Ecossistema de extensoes

#### 3.1 Plugin Registry
- [x] Registro central de plugins com JSON manifest
- [x] API: install, list, get, delete, toggle — todos funcionais
- [x] Versionamento (semver) com comparacao

#### 3.2 MCP Marketplace
- [x] Catalogo built-in de 8 MCP servers populares
- [x] API: one-click install, health check, config-schema
- [~] UI de configuracao por MCP — backend pronto, UI nao wired

#### 3.3 Skills Editor
- [x] API: CRUD de skills, import/export, triggers — todos funcionais
- [~] Editor visual de skills — nao implementado no frontend
- [~] Galeria de skills da comunidade — nao implementado

#### 3.4 WASM Plugin SDK
- [x] Documentacao do Plugin SDK (docs/src/guides/plugin-sdk.md)
- [x] Host functions definidas (sdk/host_functions.rs)
- [x] Plugin trait definido (sdk/plugin_trait.rs)
- [~] Template de plugin com `cargo generate` — docs existem, template nao criado
- [~] Publicacao via `garraia plugin publish` — comando CLI nao implementado

---

### Fase 4 — Multi-Canal Nativo (Estrutura 2026-04-06)
**Objetivo:** Consolidar lideranca multi-canal (9/10 -> 10/10)
**Impacto:** Diferenciador unico vs 95% dos concorrentes

#### 4.1 Canais Nativos
- [x] WhatsApp Business API (canal existente, funcional)
- [x] Telegram (canal existente, funcional com voz)
- [x] Discord (canal existente, slash commands)
- [x] Slack (canal existente, Socket Mode)
- [x] iMessage (macOS only, funcional)
- [~] Google Chat — struct implementada, nao testada em producao
- [~] Microsoft Teams — struct implementada, nao testada
- [~] Matrix/Element — struct implementada, nao testada
- [~] LINE — struct implementada, nao testada
- [~] IRC — struct implementada com parser, nao testada
- [~] Signal — struct implementada, nao testada

#### 4.2 Canal Bridge (OpenClaw Pattern)
- [x] WebSocket bridge para OpenClaw daemon
- [x] Fallback routing (send_with_fallback)
- [x] ChannelMetrics com AtomicChannelMetrics (lock-free)

#### 4.3 Unified Inbox
- [~] UI: sidebar mostra sessoes de todos os canais — parcial
- [ ] Filtro por canal, usuario, data
- [ ] Responder diretamente de qualquer canal na UI
- [ ] Notificacoes cross-canal na UI

#### 4.4 Voice Native
- [x] Whisper STT integrado (whisper.cpp server CUDA, large-v3-turbo)
- [x] TTS via LM Studio (vieneu-tts-v2-turbo) + Chatterbox + Hibiki
- [x] TtsSynthesizer trait polimorfco com 3 providers
- [x] WhisperClient dual-endpoint (whisper.cpp /inference + OpenAI /v1/audio)
- [x] Health check condicional por provider ativo
- [~] Voice mode no Web UI (gravar e enviar audio) — nao wired
- [x] Transcricao automatica de audios recebidos (Telegram)

---

### Fase 5 — Agentes Avancados (Concluida 2026-04-06)
**Objetivo:** Sistema de agentes de classe mundial
**Impacto:** Compete com Claude Code, Cursor, OpenClaw agents

#### 5.1 Modos de Execucao
- [x] AgentMode enum (Ask, Code, Debug, Review, Auto, Custom)
- [x] ToolPolicyEngine — quais tools cada modo pode usar
- [x] AutoRouter — deteccao por keywords
- [x] LlmRouter — deteccao via LLM (opcional)
- [x] SessionModeMetadata — persistencia de modo por sessao

#### 5.2 Multi-Agent Orchestration
- [x] AgentCoordinator com spawn_agent + cancel token
- [x] parallel_execute (respeita max_concurrent)
- [x] pipeline_execute (output A -> input B, fail-fast)

#### 5.3 Code Agent (4 novas tools)
- [x] `repo_search` tool — grep + file pattern (usa rg)
- [x] `list_dir` tool — tree com file sizes, skip build dirs
- [x] `run_tests` tool — auto-detect framework (cargo/flutter/npm/pytest)
- [x] `code_review` tool — diff -> LLM review com severity

#### 5.4 Scheduled Agents
- [x] TriggerRegistry com on_webhook e on_event
- [x] EventType: PrCreated, Push, IssueOpened, Custom
- [x] list_scheduled() com dashboard data
- [~] UI dashboard de execucoes agendadas — nao implementado

---

### Fase 6 — Desktop e Mobile (Estrutura 2026-04-06)
**Objetivo:** Apps nativos competitivos
**Impacto:** Fecha gap com Jan.ai, Chatbox, LobeChat

#### 6.1 Desktop (Tauri v2)
- [x] Tray icon com quick-chat e menu completo
- [x] Hotkey global (Alt+G overlay, Ctrl+Space quick-chat)
- [x] File picker nativo (select_project_folder, select_files)
- [x] System notifications (tauri-plugin-notification)
- [x] Auto-start com Windows (toggle em Settings UI)
- [x] Auto-update via GitHub releases (tauri-plugin-updater)
- [x] Quick-chat HTML + Settings HTML

#### 6.2 Mobile (Flutter)
- [x] Chat screen com bolhas modernas, markdown, timestamps
- [x] Mascote Garra com 4 animacoes (idle/thinking/talking/happy)
- [~] Push notifications — stub criado, Firebase nao configurado
- [x] Offline message queue (sqflite + connectivity_plus + auto-retry)
- [x] Voice input widget (hold-to-record, waveform, transcricao)
- [x] Biometric auth (local_auth + PIN fallback)
- [x] Deep links (garraia://chat/{sessionId})
- [x] Flutter analyze: 0 issues

#### 6.3 Cross-Platform Sync
- [x] SyncService com WebSocket + auto-reconnect + heartbeat
- [x] QR code pairing screen (qr_flutter + mobile_scanner)
- [~] Sessao compartilhada real-time — struct pronta, servidor nao wired
- [ ] Notificacao cross-device cancelation

---

### Fase 7 — Seguranca e Enterprise (Backend 2026-04-06)
**Objetivo:** Production-ready para empresas
**Impacto:** Desbloqueia mercado enterprise

#### 7.1 Seguranca
- [x] Audit log completo (AuditEntry com user, action, target, result, IP)
- [x] RBAC granular (Permission enum, CustomRole, has_permission)
- [x] OAuth2/OIDC (Google, GitHub, Azure AD) — routes + handlers
- [x] 2FA TOTP (generate, verify, QR URI) — routes + handlers
- [x] Rate limiting (RateLimiter, sliding window, configurable)
- [x] Content sanitization (anti-prompt-injection patterns)
- [~] Rate limiting em /auth/login e /auth/register — implementado mas nao wired como middleware
- [~] OAuth state TTL — implementado (10min eviction)
- [ ] SSL/TLS nativo built-in (atualmente via reverse proxy)

#### 7.2 Observabilidade
- [x] Metricas Prometheus (`GET /metrics`) — 15 counters/gauges
- [x] OtelConfig + init_tracing + span helpers
- [x] Dashboard Grafana pre-configurado (deploy/grafana/dashboard.json)
- [~] Alertas configuraveis — estrutura pronta, nao wired

#### 7.3 Deploy
- [x] Dockerfile otimizado multi-stage (builder + runtime)
- [x] docker-compose.yml (app + postgres + redis opcionais)
- [x] Helm chart completo (deploy/helm/garraia/)
- [x] Terraform module AWS ECS Fargate (deploy/terraform/)
- [x] systemd unit file (deploy/systemd/garraia.service)
- [x] CI/CD: ci.yml + release.yml + deploy.yml (multi-arch Docker)

#### 7.4 Compliance
- [x] GDPR: export_user_data + delete_user_data (cascade delete)
- [x] data_retention table com TTL
- [~] SOC 2 audit log — log existe, dashboard nao
- [~] EU AI Act transparencia — campo model nos responses, nao formalizado

---

### Fase 8 — Comunidade e Ecossistema (Parcial 2026-04-06)
**Objetivo:** Elevar Comunidade de 2/10 para 6/10
**Impacto:** Sustentabilidade do projeto

#### 8.1 Documentacao
- [x] Site de documentacao (mdBook configurado, book.toml)
- [x] Getting Started em <5 minutos (docs/src/getting-started.md)
- [x] Guias: Conectar Telegram, Adicionar LM Studio, Criar Plugin
- [x] API Reference completa (docs/src/api-reference.md)
- [ ] Video tutoriais (YouTube/Loom)

#### 8.2 Comunidade
- [x] GitHub Discussions habilitado + templates
- [x] Contributing guide (CONTRIBUTING.md)
- [x] Issue templates (bug, feature, plugin)
- [x] PR template (.github/PULL_REQUEST_TEMPLATE.md)
- [ ] Discord server oficial
- [ ] Newsletter mensal

#### 8.3 Marketing
- [x] Benchmarks documentados (docs/src/benchmarks.md)
- [x] Pricing page (docs/src/pricing.md)
- [ ] Landing page (garraia.org)
- [ ] Blog com posts tecnicos
- [ ] Case studies
- [ ] Presenca em Hacker News, Reddit

#### 8.4 Monetizacao (Opcional)
- [~] Docs de planos (Free/Pro/Enterprise) criados
- [ ] GarraIA Cloud (hosted version)
- [ ] Marketplace de plugins premium
- [ ] Suporte prioritario enterprise

---

## Marcos Chave

| Marco | Data Alvo | Status |
|-------|-----------|--------|
| **v0.3 — Web UI** | 2026-05 | Backend OK, frontend parcial |
| **v0.4 — Projetos** | 2026-06 | Backend + DB + testes OK |
| **v0.5 — Plugins** | 2026-07 | Backend OK, UI nao wired |
| **v0.6 — Canais** | 2026-08 | 5 funcionais + 6 structs prontas |
| **v0.7 — Agentes** | 2026-09 | CONCLUIDO (130 testes) |
| **v0.8 — Apps** | 2026-11 | Desktop OK, Mobile 90% |
| **v0.9 — Enterprise** | 2027-01 | Backend OK, wiring parcial |
| **v1.0 — GA** | 2027-03 | Em progresso |

---

## Metricas de Sucesso

| Metrica | Anterior | Atual | Meta v1.0 |
|---------|----------|-------|-----------|
| GitHub Stars | ~100 | ~100 | 2,000 |
| Nota Competitiva | 6.6/10 | 7.0/10 | 8.5/10 |
| Providers Suportados | 14 | 14 | 20 |
| Canais Funcionais | 5 | 5 (+6 structs) | 12 |
| Plugins Registrados | 0 | 8 (MCP catalog) | 50 |
| Skills Disponiveis | 13 | 13 | 50 |
| Testes | ~120 | ~195 | 500 |
| Docs Pages | ~5 | ~30 | 100 |

---

## Prioridades Imediatas

1. **Web UI wiring** — Conectar botoes da sidebar as APIs existentes
2. **Voice UI** — Gravar audio no browser e enviar ao backend
3. **Testes dos novos canais** — Testar Google Chat, Teams, Matrix em prod
4. **Security wiring** — Rate limiter em auth, CORS restrito
5. **Mobile Firebase** — Configurar push notifications reais
6. **v0.3 release** — Tag + binarios + changelog

---

## Dependencias Criticas

```
Fase 1.2 (Botoes UI wiring) → Fase 3.2 (MCP Marketplace UI)
Fase 2.2 (Contexto Pasta) → Fase 5.3 (Code Agent) CONCLUIDO
Fase 5.1 (Modos) → Fase 5.2 (Multi-Agent) CONCLUIDO
Fase 7.1 (Seguranca) → Fase 7.4 (Compliance) Backend OK
Web UI wiring → v0.3 release
```
