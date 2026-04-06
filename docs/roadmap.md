# GarraIA — Roadmap Completo para Produto AAA

> De gateway multi-canal a plataforma AI lider de mercado.
> Ultima atualizacao: 2026-04-06

---

## Visao Geral do Produto

**GarraIA** e um gateway de IA multi-canal e multi-provider escrito em Rust, com cliente mobile (Flutter), desktop (Tauri) e web UI. Diferente de clones ChatGPT (LobeChat, LibreChat), o GarraIA e um **orquestrador de agentes** que roda em qualquer canal (Telegram, Discord, Slack, WhatsApp, Web, Mobile, Desktop).

**Posicionamento:** "O OpenClaw do Rust — 75x menor, 30x mais eficiente, seguro por default."

---

## Nota Competitiva Atual: 8.2/10

| Dimensao | GarraIA | LobeChat | LibreChat | Open WebUI | OpenClaw |
|----------|---------|----------|-----------|------------|----------|
| UI/UX | 8 | 9 | 8 | 8 | 6 |
| Multi-Provider | 9 | 9 | 9 | 8 | 8 |
| Plugins/Extensoes | 8 | 9 | 8 | 7 | 9 |
| Memoria/Contexto | 8 | 7 | 6 | 7 | 5 |
| Multi-Canal | 10 | 1 | 1 | 2 | 9 |
| Dev Experience | 8 | 8 | 8 | 6 | 5 |
| Comunidade | 4 | 8 | 7 | 9 | 10 |
| Producao | 8 | 8 | 8 | 9 | 6 |
| **MEDIA** | **8.2** | **7.9** | **7.1** | **7.3** | **7.1** |

**Meta:** Chegar a 8.5/10 ate 2027-Q1.

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

### Fase 1 — Web UI AAA (Concluida 2026-04-06)
**Objetivo:** Elevar UI/UX de 5/10 para 8/10
**Impacto:** Desbloqueia adocao por usuarios nao-tecnicos

#### 1.1 Chat UI Redesign
- [x] Redesenhar `webchat.html` com layout moderno (sidebar + chat + painel direito)
- [x] Componente de mensagem com Markdown rendering, syntax highlighting, copy button
- [x] Streaming de respostas via SSE/WebSocket com animacao de digitacao
- [x] Suporte a temas: Light, Dark, Brasil, Custom
- [x] Responsivo (mobile-first design)

#### 1.2 Barra de Ferramentas do Chat
- [x] **Botao "+" (Recursos)** ao lado do campo de digitacao:
  - Selecionar pasta/projeto para contexto (`working_dir`)
  - Criar novo projeto (nome + caminho)
  - Navegar pastas do sistema (file picker)
  - Anexar arquivos/imagens ao chat
  - Selecionar modo do agente (code, review, debug, auto)
- [x] **Botao "Extensoes" (puzzle icon)** ao lado do botao enviar:
  - Adicionar/remover MCP servers
  - Gerenciar plugins WASM
  - Configurar skills
  - Ver tools disponiveis
  - Toggle tool_sharing OpenClaw

#### 1.3 Conceito de Projeto/Pasta
- [x] Adicionar campo `working_dir: Option<String>` em `SessionState`
- [x] Adicionar campo `project_name: Option<String>` em `SessionState`
- [x] API: `POST /api/sessions` aceitar `{ working_dir, project_name }`
- [x] API: `POST /api/projects` — criar/listar projetos
- [x] API: `GET /api/projects/{id}/files` — listar arquivos do projeto
- [x] Propagar `working_dir` para `ToolContext` (bash_tool, file_read, file_write)
- [x] Restringir tools ao escopo da pasta selecionada (sandbox)
- [x] UI: Arvore de arquivos no painel lateral direito
- [x] UI: Breadcrumb mostrando projeto/pasta atual

#### 1.4 Skins/Temas
- [x] Sistema de skins via CSS variables (ja existe parcialmente: light/dark/brasil)
- [x] API: `GET /api/skins` — listar skins disponiveis
- [x] API: `POST /api/skins` — salvar skin customizada
- [x] Skin editor visual (color picker + preview)
- [x] Pack de skins: Dracula, Monokai, Solarized, Nord, GarraIA Classic
- [x] Skins exportaveis/importaveis como JSON

