# Contribuindo com o GarraIA

Obrigado pelo interesse em contribuir! Este documento descreve como configurar o ambiente, as convenções do projeto e o processo de envio de contribuições.

---

## Código de Conduta

Ao participar deste projeto, você concorda em ser respeitoso, inclusivo e construtivo nas interações com outros membros da comunidade.

---

## Pré-requisitos

| Ferramenta | Versão mínima | Instalação |
|------------|---------------|------------|
| Rust | 1.88 | `rustup update stable` |
| Git | qualquer | [git-scm.com](https://git-scm.com) |
| FFmpeg | 6.x | `apt install ffmpeg` / `brew install ffmpeg` |
| Node.js (opcional) | 20+ | Para rodar servidores MCP de teste |

Para features de voz (`garraia-voice`): CUDA Toolkit 12+ (NVIDIA) ou Metal (Apple Silicon).

---

## Configurar o ambiente de desenvolvimento

### 1. Fazer fork e clonar

```bash
git clone https://github.com/SEU_USUARIO/GarraRUST.git
cd GarraRUST
git remote add upstream https://github.com/michelbr84/GarraRUST.git
```

### 2. Compilar o workspace

```bash
cargo build --workspace
```

### 3. Executar os testes

```bash
# Suite completa
cargo test --workspace

# Crate específica
cargo test -p garraia-gateway

# Com saída de log
cargo test --workspace -- --nocapture
```

### 4. Rodar o servidor localmente

```bash
cargo run --package garraia-cli -- init
cargo run --package garraia-cli -- start
```

### 5. Verificar saúde do servidor

```bash
curl http://127.0.0.1:3888/health
# {"status":"ok","version":"0.9.0-dev"}
```

### 6. Construir a documentação

```bash
# Instalar mdBook
cargo install mdbook

# Construir e servir localmente
mdbook serve docs
# Acesse http://localhost:3000
```

---

## Estrutura do projeto

```
crates/
├── garraia-cli/          # Ponto de entrada da CLI (main.rs)
├── garraia-gateway/      # Servidor HTTP/WebSocket (Axum 0.8)
├── garraia-config/       # Parsing e hot-reload de configuração
├── garraia-channels/     # Adaptadores de canais (Telegram, Discord, Slack...)
├── garraia-agents/       # Provedores LLM + AgentRuntime
├── garraia-voice/        # Pipeline de voz (STT + TTS)
├── garraia-tools/        # Trait Tool + registro de ferramentas
├── garraia-runtime/      # Máquina de estados do executor
├── garraia-db/           # SQLite + busca vetorial (rusqlite)
├── garraia-plugins/      # Sandbox de plugins WASM (Wasmtime)
├── garraia-media/        # Processamento de PDF e imagens
├── garraia-security/     # Vault de credenciais + autenticação
├── garraia-skills/       # Parser e instalador de skills Markdown
└── garraia-common/       # Tipos compartilhados + erros
apps/
└── garraia-mobile/       # Cliente Flutter Android (Riverpod + go_router)
```

---

## Fluxo de trabalho

### 1. Sincronizar com upstream

```bash
git fetch upstream
git checkout main
git merge upstream/main
```

### 2. Criar um branch

```bash
git checkout -b feat/nome-da-feature  # nova feature
git checkout -b fix/descricao-do-bug  # correção de bug
git checkout -b docs/pagina-atualizar  # documentação
```

### 3. Verificar antes de enviar

Execute todos os checks antes de abrir o PR:

```bash
cargo fmt --all                           # formatar
cargo clippy --workspace -- -D warnings   # linting
cargo test --workspace                    # testes
cargo deny check                          # dependências
```

### 4. Fazer commits

Use o formato [Conventional Commits](https://www.conventionalcommits.org/):

```bash
git commit -m "feat(channels): adiciona suporte ao canal WhatsApp Business"
git commit -m "fix(agents): corrige vazamento de memória no streaming SSE"
git commit -m "docs(api): documenta endpoint POST /api/sessions"
git commit -m "test(gateway): adiciona testes de integração para auth JWT"
git commit -m "refactor(db): simplifica SessionStore com query builder"
git commit -m "chore(deps): atualiza axum para 0.8.4"
```

**Tipos aceitos:** `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, `perf`, `ci`

Limite de 72 caracteres no assunto. Use o imperativo: "adiciona" (não "adicionada").

### 5. Abrir o Pull Request

```bash
git push origin feat/nome-da-feature
```

Abra o PR no GitHub apontando para `main`. Preencha o template completamente.

---

## Convenções de código

### Rust

- **Sem `unwrap()` em código de produção** — use `?` ou `expect()` com mensagem descritiva apenas em testes
- **Sem concatenação de strings em SQL** — use sempre a macro `params!`
- **Sem secrets em logs** — nunca registre `ANTHROPIC_API_KEY`, `GARRAIA_JWT_SECRET` etc.
- `AppState` é `Arc<AppState>` — importe via `crate::state::AppState`
- Axum 0.8: `FromRequestParts` usa AFIT nativo — sem `#[async_trait]`
- Docstrings com `///` para funções públicas, `//!` para módulos
- Erros via `thiserror`; use `garraia_common::Error` para erros cross-crate

### Flutter

- State management: Riverpod com code generation (`@riverpod`)
- Navigation: go_router com redirect de autenticação
- HTTP: Dio com `_AuthInterceptor` para JWT bearer automático
- Nunca use `withOpacity()` — use `withValues(alpha:)`
- Docstrings `///` para classes e métodos públicos

### Documentação

- PT-BR para guias do usuário e documentação interna
- EN para comentários de código, README principal e PR descriptions
- Nenhum placeholder como `TODO`, `...` ou `<inserir aqui>`

---

## Prioridades atuais

| Prioridade | Issue | Descrição |
|------------|-------|-----------|
| **P0** | [#104](https://github.com/michelbr84/GarraRUST/issues/104) | Website: garraia.org com páginas de comparação |
| **P0** | [#105](https://github.com/michelbr84/GarraRUST/issues/105) | Comunidade Discord |
| **P1** | [#106](https://github.com/michelbr84/GarraRUST/issues/106) | Skills iniciais embutidas |
| **P1** | [#108](https://github.com/michelbr84/GarraRUST/issues/108) | Roteamento multi-agente |
| **P1** | [#80](https://github.com/michelbr84/GarraRUST/issues/80) | MCP: resources, prompts, HTTP transport |
| **P1** | [#74](https://github.com/michelbr84/GarraRUST/issues/74) | Hardening de segurança |
| **P2** | [#72](https://github.com/michelbr84/GarraRUST/issues/72) | Suite de testes e benchmarks |
| **P2** | [#73](https://github.com/michelbr84/GarraRUST/issues/73) | CI/CD: matrix builds, crates.io, Docker |

---

## Adicionando um novo provedor LLM

1. Crie `crates/garraia-agents/src/providers/meu_provedor.rs`
2. Implemente o trait `LlmProvider`
3. Registre em `providers/mod.rs`
4. Adicione à configuração de exemplo em `docs/src/providers.md`
5. Adicione testes de integração em `tests/providers/`

## Adicionando um novo canal

1. Crie `crates/garraia-channels/src/meu_canal.rs`
2. Implemente o trait `Channel`
3. Registre no bootstrap em `crates/garraia-gateway/src/bootstrap.rs`
4. Documente em `docs/src/channels.md` e crie `docs/src/guides/connect-meu-canal.md`

---

## Encontrando algo para trabalhar

- [Good first issues](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen)
- [Help wanted](https://github.com/michelbr84/GarraRUST/issues?q=label%3Ahelp-wanted+is%3Aopen)
- [Roadmap no Linear](https://linear.app/chatgpt25/project/garraia-complete-roadmap-2026-ac242025/overview)

Comente em uma issue antes de começar a trabalhar para evitar esforço duplicado.

---

## Comunicação

- **Discord:** [discord.gg/aEXGq5cS](https://discord.gg/aEXGq5cS)
- **GitHub Issues:** bugs, feature requests e discussões técnicas
- **GitHub Discussions:** perguntas gerais e ideias
- **Segurança:** `security@garraia.cloud` (veja [SECURITY.md](SECURITY.md))

---

## Licença

Ao contribuir com este projeto, você concorda que suas contribuições serão licenciadas sob a [Licença MIT](LICENSE).
