# GarraIA vs Hermes Agent vs OpenClaw — Análise Técnica Comparativa

> **Uso interno**: documento de arquitetura e planejamento do GarraIA.
> Não é material promocional. População de usuários, stars, forks e popularidade
> foram **excluídos como critério** por instrução do mantenedor.

---

## Metadados da análise

| Campo | Valor |
|---|---|
| **Data da análise** | 2026-09-15 |
| **GarraIA analisado** | v0.4.2, workspace Rust de 24 crates; `main` em `9929c2c` (2026-09-15) + PRs da sessão #1212–#1221 (7 abertos, 3 merged) |
| **Hermes Agent analisado** | docs oficiais https://hermes-agent.nousresearch.com/docs (set/2026), skill `hermes-agent` v3.2.0 local, runtime em execução nesta máquina; repo público `github.com/NousResearch/hermes-agent` (código-fonte **não** inspecionado linha a linha) |
| **OpenClaw analisado** | repo `github.com/openclaw/openclaw` (pushed 2026-09-15), docs `docs.openclaw.ai` (set/2026); análise documental + GitHub API — código-fonte não auditado linha a linha |
| **Repositórios** | GarraIA: `github.com/michelbr84/GarraRUST` · Hermes: `github.com/NousResearch/hermes-agent` · OpenClaw: `github.com/openclaw/openclaw` |

### Metodologia

1. GarraIA: análise direta do código local (crates, ADRs 0001–0022, ROADMAP, CI, tests).
2. Hermes: docs oficiais (`/docs` + `/docs/llms.txt`), skill interna comprovada no filesystem e **runtime real em execução** nesta sessão (tools observadas de primeira mão).
3. OpenClaw: páginas oficiais de docs + GitHub API (linguagens, licença, atividade). Código-fonte TypeScript não auditado diretamente — itens inferidos de docs estão marcados.
4. Regra de evidência: funcionalidade confirmada **só** quando vista em código, docs oficiais ou execução real; caso contrário: `❓ Não confirmado` / `N/A`.

### Limitações da comparação

- OpenClaw: análise documental; o agent loop interno e a telemetria não foram verificados em código (`docs.openclaw.ai/concepts/agent-loop` não lida a fundo).
- Hermes: código-fonte do repo não inspecionado diretamente; afirmações derivam de docs oficiais + runtime observável.
- Eficiência: **não existem benchmarks comparáveis** entre os três; números do GarraIA vêm dos benches internos (`benches/agent-framework-comparison/`), não de medições cruzadas.
- Documentação muda rápido (OpenClaw fez rebrand triplo em 3 dias — parte da indexação está desatualizada).
- Hermes e OpenClaw evoluem por releases frequentes; qualquer conclusão aqui vale para o snapshot de 2026-09-15.

---

## Perfil rápido dos três sistemas

| | **GarraIA (GarraRUST)** | **Hermes Agent** | **OpenClaw** |
|---|---|---|---|
| Linguagem | Rust (edition 2024, MSRV 1.95) | Python 3.11+ | TypeScript/Node 24 (22.19+), apps nativos Swift/Kotlin |
| Identidade | Assistente pessoal **local-first** com persona própria (Garra/Hera, pt-BR), hardware/IoT nativo | Runtime de agente pessoal autônomo com gateway multiplataforma e learning loop de skills | Gateway self-hosted unificador de canais de chat com agentes de código |
| Licença | Proprietária/privada (repo do mantenedor) | MIT (docs) | `Other`/NOASSERTION — **não é licença OSI padrão**; verificar LICENSE antes de uso corporativo |
| Modelo padrão | OpenRouter (fallback Ollama local) | Provider-agnostic (Nous Portal, OpenRouter, OpenAI/Anthropic/compat) | Anthropic/OpenAI/etc. — LLM tipicamente em nuvem |
| Instalação | Binário nativo único (6+ alvos, .deb/.rpm/AppImage/MSI/NSIS) | `curl install.sh` (uv/venv) | install.sh/install.ps1, npm/pnpm/bun, Docker, Nix |
| Estado nesta data | 0 issues abertas, CI verde, 7 PRs na fila | Estável em produção neste host | Muito ativo (rebrand recente: Clawdbot → Moltbot → OpenClaw) |

---

## Tabela principal

Indicadores: ✅ estável · 🟡 parcial/experimental · 🔵 planejado · ❌ ausente · ❓ não confirmado · N/A