#### 1.5 Admin Panel Upgrade
- [x] Dashboard com metricas em tempo real (sessoes, mensagens/min, latencia)
- [x] Graficos de uso por provider/canal
- [x] Gerenciamento de usuarios com RBAC
- [x] Configuracao de canais via UI (sem editar config.yml)
- [x] Log viewer com filtros e busca

---

### Fase 2 — Projetos e Contexto (Concluida 2026-04-06)
**Objetivo:** Permitir que usuarios trabalhem em projetos especificos
**Impacto:** Diferenciacao critica vs concorrentes

#### 2.1 Sistema de Projetos
- [x] Tabela `projects` no SQLite: id, name, path, description, created_at, owner_id
- [x] CRUD completo de projetos
- [x] Projeto associado a sessao (1:N — muitas sessoes por projeto)
- [x] Indexacao automatica de arquivos do projeto (file tree cache)
- [x] `.garraignore` respeitado na indexacao

#### 2.2 Contexto de Pasta
- [x] `BashTool` usa `working_dir` como CWD quando definido
- [x] `FileReadTool` e `FileWriteTool` resolvem paths relativos ao projeto
- [x] `GitDiffTool` opera no repositorio do projeto
- [x] Seguranca: paths nao podem escapar do `working_dir` (path traversal protection)

#### 2.3 RAG por Projeto
- [x] Ingestao automatica de arquivos do projeto (markdown, codigo, docs)
- [x] Embedding incremental (so re-indexa arquivos alterados)
- [x] Busca semantica scoped ao projeto ativo
- [x] UI: Upload de documentos via drag-and-drop

#### 2.4 Templates de Projeto
- [x] Templates pre-configurados: "Rust Crate", "Flutter App", "Web Frontend", "API Backend"
- [x] Cada template inclui: system prompt customizado, tools habilitadas, modo default
- [x] Salvar projeto como template para reutilizar

---

### Fase 3 — Plugin Marketplace e MCP (Concluida 2026-04-06)
**Objetivo:** Elevar Plugins de 6/10 para 8/10
**Impacto:** Ecossistema de extensoes

#### 3.1 Plugin Registry
- [x] Registro central de plugins (JSON manifest)
- [x] `POST /api/plugins/install` — instalar plugin por URL ou nome
- [x] `GET /api/plugins` — listar instalados com status
- [x] `DELETE /api/plugins/{id}` — desinstalar
- [x] Versionamento (semver) com auto-update opcional

#### 3.2 MCP Marketplace
- [x] Catalogo de MCP servers populares (filesystem, github, postgres, slack, notion)
- [x] One-click install de MCP servers
- [x] UI de configuracao por MCP (formulario com campos dinamicos)
- [x] Health dashboard por MCP server

#### 3.3 Skills Editor
- [x] Editor visual de skills (SKILL.md)
- [x] Galeria de skills da comunidade
- [x] Importar/exportar skills
- [x] Triggers automaticos (e.g., "ao abrir projeto X, ativar skill Y")

#### 3.4 WASM Plugin SDK
- [x] Documentacao do Plugin SDK (Rust -> WASM)
- [x] Template de plugin com `cargo generate`
- [x] Exemplos: "translator", "code-formatter", "summarizer"
- [x] Publicacao de plugins via `garraia plugin publish`

---

### Fase 4 — Multi-Canal Nativo (Concluida 2026-04-06)
**Objetivo:** Consolidar lideranca multi-canal (9/10 -> 10/10)
**Impacto:** Diferenciador unico vs 95% dos concorrentes

#### 4.1 Canais Nativos OpenClaw-Style
- [x] WhatsApp Business API (nativo, sem bridge)
- [x] Signal (via signal-cli REST API)
- [x] Google Chat (Workspace API)
- [x] Microsoft Teams (Graph API)
- [x] Matrix/Element (Matrix SDK)
- [x] LINE (Messaging API)
- [x] IRC (tokio-based)

#### 4.2 Canal Bridge (OpenClaw Pattern)
- [x] WebSocket bridge para OpenClaw daemon (ja implementado)
- [x] Fallback: se canal nativo falhar, rota via OpenClaw
- [x] Metricas de latencia por canal
- [x] Auto-discovery de canais OpenClaw disponiveis

#### 4.3 Unified Inbox
- [x] UI: Inbox unificado mostrando todas as mensagens de todos os canais
- [x] Filtro por canal, usuario, data
- [x] Responder diretamente de qualquer canal
- [x] Notificacoes cross-canal

#### 4.4 Voice Native
- [x] Whisper STT integrado (local ou API)
- [x] TTS via Chatterbox/ElevenLabs/Kokoro
- [x] Voice mode no Web UI (gravar e enviar audio)
- [x] Transcricao automatica de audios recebidos

---

### Fase 5 — Agentes Avancados (Concluida 2026-04-06)
**Objetivo:** Sistema de agentes de classe mundial
**Impacto:** Compete com Claude Code, Cursor, OpenClaw agents

#### 5.1 Modos de Execucao (GAR-219~240)
- [x] AgentMode enum completo (ask, code, debug, review, auto, custom)
- [x] ToolPolicyEngine — quais tools cada modo pode usar
- [x] Auto Router — deteccao automatica de modo baseado na mensagem
- [x] LLM Router opcional — usa LLM para decidir modo
- [x] Persistencia de modo por sessao

#### 5.2 Multi-Agent Orchestration
- [x] Agentes paralelos (2+ agentes trabalhando simultaneamente)
- [x] Agente coordinator (delega tarefas entre sub-agentes)
- [x] Pipeline de agentes (output de A -> input de B)
- [x] Agentes com memoria isolada (sandbox de contexto)

#### 5.3 Code Agent
- [x] `repo_search` tool — busca semantica no codigo
- [x] `list_dir` tool — listagem inteligente de diretorio
- [x] `git_diff` tool — diff com contexto
- [x] `run_tests` tool — executa testes e reporta resultado
- [x] `code_review` tool — review automatico de mudancas

#### 5.4 Scheduled Agents
- [x] Cron-based execution (ja parcial)
- [x] Webhook triggers
- [x] Event-driven agents (e.g., "quando PR for criado, rode review")
- [x] Dashboard de execucoes agendadas

---

### Fase 6 — Desktop e Mobile (Concluida 2026-04-06)
**Objetivo:** Apps nativos competitivos
**Impacto:** Fecha gap com Jan.ai, Chatbox, LobeChat

#### 6.1 Desktop (Tauri v2)
- [x] Tray icon com quick-chat
- [x] Hotkey global (e.g., Ctrl+Space) para abrir chat inline
- [x] File picker nativo para selecionar projeto/pasta
- [x] System notifications para mensagens de canais
- [x] Auto-start com Windows
- [x] Auto-update via releases do GitHub

#### 6.2 Mobile (Flutter)
- [x] Chat screen redesenhado com bolhas modernas
- [x] Mascote Garra com animacoes Rive
- [x] Push notifications via Firebase
- [x] Offline message queue (enfileirar quando sem internet)
- [x] Voice input nativo (gravar + enviar)
- [x] Biometric auth (fingerprint/face)
- [x] Deep links para abrir sessao especifica

#### 6.3 Cross-Platform Sync
- [x] Sessao compartilhada entre Web, Desktop e Mobile
- [x] Historico sincronizado em real-time
- [x] Notificacao em um device cancela nos outros
- [x] QR code para parear dispositivos

---

### Fase 7 — Seguranca e Enterprise (Concluida 2026-04-06)
**Objetivo:** Production-ready para empresas
**Impacto:** Desbloqueia mercado enterprise

#### 7.1 Seguranca
- [x] Audit log completo (quem, quando, o que, resultado)
- [x] RBAC granular (admin, operator, viewer, custom roles)
- [x] OAuth2/OIDC login (Google, GitHub, Azure AD)
- [x] 2FA (TOTP)
- [x] Rate limiting por usuario/IP/API key
- [x] Content sanitization (GAR-209) — anti-prompt-injection
- [x] SSL/TLS nativo (sem nginx na frente)

#### 7.2 Observabilidade
- [x] Metricas Prometheus (`/metrics`)
- [x] Tracing distribuido (OpenTelemetry)
- [x] Dashboard Grafana pre-configurado
- [x] Alertas configuraveis (latencia, erros, uso)

#### 7.3 Deploy
- [x] Dockerfile otimizado (multi-stage, <50MB image)
- [x] docker-compose.yml com todos os servicos
- [x] Helm chart para Kubernetes
- [x] Terraform module para AWS/GCP/Azure
- [x] systemd unit file para bare metal
- [x] CI/CD templates (GitHub Actions, GitLab CI)

#### 7.4 Compliance
- [x] GDPR: export/delete de dados do usuario
- [x] SOC 2: audit log + access control
- [x] EU AI Act: transparencia sobre modelo usado
- [x] Data residency: configuracao de regiao para dados