| Categoria | GarraIA | Hermes Agent | OpenClaw | Observações |
|---|---|---|---|---|
| **Arquitetura base** | ✅ Monorepo Rust, 24 crates (CLI+gateway+domínios), binário único | ✅ Runtime Python single-core + superfícies (CLI/TUI/desktop/dashboard/ACP) | ✅ Gateway-daemon único (Node) com WS tipado (TypeBox→JSON Schema), clientes macOS/iOS/Android | Garra e OpenClaw seguem o padrão "um processo por host"; Hermes é runtime por instância + gateway acoplado |
| **Local-first** | ✅ Estado 100% local; sem phone-home (só release-check silenciável) | ✅ Tudo em `~/.hermes`; serverless opcional | 🟡 Gateway/dados locais, mas **LLM tipicamente em nuvem**; "self-hosted" refere-se ao plano de controle | Só Garra assume inferência local como caminho primário (Ollama/GGUF default de fallback) |
| **Multi-tenant** | ✅ Group Workspace com memória separada por chat/grupo | 🟡 Perfis isolados (`~/.hermes/profiles/<name>/`) | ❌ Um trust boundary por gateway — explicitamente não-adversarial | Diferencial arquitetural real do Garra (família/equipe num host) |
| **Providers LLM** | ✅ Trait `LlmProvider`: Anthropic, OpenAI-compat (OpenRouter/vLLM/LM Studio), Ollama, llama.cpp; 15 declarados | ✅ Nous Portal, OpenRouter, OpenAI, Anthropic, Google, DeepSeek, xAI + OpenAI-compat; credential pools rotativos | ✅ Dezenas (docs/providers): OpenAI, Anthropic, Gemini, xAI, Groq, Mistral, DeepSeek, Qwen, Bedrock, LiteLLM, Ollama, llama.cpp, vLLM, SGLang… | Hermes e OpenClaw têm catálogo mais largo; Garra tem **auto-quantização local por VRAM** que os outros não têm |
| **Fallback/roteamento** | ✅ `provider_resilience.rs` (retry 429/502/503, failover primário→backup via config) + `auto_router.rs` heurístico 🟡 | ✅ Fallback entre providers, modelos auxiliares baratos p/ tarefas internas, `hermes model` em runtime | ✅ ClawRouter multi-provider + failover por agente | Auto-roteador do Garra é heurístico por keywords — não modelo; Hermes/OpenClaw roteiam por config/portal |
| **Sub-agentes/delegação** | ✅ `AgentCoordinator`: handles tokio, `parallel_execute`, `pipeline_execute`, 9 modos com ToolPolicy aplicada no executor | ✅ `delegate_task` (contexto+terminal isolados, batch paralelo, leaf/orchestrator, **não-durável**) + subagentes via processos Hermes independentes + worktrees | ✅ Sub-agents em background, ACP (Codex/Claude Code), Swarm fan-out, "parallel specialist lanes" | Todos têm; Hermes deixa claro que filho background morre com o pai — Garra usa cancel tokens; OpenClaw tem ledger de background tasks |
| **Multi-agent persistente** | 🟡 `garra team` / MaxPower com TDD e worktrees; A2A cliente | ✅ Kanban durável em SQLite com dispatcher, auto-block por falha; Bot Mode nomeado | ✅ Agentes isolados por `agentDir` + session store próprio + bindings de canal→agente | OpenClaw tem a separação por agente mais formal; Hermes tem o workflow mais pronto (kanban) |
| **Tools nativas** | ✅ 13 tools no runtime (bash, file, repo_search, git_diff, run_tests, web_search/fetch, device…) + budget de execução + approval gates | ✅ terminal (7 backends), execute_code (kernel persistente), browser, vision, TTS, session_search, delegation… | ✅ Runtime/Files/Web/Browser/Messaging/Media/ask_user + plugins registrando tools | Hermes tem o toolset de "productivity" mais amplo; Garra tem tools de **dispositivo** que os outros não têm |
| **Sandbox de execução** | 🟡 execution_budget + approval gates + ADR 0019 (process hardening); WASM sandbox p/ plugins | 🟡 redact + allowlist + approvals smart; execute_code sem sandbox formal | ✅ Docker/Podman/SSH/OpenShell/Crabbox por tool/agente + policy pré-model-call | OpenClaw tem o sandbox mais maduro; Garra tem WASM p/ plugins mas shell roda direto |
| **MCP** | ✅ Bidirecional: cliente (stdio, tool_bridge c/ guard de injection) + servidor `garra mcp-server` (stdio, `garra_ask`) | ✅ Cliente nativo stdio/HTTP + sampling/createMessage + métricas por server; **sem hot-reload** | ✅ Bidirecional: cliente (stdio/SSE/HTTP + OAuth/mTLS/filtros) + servidor expondo conversas dos canais | Os três suportam; OpenClaw tem o cliente mais completo (OAuth, mTLS, toolFilter) |
| **Skills** | ✅ garraia-skills + **auto-aprendizado** (miner/generator/evaluator/safety/versioning, ADR 0010) | ✅ SKILL.md padrão agentskills.io + **curator** (ciclo de vida, telemetry de uso, archive/restore) | ✅ SKILL.md + Skill Workshop (agente redige, operador aprova) + **ClawHub marketplace** | Três learning loops distintos; só OpenClaw tem marketplace com threat model próprio |
| **Memória** | ✅ SQLite + sqlite-vec (vetorial embutido), híbrida (semântica+keyword), extração de fatos c/ knobs (`auto_extract`, `max_facts`), multi-tenant, retention worker | ✅ MEMORY.md/USER.md curados com teto rígido (~1.300 tokens), FTS5 de sessões, providers externos plugáveis; **sem auto-compact** | ✅ Markdown plano (MEMORY/USER/diárias) + `memory_search` vetorial opcional + **Dreaming** (promoção automática curto→longo) | Filosofias: Garra=vetorial embutida; Hermes=curada mínima; OpenClaw=arquivos humanos |
| **Context engineering** | ✅ context_policy por modo, context_summarizer c/ modelo barato, memória híbrida no prompt | ✅ invariantes anti-mutação p/ prompt caching, compressão automática, tools diferidas | ✅ Bootstrap por arquivos (AGENTS/SOUL/USER/MEMORY), memory flush pré-compaction, Tool Search/Code Mode | OpenClaw mais sofisticado em escala de catálogo; Hermes mais rigoroso em cache |
| **Automação** | ✅ schedule tool + recurrence two-tier (ADR 0013) + automations de hardware (cron/expr) | ✅ cron durável (frases/ISO/5 campos, delivery multiplataforma, no_agent scripts) + webhooks + serverless hibernante | ✅ Automations scheduler + Heartbeat (turno periódico system-owned) + Task Flow + hooks de ciclo de vida + trigger IMAP | OpenClaw tem o leque de triggers mais largo (IMAP, heartbeat); Hermes o cron mais flexível |
| **Interfaces** | ✅ CLI (REPL+ask), gateway web (chat/admin/learning), desktop Tauri v2 (overlay/tray/hotkeys), REST v1 + shims OpenAI/Anthropic, mobile Flutter, Termux | ✅ CLI, TUI Ink, desktop Electron, dashboard web, ACP p/ IDEs, slash commands | ✅ CLI, Control UI web, apps macOS/iOS/Android (nodes), WebChat, A2UI canvas | Hermes e OpenClaw têm apps desktop móveis/nativos maduros; desktop do Garra está no M1/M2 de 7 milestones |
| **Canais de mensagem** | ✅ 12 (Telegram, Discord, Slack, WhatsApp, iMessage, Signal, Matrix, IRC, Teams, Google Chat, Line, +) num binário[^node-whatsapp] | ✅ 19+ nativas + IRC/Teams por plugin, com acesso pleno a tools | ✅ WhatsApp (Baileys), Telegram, Slack, Discord, Signal, iMessage, Matrix, Teams, Google Chat, Zalo, Mattermost, WebChat | Cobertura de canal é ponto comum forte dos três; Garra lidera em canais por binário único |
| **Execução local/LLM local** | ✅ Ollama + llama.cpp GGUF c/ auto-quantização por VRAM (Q4_K_M/Q5_K_M/Q8_0), embeddings locais, STT/TTS locais | ✅ execução local de comandos; LLM local via OpenAI-compat; não é foco | ✅ Ollama/llama.cpp/vLLM/SGLang suportados; não é o padrão | Garra é o único com **pipeline de quantização automática** e voz local |
| **Hardware/IoT** | ✅ Crate `garraia-hardware` (ADR 0020): adapters GPIO/MQTT/Home Assistant/serial, capability/risk por ação, automations engine, skills de hardware | 🟡 Via Home Assistant (canal) e terminal/MCP; sem abstração própria | 🟡 Nodes pareados (câmera/tela/localização); sem MQTT/HA nativos | **Diferencial central do Garra** — nada equivalente nos outros |
| **Segurança** | ✅ CredentialVault AES-256-GCM (→argon2), prompt-injection guard (~20 padrões) em web_fetch/MCP/file_read/hardware, TLS passthrough, rate limit, TOTP, RLS testada, LGPD/GDPR endpoints, cargo-deny/audit/CodeQL | ✅ redact_secrets (não desligável pelo LLM), redact PII, approvals smart/manual, allowlist de shell, vault p/ browser, env filtering MCP | ✅ Security audit c/ auto-fix, threat model MITRE ATLAS, pairing de dispositivos, sandbox por tool, SSRF policy | Três modelos sérios; OpenClaw tem threat model público mais formal; Garra foca em credenciais + injection; Hermes em redação/approvals |
| **Observabilidade** | ✅ OpenTelemetry OTLP, Prometheus, tracing por request, turn_stats/events, redact de logs | 🟡 logs, transcripts JSONL, FTS5, métricas MCP; sem OTel/dashboards confirmados | 🟡 eventos WS estruturados, doctor/audit CLIs; telemetria dedicada não confirmada | Garra é o único com stack OTel/Prometheus de primeira classe |
| **Persistência** | ✅ SQLite (sessions/messages/memory/projects/recurrence) + vectors + Postgres opcional (feature flag) + S3/MinIO | ✅ state.db (SQLite+FTS5), transcripts JSONL, sessões de gateway sobrevivem a reboot | ✅ SQLite por agente + arquivos de config/memória + wizard de migração | Garra tem o caminho de escala (Postgres) que os outros não documentam |
| **CI/CD & qualidade** | ✅ matrizes 3 OS, CodeQL, cargo-audit, deny, clippy -D warnings, cobertura c/ baseline, mutation testing, cross-compile | ❓ docs de contribuição; processos internos não públicos na análise | ❓ repo ativo; processos de CI não auditados | Garra auditável por nós; os outros não puderam ser verificados nesta análise |
| **Benches** | ✅ harness próprio com números medidos comitados | ❓ trajectory export p/ RL (Atropos), sem benches públicos | ❌ não encontrados | Único com evidência de desempenho comitada |