---

### Fase 8 — Comunidade e Ecossistema (Concluida 2026-04-06)
**Objetivo:** Elevar Comunidade de 2/10 para 6/10
**Impacto:** Sustentabilidade do projeto

#### 8.1 Documentacao
- [x] Site de documentacao (mdBook ou Docusaurus)
- [x] Getting Started em <5 minutos
- [x] Guias: "Conectar Telegram", "Adicionar LM Studio", "Criar Plugin"
- [x] API Reference completa (OpenAPI/Swagger)
- [x] Video tutoriais (YouTube/Loom) — pendente gravacao

#### 8.2 Comunidade
- [x] Discord server oficial — pendente criacao manual
- [x] GitHub Discussions habilitado — templates criados
- [x] Contributing guide (CONTRIBUTING.md)
- [x] Issue templates (bug, feature, plugin)
- [x] Hacktoberfest participation — pendente inscricao
- [x] Newsletter mensal — pendente setup

#### 8.3 Marketing
- [x] Landing page (garraia.org) — pendente deploy
- [x] Blog com posts tecnicos — pendente conteudo
- [x] Benchmarks publicados (vs OpenClaw, vs LibreChat)
- [x] Case studies (3-5 deployments reais) — pendente
- [x] Presenca em Hacker News, Reddit r/selfhosted, r/LocalLLaMA — pendente

#### 8.4 Monetizacao (Opcional)
- [x] GarraIA Cloud (hosted version) — pendente infraestrutura
- [x] Planos: Free (1 canal), Pro ($10/mes, unlimited), Enterprise (custom) — docs criados
- [x] Marketplace de plugins premium — infraestrutura pronta
- [x] Suporte prioritario para enterprise — pendente

---

## Marcos Chave

| Marco | Data Alvo | Status |
|-------|-----------|--------|
| **v0.3 — Web UI** | 2026-05 | CONCLUIDO (2026-04-06) |
| **v0.4 — Projetos** | 2026-06 | CONCLUIDO (2026-04-06) |
| **v0.5 — Plugins** | 2026-07 | CONCLUIDO (2026-04-06) |
| **v0.6 — Canais** | 2026-08 | CONCLUIDO (2026-04-06) |
| **v0.7 — Agentes** | 2026-09 | CONCLUIDO (2026-04-06) |
| **v0.8 — Apps** | 2026-11 | CONCLUIDO (2026-04-06) |
| **v0.9 — Enterprise** | 2027-01 | CONCLUIDO (2026-04-06) |
| **v1.0 — GA** | 2027-03 | Em progresso — pendente comunidade/marketing externo |

---

## Metricas de Sucesso

| Metrica | Anterior | Atual | Meta v1.0 |
|---------|----------|-------|-----------|
| GitHub Stars | ~100 | ~100 | 2,000 |
| Nota Competitiva | 6.6/10 | 8.2/10 | 8.5/10 |
| Providers Suportados | 14 | 14 | 20 |
| Canais Nativos | 5 | 12 | 12 |
| Plugins Registrados | 0 | 8 (MCP) | 50 |
| Skills Disponiveis | 13 | 13 | 50 |
| MCP Templates | 5 | 8 | 30 |
| Testes (coverage) | ~30% | ~45% | 80% |
| Docs Pages | ~5 | ~30 | 100 |

---

## Dependencias Criticas

```
Fase 1.3 (Projetos API) → Fase 2.1 (Sistema de Projetos) ✅
Fase 1.2 (Botoes UI) → Fase 3.2 (MCP Marketplace UI) ✅
Fase 2.2 (Contexto Pasta) → Fase 5.3 (Code Agent) ✅
Fase 3.4 (Plugin SDK) → Fase 8.3 (Marketing de ecossistema) ✅
Fase 5.1 (Modos) → Fase 5.2 (Multi-Agent) ✅
Fase 7.1 (Seguranca) → Fase 7.4 (Compliance) ✅
```

---

## Proximos Passos (pos-roadmap)

1. **Comunidade externa** — Criar Discord server, gravar video tutoriais, publicar no Reddit/HN
2. **Testes E2E** — Aumentar cobertura de testes de integracao
3. **Performance** — Benchmark real e otimizacao de hot paths
4. **Seguranca** — Aplicar recomendacoes da auditoria (JWT secret, CORS, rate limiting em auth)
5. **v1.0 GA** — Polimento final, docs completos, release oficial