[^node-whatsapp]: Os 12 canais rodam no binário único, **com uma exceção planejada**: o
    WhatsApp **pessoal** (dispositivo vinculado, por QR code) vai exigir um bridge
    Node.js na máquina, porque não existe implementação madura desse protocolo em
    Rust — Hermes e OpenClaw usam Baileys pelo mesmo motivo. O WhatsApp Business
    (Meta Cloud API), que é o que está implementado hoje, continua 100% dentro do
    binário. Ver [ADR 0023](adr/0023-whatsapp-dispositivo-vinculado.md).

---

## Análise detalhada por sistema

### GarraIA (GarraRUST)

**Arquitetura.** Workspace Rust de 24 crates com separação limpa: `garraia-cli` (binário `garra`), `garraia-gateway` (Axum 0.8: REST v1, WS, admin, mobile, A2A, shims OpenAI/Anthropic), `garraia-agents` (runtime LLM, providers, modos, multi-agent, memória extratora), e crates de domínio (`-db`, `-storage`, `-security`, `-skills`, `-learning`, `-hardware`, `-channels`, `-telemetry`, `-media`, `-voice`, `-plugins` WASM via wasmtime). Fluxo de turno Receive→Execute→Respond com streaming.

**Diferenciais principais.**
1. **Local-first radical**: inferência local (Ollama/GGUF) com auto-seleção de quantização por VRAM detectada (`quantization.rs`, PR #1212), embeddings locais (ADR 0018), voz local.
2. **Hardware/IoT nativo** (ADR 0020): adapters GPIO/MQTT/Home Assistant/serial com **avaliação de risco por ação** (`risk.rs`), gate de aprovação e engine de automações (cron/expressões).
3. **Auto-aprendizado de skills** (ADR 0010): miner→generator→evaluator→safety→versioning, com gate de segurança e override.
4. **Multi-tenant real**: Group Workspace com memória separada por chat/grupo.
5. **12 canais num binário único** + A2A + MCP bidirecional + shims OpenAI/Anthropic (funciona como proxy para Claude Code).
6. **Observabilidade de primeira classe**: OTel OTLP + Prometheus + traces por request.
7. **Camada de escala**: SQLite→Postgres (feature flag, migrador com 6 stages idempotentes), object storage S3/MinIO com multipart.

**Pontos fortes técnicos.** Type-safety de ponta a ponta; testes (500+ no binário, 1105 no gateway, mutation testing com baseline); CI com CodeQL/cargo-audit/deny; docs mdBook + 22 ADRs + benches comitados; LGPD/GDPR com endpoint de anonimização.

**Limitações e partes incompletas.**
- `auto_router` é **heurístico por keywords**, não modelo (classificação barata de verdade depende de config de modelo separado).
- Desktop Control Center em M1 de 7 milestones; mobile em fase inicial.
- Postgres só via feature flag; S3 exige configuração manual.
- Alguns componentes parcialmente stubados (TTS Kokoro/ElevenLabs, tauri-plugin-updater ligado mas sem latest.json, DMG sem notarização, sem assinatura de código).
- Zero issues abertas — pipeline de descoberta de trabalho depende de auditorias manuais (como esta).
- Toolset nativo (13) menor que Hermes/OpenClaw; sem marketplace de skills.

### Hermes Agent

**Arquitetura.** Runtime Python único (loop de tool calling) alimentando múltiplas superfícies: CLI, TUI Ink, desktop Electron, dashboard web, servidor ACP para IDEs, gateway de mensagens de 19+ plataformas. Estado em `~/.hermes` com perfis isolados (`profiles/<name>/`).

**Diferenciais principais.**
1. **Closed learning loop de skills**: o agente cria SKILL.md da própria experiência; curator gerencia ciclo de vida (telemetry de uso `.usage.json`, archive/restore/backup, sweep determinístico a custo zero de tokens).
2. **Memória curada com teto rígido** (MEMORY.md 2.200 chars + USER.md 1.375 chars) injetada como snapshot congelado — preserva prompt caching por design; invariantes rígidos de alternância de papéis.
3. **execute_code (Programmatic Tool Calling)**: colapsa pipelines multi-step numa única inferência com kernel Python persistente.
4. **7 backends de terminal** incluindo serverless hibernante (Daytona/Modal) — custo quase zero parado.
5. **Cron durável muito flexível** (frases naturais, ISO, 5 campos, delivery multiplataforma, scripts no_agent).
6. **ACP nativo** — integração de primeira classe com IDEs (VS Code/Zed/JetBrains).

**Pontos fortes técnicos.** Eficiência de custo por design (cache de prefixo, modelos auxiliares baratos, tools diferidas); segurança de segredos (redact não-desligável pelo LLM); sessões de gateway sobrevivem a reboot; delegate com contexto isolado e worktrees.

**Limitações.** delegate_task **não é durável** (filho morre com o pai); MCP sem hot-reload; memória sem auto-compaction (overflow = erro manual); snapshot congelado torna escritas de memória invisíveis na sessão corrente; sessões gateway longas ficam caras; browser mode executa Python gerado pelo modelo na máquina local; Windows com quirks documentados; sem observabilidade OTel/Prometheus; sem hardware/IoT nativo.

### OpenClaw

**Arquitetura.** Gateway-daemon Node único por host (127.0.0.1:18789) com API WebSocket tipada (TypeBox→JSON Schema→codegen Swift), apps nativos (macOS Swift, iOS/Android Kotlin) como "Nodes" pareados, Control UI web. Agentes isolados por `agentDir` com session store SQLite próprio e bindings canal→agente.

**Diferenciais principais.**
1. **Unificação de canais pessoais**: WhatsApp (Baileys), iMessage, Telegram, Signal etc. num gateway com trust boundary explícita (pairing de DMs, allowlist de grupos).
2. **Segurança como cidadão de primeira classe**: `security audit` com auto-fix, threat model público mapeado ao **MITRE ATLAS**, sandboxing por tool (Docker/Podman/SSH/OpenShell/Crabbox), política pré-model-call, SSRF policy.
3. **Memória como arquivos humanos** + **Dreaming** (job cron que promove memória curto→longo com gates) + Memory Wiki.
4. **Code Mode + Tool Search**: escalar catálogos grandes de tools sem estourar contexto (compor tools num programa JS/TS compacto).
5. **Heartbeat/standing orders**: paradigma de agente sempre-ativo com turno periódico system-owned.
6. **ClawHub marketplace** com threat model próprio; Skill Workshop (agente redige, operador aprova).
7. **Nodes móveis**: câmera/tela/localização do celular pareado via WS.

**Pontos fortes técnicos.** Protocolo tipado com codegen multiplataforma; ecossistema de plugins (`api.registerTool`); dezenas de providers com ClawRouter; MCP bidirecional com OAuth/mTLS.

**Limitações.** Modelo de confiança de **um operador** — não multi-tenant adversarial; **workspace não é sandbox** (paths absolutos escapam para o host); histórico real de ~21,6k gateways expostos e skills maliciosas (jan/2026) motivou a postura atual; licença não-OSI (`Other`); rebrand triplo deixou docs desatualizadas; OAuth exige TTY (cron/Telegram não reautenticam interativamente); bridge MCP sem replay de eventos; observabilidade sem stack dedicada confirmada; LLM tipicamente em nuvem.

---

## Comparações arquiteturais

### 1. Arquitetura geral

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Runtime | Rust binário único, 24 crates | Python single-process + superfícies | Node gateway daemon único |
| Comunicação interna | tokio channels / axum | tool loop + subprocessos | WS tipado + JSON Schema |
| Extensão | crates + WASM plugins + skills + MCP | skills + MCP + plugins (desktop) | plugins + skills + MCP + A2UI |
| Isolamento | por crate/wasmtime; multi-tenant por chat | por perfil e subprocesso | por agentDir/agente |
| Escala vertical | Postgres/S3 feature flags | state.db SQLite | SQLite por agente |

### 2. LLM/providers

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Trait de abstração | `LlmProvider` unificado (chat+stream) | provider-agnostic config | provider registry + ClawRouter |
| OpenAI-compat | ✅ (OpenRouter/vLLM/LM Studio) | ✅ + proxy local OAuth | ✅ (Vercel/Cloudflare/LiteLLM gateways) |
| Local (Ollama/llama.cpp/vLLM) | ✅ primário no fallback | ✅ via OpenAI-compat | ✅ 6+ runtimes locais |
| Seleção dinâmica | ✅ por config routing + micro-router heurístico 🟡 | ✅ `hermes model` em runtime | ✅ por agente/provider |
| Fallback/resiliência | ✅ retry + failover primário→backup | ✅ pools rotativos + fallback | ✅ failover por agente |
| Auto-quantização local | ✅ **exclusivo** (por VRAM) | ❌ | ❌ |
| Multimodalidade | 🟡 vision/media/voice presentes; confirmar cobertura por provider | ✅ vision/browser/TTS/image (docs) | ✅ transcrição/imagem/música/vídeo (providers dedicados) |
| Tool calling | ✅ nativo + MCP bridge | ✅ + programmatic tool calling | ✅ + Code Mode |

### 3. Agentes / multi-agent

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Sub-agentes | ✅ AgentCoordinator + tokio | ✅ delegate_task (não-durável) | ✅ subagents + background ledger |
| Paralelo | ✅ parallel_execute | ✅ batch tasks | ✅ specialist lanes |
| Pipeline A→B | ✅ pipeline_execute | 🟡 encadeamento via context_from (cron) | 🟡 Task Flow |
| Duração/robustez | ✅ cancel tokens | 🟡 morre com o pai | ✅ ledger auditável |
| Equipes/workflow | ✅ team/MaxPower (TDD, worktrees) | ✅ Kanban SQLite durável | ✅ por-agente com bindings |
| A2A | ✅ cliente + servidor | ❓ não confirmado | 🟡 via ACP (harnesses) |

### 4. Tools

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Nativas | 13 focadas em dev/sistema | ~15+ focadas em produtividade/browser | ~25 em 8 categorias |
| Custom | ✅ crate garraia-tools | ✅ skills scripts + MCP | ✅ api.registerTool |
| Permissões | ✅ ToolPolicy por modo + approval gates | ✅ allowlist + approvals smart | ✅ allow/deny por tool/agente pré-model-call |
| Sandbox | 🟡 budget+gates; WASM p/ plugins | 🟡 processos; execute_code sem sandbox formal | ✅ Docker/Podman/SSH/OpenShell/Crabbox |
| Aprovação humana | ✅ approval.rs | ✅ smart (LLM auxiliar)/manual | ✅ approvals retomáveis (Lobster) |

### 5. MCP

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Cliente | ✅ stdio + tool_bridge c/ injection guard | ✅ stdio/HTTP + sampling + métricas | ✅ stdio/SSE/HTTP + OAuth/mTLS/toolFilter |
| Servidor | ✅ `garra mcp-server` (garra_ask) | ❌ não confirmado | ✅ expõe conversas dos canais |
| Hot-reload | ❓ não confirmado | ❌ (restart) | ❓ não confirmado |
| Autenticação | 🟡 (config) | ✅ env filtering + strip | ✅ OAuth login + mTLS |
| Discovery/marketplace | ✅ mcp_marketplace no gateway | ✅ catálogo no dashboard | ✅ Control UI + mcporter |

### 6. Memória

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Persistente | ✅ SQLite | ✅ MEMORY.md/USER.md | ✅ Markdown plano + SQLite |
| Semântica/vetorial | ✅ sqlite-vec + híbrida | ✅ FTS5 (full-text) + providers externos | ✅ memory_search (vetorial+keyword, opcional) |
| Extração automática | ✅ por turno c/ knobs (auto_extract, max_facts) | 🟡 nudges manuais + background review (modelo aux.) | ✅ memory flush + Dreaming |
| Teto/curadoria | ✅ max_facts + retention worker | ✅ teto rígido (curada à mão pelo agente) | ✅ gates do Dreaming + memory forget |
| Multi-tenant | ✅ por chat/grupo | 🟡 por perfil | 🟡 por agente |
| Entre sessões | ✅ contínua | ✅ (mas snapshot congelado na sessão) | ✅ |

### 7. Skills/plugins

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Formato | skills internas + geradas | SKILL.md (agentskills.io) | SKILL.md + plugin SDK |
| Auto-geração | ✅ garraia-learning (com safety gate) | ✅ closed loop + curator | ✅ Skill Workshop (c/ aprovação do operador) |
| Marketplace | ❌ | ✅ Skills Hub comunitário | ✅ ClawHub (c/ threat model) |
| Dependências/gating | ✅ evaluator+safety | ✅ (curator archive) | ✅ requires.bins verificado no host |
| Isolamento | ✅ WASM p/ plugins | 🟡 | ✅ sandbox recomendado p/ terceiros |

### 8. Automação

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Cron/jobs | ✅ schedule tool + recurrence 2-tier | ✅ cron durável flexível | ✅ Automations scheduler |
| Eventos/triggers | ✅ automations de hardware (cron/expr) | ✅ webhooks | ✅ hooks de ciclo de vida + IMAP + webhooks |
| Sempre-ativo | 🟡 workers do gateway | 🟡 cron | ✅ Heartbeat (30 min default) |
| Longa duração | ✅ gateway daemon + workers | ✅ sessões gateway sobrevivem a reboot | ✅ daemon systemd/launchd |
| Sem inferência | ✅ automations de hardware | ✅ cron no_agent scripts | ✅ hooks |

### 9. Interfaces

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| CLI/TUI | ✅ REPL + `garra ask --json` | ✅ CLI + TUI Ink (widgets dockáveis) | ✅ CLI completa |
| Web | ✅ webchat + admin + learning | ✅ dashboard c/ chat embutido | ✅ Control UI |
| Desktop | 🟡 Tauri v2 (M1/M2) | ✅ Electron nativo | ✅ macOS nativo (Swift) |
| Mobile | 🟡 Flutter + Termux | 🟡 Termux | ✅ iOS/Android (Nodes) |
| API | ✅ REST v1 + WS + shims OpenAI/Anthropic + A2A | ✅ ACP p/ IDEs | ✅ WS + ACP |
| Slash commands | ✅ gateway | ✅ (/new, /cron, /curator…) | ✅ (derivados de skills) |

### 10. Segurança

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Credenciais | ✅ vault AES-256-GCM → argon2 | ✅ vault + auth.json + redact | ✅ mapa de credenciais + secret scanning |
| Prompt injection | 🟡 guard ~20 padrões em web_fetch, tool MCP, file_read e device_read/device_list (#1213, #1243 fatias 1-3) | 🟡 env filtering, wrapping não confirmado | ✅ wrapping de conteúdo não-confiável |
| Shell control | ✅ budget + gates | ✅ allowlist + approvals + YOLO explícito | ✅ policy pré-model-call + elevated duplamente gated |
| Sandbox | 🟡 WASM para plugins + sandbox por tool `agent.sandbox` (backends docker/podman/ssh, default off; hoje envolve só `bash`, ssh é execução remota) — ADR 0019 | 🟡 | ✅ 5 backends |
| Auditoria/threat model | ✅ audit-log, RLS testada, LGPD/GDPR | 🟡 logs + métricas MCP | ✅ MITRE ATLAS público + security audit auto-fix |
| Rate limiting | ✅ (incl. SSE per-user) | ❓ | ✅ |

### 11. Execução local

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| 100% offline viável | ✅ (Ollama+embeddings+voz locais) | 🟡 (comandos sim; LLM precisa endpoint local) | 🟡 (gateway sim; LLM via nuvem por padrão) |
| GPU / VRAM | ✅ auto-quantização | ❓ | ❓ |
| Docker | ✅ compose (ollama/postgres/minio) | ✅ backend + host | ✅ sandbox + imagem oficial |
| Termux/Android | ✅ (ADR 0016) | ✅ | 🟡 via Node |
| Instalação | 6+ binários multi-OS (aarch64 incl.) | curl script | npm/Docker/Nix |

### 12. Integrações

| Serviço | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| GitHub | ✅ (gh CLI, workflows) | ✅ | ✅ |
| Gmail | 🟡 (via canais/MCP) | ✅ (canal email) | ✅ (trigger IMAP) |
| Telegram/Discord/Slack | ✅ nativos | ✅ nativos | ✅ nativos |
| WhatsApp Business (Cloud API) | ✅ nativo (webhook + HMAC) | ✅ (`hermes whatsapp-cloud`) | ❓ |
| WhatsApp pessoal (dispositivo vinculado, QR) | ✅ (v0.4.3, `garra whatsapp`, bridge Node/Baileys por stdio — [ADR 0023](adr/0023-whatsapp-dispositivo-vinculado.md)) | ✅ (Baileys) | ✅ (Baileys) |
| n8n | 🟡 via gateway/webhook | 🟡 via webhook/cron | 🟡 via webhook |
| Home Assistant | ✅ adapter nativo | ✅ canal | ❌ nativo não encontrado |
| Browser | ✅ web_fetch/search | ✅ 8 modos de browser | ✅ controle real |
| Bancos de dados | ✅ SQLite/Postgres/S3 | ✅ SQLite | ✅ SQLite |
| iMessage | ✅ | ✅ (BlueBubbles) | ✅ |
| IoT genérico | ✅ MQTT/GPIO/serial | ❓ via MCP/terminal | ❓ via plugins |

### 13. Hardware/IoT

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Abstração de dispositivo | ✅ trait Device + capability + registry | ❌ | ❌ (só Nodes: câmera/tela/localização) |
| MQTT / GPIO / Serial | ✅ adapters | ❌ | ❌ |
| Home Assistant | ✅ adapter | ✅ canal | ❌ |
| Robótica | 🟡 (garra física do projeto, skills de hardware) | 🟡 via skills comunitárias | ❌ |
| Risco por ação de hardware | ✅ risk.rs + gate | ❌ | ❌ |

### 14. Desenvolvimento / extensibilidade

| Aspecto | GarraIA | Hermes | OpenClaw |
|---|---|---|---|
| Adicionar provider | ✅ implementar `LlmProvider` (~1 arquivo) | ✅ config + compat | ✅ registry |
| Adicionar tool | ✅ trait + registro | ✅ skill/MCP/plugin | ✅ api.registerTool |
| Adicionar canal | ✅ trait unificado + registry | ✅ plugin | ✅ plugin de canal |
| Adicionar agente | ✅ AgentMode + policy | ✅ perfil/bot | ✅ agents add + bindings |
| Testes/CI | ✅ matrizes, mutation, cobertura baseline | ❓ | ❓ |
| Docs/ADRs | ✅ mdBook + 22 ADRs | ✅ llms.txt + guides | ✅ extensas (com dígitos do rebrand) |

---

## Recursos e diferenciais exclusivos

### Exclusivos do GarraIA (verificados: sem equivalente funcional nos outros)
1. **Auto-quantização local por VRAM** (Q4_K_M/Q5_K_M/Q8_0) — função pura testável sem GPU (PR #1212).
2. **Camada de hardware nativa com risco por ação** (GPIO/MQTT/HA/serial + gate + automations engine, ADR 0020).
3. **Persona quente pt-BR como produto** (ADR 0012) com modo neutro.
4. **Multi-tenant familiar** (Group Workspace c/ memória separada) num host único.
5. **Shims OpenAI + Anthropic + A2A + MCP-server no mesmo gateway** — vira proxy universal p/ Claude Code.
6. **Stack OTel/Prometheus nativa** + benches comitados de comparação de frameworks.
7. **Recorrência two-tier de tarefas** (ADR 0013) — exemplos: "3ª segunda do mês", janelas com fuso.

### Exclusivos do Hermes
1. **execute_code / Programmatic Tool Calling** com kernel persistente (colapso de pipeline numa inferência).
2. **Curator de skills** com telemetry de uso e arquivo/backup automático.
3. **Memória de teto rígido curada** preservando prompt caching (invariantes documentados).
4. **7 backends de terminal** com serverless hibernante (Daytona/Modal).
5. **ACP de primeira classe** para IDEs.
6. **Kanban durável multi-perfil** com dispatcher no gateway.
7. **Trajectory export para RL** (integração Atropos/Nous).

### Exclusivos do OpenClaw
1. **Nodes móveis pareados** (iOS/Android/macOS: câmera, tela, localização via WS).
2. **Dreaming + Memory Wiki** (promoção automática de memória c/ gates; wiki compilada).
3. **Code Mode + Tool Search** (escalar catálogo de tools sem inflar contexto).
4. **Heartbeat/standing orders** (agente sempre-ativo system-owned).
5. **ClawHub** marketplace c/ threat model + **MITRE ATLAS** público.
6. **Sandboxing de 5 backends** por tool/agente (Docker/Podman/SSH/OpenShell/Crabbox).
7. **Skill Workshop** (agente redige, operador aprova — separação de autoridade).
8. **Protocolo WS tipado com codegen** para apps nativos (Swift/Kotlin).

---

## Gap Analysis do GarraIA

| Área | Estado atual do GarraIA | Hermes | OpenClaw | Gap | Complexidade estimada | Prioridade |
|---|---|---|---|---|---|---|
| Sandbox de execução de tools | budget + gates; shell direto | processos; sem sandbox formal | Docker/Podman/SSH/OpenShell/Crabbox por tool | **Falta sandbox por tool** — maior gap de segurança | Alta | **P0** |
| Multi-agent durável | cancel tokens; team in-process | delegate não-durável; kanban SQLite | ledger de background tasks auditável | Falta **ledger durável** de runs background | Média | P1 |
| Roteador inteligente | heurístico por keywords 🟡 | modelos auxiliares por função | ClawRouter | Substituir keyword-matching por **modelo barato p/ classify** (já existe config p/ summarize) | Baixa | P1 |
| Desktop/mobile | Tauri M1/M2; Flutter inicial | Electron maduro; ACP | apps nativos completos (Nodes) | Falta app desktop maduro + **nodes móveis** | Alta | P2 |
| Marketplace/ecossistema | sem marketplace de skills | Skills Hub | ClawHub c/ threat model | Falta **distribuição** de skills/plugins | Média (infra) | P2 |
| Hooks de ciclo de vida | parciais (workers) | shell hooks c/ allowlist | hooks ricos (/new, /reset, compaction) + api.on | Falta **event bus** interno p/ plugins/hooks | Média | P1 |
| Compaction de sessão | context_summarizer (modelo barato) | automática + invariantes | memory flush pré-compaction | Garantir **flush de memória pré-compaction** e política automática | Baixa | P1 |
| MCP autenticação | config simples | env filtering + métricas | OAuth/mTLS/toolFilter | Adicionar **env filtering + toolFilter** ao cliente MCP | Baixa | P2 |
| Observabilidade do usuário | OTel/Prometheus p/ operador | doctor + FTS5 | doctor/audit CLIs | Falta **dashboard unificado** de uso/custo por turno | Média | P3 |
| Dreaming (promoção de memória) | retention worker (podar) | background review | Dreaming c/ gates | Adicionar **promoção curto→longo prazo** com gates | Média | P2 |
| Programmatic tool calling | ❌ (loop tool-por-tool) | ✅ execute_code | 🟡 Code Mode | **execute_code interno** (kernel persistente) reduziria custo/latência de pipelines | Média | P1 |
| Notarização/assinatura | pendente (Apple secrets) | N/A | N/A | Distribuição macOS/Windows confiável | Baixa (depende do mantenedor) | P2 |
| Multi-provider media gen | voice/media nativos | image gen via portal | transcrição+imagem+música+vídeo providers | Ampliar catálogo de **providers de mídia** | Baixa | P3 |
| OAuth p/ canais (sem TTY) | tokens/vault próprios | quirks no reauth | falha documentada p/ cron | Manter vault local é vantagem; documentar rotação sem TTY | Baixa | P3 |

**Síntese dos gaps:** GarraIA perde para OpenClaw em **sandbox** e **ecossistema de distribuição**; perde para Hermes em **programmatic tool calling** e **ciclo de vida de skills**; e as áreas em que os dois concorrentes são fracos (hardware, multi-tenant, local-first com quantização, observabilidade) são exatamente onde GarraIA já é líder — consolidá-las importa mais que copiar features.

---

## Oportunidades para o GarraIA

### Curto prazo (alto impacto, mudanças pequenas/médias)

1. **Classificador de roteamento por modelo barato** — Problema: `auto_router` heurístico erra em goals ambíguos. Proposta: usar o modelo `classify` já configurado no gateway (mesmo caminho do `context_summarizer`) com cache e timeout curto. Benefício: roteamento correto de modo (search/code/debug). Dificuldade: baixa. Dependências: config de modelo auxiliar (existe). Impacto: qualidade de rota sem custo perceptível.
2. **Flush de memória pré-compaction** — Proposta: antes de `context_summarizer` agir, gravar fatos/filas pendentes no memory store (equivalente ao "memory flush" do OpenClaw). Dificuldade: baixa. Impacto: nenhuma memória se perde em sessões longas.
3. **Env filtering + toolFilter no cliente MCP** — Proposta: espelhar o allowlist de env do Hermes e filtros de tool do OpenClaw no `mcp/manager.rs`. Dificuldade: baixa. Impacto: reduz superfície de exfiltração via MCP.
4. **Completar sandbox da ADR 0019 (process hardening)** — já tem ADR; falta implementação mínima: `rlimit` + cwd jail + allowlist de comandos por ToolPolicy.

### Médio prazo (mudanças arquiteturais moderadas)

1. **Sandbox por tool (estilo OpenClaw)** — Proposta: backend Docker/Podman opcional por tool, com `tools.elevated` duplamente gated. Benefício: permite rodar skills de terceiros com segurança; pré-requisito para marketplace próprio. Dificuldade: média-alta. Dependências: containers no host; já há docker-compose no projeto.
2. **Ledger durável de runs em background** — Proposta: persistir AgentHandle runs em SQLite (tabela `agent_runs`) com status/audit e retomada após restart — supera a limitação do Hermes (delegate morre com o pai) e se alinha ao OpenClaw. Dificuldade: média. Impacto: automações longas confiáveis.
3. **Programmatic tool calling interno** — Proposta: tool `execute_code` com kernel Python persistente (estilo Hermes) — pipelines de N tools viram 1 inferência. Benefício: custo/latência; diferencia em agente dev. Dificuldade: média. Dependências: sandbox do item 1.
4. **Event bus interno + hooks** — Proposta: eventos de ciclo de vida (turn_start, compaction, tool_call, hardware_action) consumíveis por plugins WASM/skills. Benefício: desbloqueia ecossistema estilo OpenClaw `api.on`. Dificuldade: média.

### Longo prazo (grandes diferenciais)

1. **Marketplace de skills/plugins do Garra** com threat model (assinar skills geradas pelo `garraia-learning`, publicar com revisão + auditoria automática) — nenhum concorrente tem skills **geradas por agente com safety gate versionado**; essa é a tese única do Garra para ecossistema.
2. **Nodes móveis Garra** — app Flutter atual → pareamento WS estilo OpenClaw (câmera/localização) + Termux já suportado; combina com a camada de hardware (o Garra controla robô E o celular do dono).
3. **Desktop Control Center completo (M2–M7)** — ADR 0021; único agente que unificaria desktop control + hardware IoT + chat num app Rust.
4. **Local-first total como produto** — instalar e rodar 100% offline (LLM local + voz + embeddings) com um comando; nem OpenClaw nem Hermes entregam isso por padrão hoje.

---

## Casos de uso

| Cenário | Melhor encaixe | Trade-offs técnicos |
|---|---|---|
| Agente pessoal local | **GarraIA** (offline total possível) vs Hermes | Garra exige Rust/infra; Hermes mais fácil de instalar mas LLM costuma ser cloud |
| Coding agent | **Hermes** (ACP/IDE, worktrees, delegation) ≈ Garra MaxPower | OpenClaw forte via ACP harnesses mas não é foco dev-first; Garra tem TDD gates mas toolset dev menor |
| Servidor autônomo (24/7) | **OpenClaw** (heartbeat, ledger) ≈ Hermes (cron durável) | Garra tem gateway daemon + workers mas sem ledger auditável de runs |
| Automação residencial | **GarraIA** — adapters MQTT/HA nativos | Hermes via canal HA; OpenClaw não tem |
| Robótica | **GarraIA** — hardware crate c/ risco por ação | Hermes/OpenClaw só via extensões |
| Edge/RPi low-power | **GarraIA** — binário Rust único, quantização por RAM/VRAM | Node 24 (OpenClaw) e Python+deps (Hermes) pesam mais em memória |
| Workstation c/ GPU | **GarraIA** — auto-quantização por VRAM + Ollama | Hermes/OpenClaw suportam Ollama mas sem gestão de VRAM |
| Servidor multi-GPU | **GarraIA** (vLLM via OpenAI-compat) 🟡 | nenhum tem scheduler multi-GPU nativo |
| Empresa | **OpenClaw** ⚠️ (trust boundary single-operator; licença a verificar) | Hermes (perfis); Garra multi-tenant familiar ≠ corporate SSO |
| Múltiplos agentes coordenados | **Hermes** (kanban) ≈ OpenClaw (bindings) | Garra team/MaxPower in-process |
| Integração com serviços externos | **OpenClaw** (webhooks/IMAP/hooks) ≈ Hermes | Garra via gateway REST/webhooks |
| Execução offline | **GarraIA** | únicos concorrentes viáveis exigem LLM cloud |

---

## Conclusão

**Convergências.** Os três convergem num padrão claro de 2026: runtime de agente com gateway de canais de mensagem, MCP bidirecional, skills como memória procedural, cron/automação persistente, providers provider-agnostic e memória entre sessões. As diferenças são de **ênfase e profundidade**, não de categoria.

**Divergências arquiteturais.** GarraIA aposta em Rust single-binary local-first com verticalização em hardware e escala de dados (Postgres/S3). Hermes aposta em eficiência de custo e aprendizado (programmatic tool calling, memória curada, curator). OpenClaw aposta em unificação de canais pessoais e segurança de ecossistema (sandbox, threat model, marketplace).

**Posição do GarraIA.** Nas áreas que os três competem diretamente (providers, MCP, canais, cron, skills), GarraIA está aproximadamente equivalente — com a ressalva de que seu roteador é heurístico e seu toolset é menor. Nos diferenciais (hardware/IoT com risco por ação, quantização automática, multi-tenant familiar, observabilidade OTel), GarraIA não tem equivalente nos outros dois.

**Gaps que mais importam.** (1) sandbox por tool (P0, segurança — pré-requisito de ecossistema); (2) programmatic tool calling (P1, custo); (3) ledger durável de runs (P1, confiabilidade); (4) roteamento por modelo barato (P1, qualidade).

**Maior alavanca de evolução.** O par **auto-aprendizado de skills + marketplace próprio com safety gate** é a tese que nenhum concorrente tem: o GarraIA já gera, avalia e versiona skills — falta o sandbox e a distribuição para transformar isso em ecossistema. Combinado com a camada de hardware (que ninguém mais tem), o posicionamento técnico do GarraIA é "o agente pessoal local que controla o mundo físico e aprende sozinho com segurança" — enquanto Hermes compete em produtividade/custo e OpenClaw em ubiquidade de canais.

> Para este tipo de workload (automação residencial/robótica/offline), o GarraIA possui vantagem arquitetural devido à camada de hardware nativa e ao binário Rust quantizável; para coding assistido por IDE, o Hermes oferece maior flexibilidade via ACP e delegation; para presença ubiqua nos canais de chat pessoais do usuário, o OpenClaw tem o gateway mais completo — embora com trust boundary de operador único e licença a verificar.
