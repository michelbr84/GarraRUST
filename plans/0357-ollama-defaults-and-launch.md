# Plan 0357 — Padrão `qwen3.8`, `garraia --model <tag>` e preparo para `ollama launch`

**Origem:** pedido direto do operador (2026-08-29), sem issue no tracker.
**Branch:** `claude/garraia-ollama-defaults-11te9j`
**Status:** ✅ Entregue 2026-08-29

## Problema

Três lacunas, todas verificadas empiricamente antes do plano:

1. **Não havia modelo padrão coerente.** O fallback de código era `llama3.1`,
   repetido inline em 11 pontos de `crates/garraia-cli/src/chat.rs` — a tabela
   `hardcoded_default_model` existia mas estava duplicada 3×. O wizard baixava
   `hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M`. Docs e exemplos citavam
   `llama3.1`.
2. **`garraia --model qwen3.8` não rodava.** `main()` injetava o subcomando
   `chat` só quando `args.len() == 1`; com qualquer flag presente o clap
   falhava com *unexpected argument*. Pior: `--model` sem `--provider` só
   trocava a string do banner — o `Arc<dyn LlmProvider>` seguia sendo o do
   autodetect, com o modelo interno obsoleto (idem em `garraia ask`).
3. **`ollama launch garraia` não existe** e não pode ser resolvido só aqui.

## Fatos externos confirmados

| Fato | Evidência |
|---|---|
| `qwen3.8:latest` = `qwen3.8:27b`, ~18 GB Q4_K_M, 262 144 tokens de contexto | `ollama.com/library/qwen3.8/tags` |
| `ollama launch` existe desde a v0.15.0 (`--model`, `--config`, `--restore`, `-y`) | `cmd/launch/launch.go:281,400-403` |
| O registro de integrações é uma slice Go compilada no binário — sem manifesto, plugin ou namespace | `cmd/launch/registry.go:38` |
| O Hermes instala via `curl … \| bash -s -- --skip-setup` e grava `provider: ollama-launch`, `base_url: …/v1`, `api_key: ollama` | `cmd/launch/hermes.go:29,33,268-311` |

## Decisões

### D1 — `--model` por pré-processamento de argv, não flag global do clap

`Commands::Chat` e `Commands::Ask` já declaram `model`; um `#[arg(global)]`
homônimo é propagado para todo subcomando e dispara a asserção de nomes únicos
do clap. E `-m` global colidiria com `MaxPower { mode: short='m' }`. A feature
inteira vive em `cli_args::inject_default_subcommand`, sem tocar nos derives.

A regra é **"injeta se nada ocupa a posição de subcomando"**, e não uma lista
de nomes: preserva o `did you mean 'chat'?` do clap, não tem drift com
`#[cfg(feature = "plugins")]`, e trata `garraia --model chat` corretamente. A
tabela de flags-que-consomem-valor é derivada de `Cli::command()`, com um teste
de drift que falha se alguém adicionar uma flag com valor a `chat`.

### D2 — A sonda do Ollama vence o `agent.default_provider`, mas só com acerto exato

`garraia --model qwen3.8` numa máquina configurada para OpenRouter tem de abrir
o qwen3.8 — senão a feature não faz o que promete. Fica **depois** do `--url`
(roteamento mais explícito) e **antes** do `default_provider`. Como só vence
com acerto exato em `GET /api/tags`, `--model gpt-4o` nunca é sequestrado.

Guarda extra (`model_belongs_to_configured_cloud_provider`): se o `config.yml`
já declara aquele nome sob um provider de nuvem, o caminho do Ollama é pulado
inteiro. Sem isso, `--model gpt-4o` ofereceria "baixar gpt-4o do Ollama" — um
download que não pode dar certo e uma pergunta que o usuário nunca deveria ver.

### D3 — Pull via `POST /api/pull`, não shell-out

No caminho `chat`/`ask` o binário `ollama` pode não estar no `PATH`, e quando
`OLLAMA_BASE_URL` aponta para outro host um shell-out baixaria na máquina
errada. No **wizard** é o inverso — ele acabou de instalar e subir um daemon
local, `ollama` está no `PATH` por construção, e a barra de progresso do CLI
upstream é melhor que qualquer reimplementação. Por isso os dois caminhos
divergem de propósito, cada um documentado no seu lugar.

### D4 — O wizard continua gravando `provider: openai` + `/v1`

É exatamente o contrato que o `ollama launch` escreve para o Hermes
(`cmd/launch/hermes.go:294-297`), então mantê-lo alinha o GarraIA com o
upstream. O caminho nativo `provider: ollama` do `--model` é um eixo separado.

### D5 — `QWEN3_MODEL_TAG` fica, como opção do seletor

O teste `constants_match_plan_0126` trava as 4 constantes do plan 0126.
Introduzir `DEFAULT_OLLAMA_MODEL_TAG` ao lado, com o tag antigo virando uma
linha do seletor, evita emendar aquele plano e ainda dá ao usuário a opção
mais leve (~9 GB contra ~18 GB).

## Entregue

**Novos:** `crates/garraia-cli/src/cli_args.rs`, `tests/install_sh/parse_args.sh`,
`docs/integrations/ollama-launch.md`, este plano.

**Modificados:** `crates/garraia-agents/src/{ollama.rs,lib.rs}` ·
`crates/garraia-cli/src/{main.rs,chat.rs,ask.rs,config_cmd.rs,mcp_server.rs}` ·
`crates/garraia-cli/src/wizard/{mod.rs,local_stack.rs,config_writer.rs}` ·
`install.sh` · `.github/workflows/ci.yml` · configs de exemplo e docs ·
`CHANGELOG.md`.

**Superfícies novas:**

- `normalize_ollama_tag`, `OllamaProvider::{resolve_installed_model, pull_model}`,
  `PullProgress` em `garraia-agents`.
- `garraia --model <tag>` / `garraia` (bare) → REPL; `-y/--yes` em `chat` e `ask`.
- `garraia config set-model --model <tag> [--provider-key|--provider|--base-url]`.
- `install.sh --skip-setup|--skip-init|--skip-start|--no-local|--version|--install-dir|--help`.

## Verificação

- `cargo +1.95 test -p garraia-agents --lib` — 149 verdes (24 novos).
- `cargo +1.95 test -p garraia --bin garra` — 201 verdes (26 novos).
- `cargo +1.95 clippy -p garraia -p garraia-agents --all-targets -- -D warnings` — limpo.
- `cargo +1.95 fmt --all -- --check` — limpo.
- `bash tests/install_sh/parse_args.sh` — 18/18; as 4 suítes pré-existentes seguem verdes.
- Ponta a ponta contra um daemon Ollama stub (`/api/tags`, `/api/chat`, `/api/pull`):
  `--model qwen3.8` normaliza para `:latest` e a requisição na rede carrega
  `qwen3.8:latest` (prova de que o bug do provider obsoleto morreu); a tag
  ausente imprime a dica sem prompt fora de TTY; `-y` baixa e usa;
  `--model openrouter/auto` não vira tag do Ollama.

## Parte upstream — escrita e validada, falta abrir o PR

`contrib/ollama-launch/` traz `garraia.go` (implementa `Runner` +
`ManagedSingleModel`, espelhando `cmd/launch/hermes.go`), `garraia_test.go`
(10 testes) e o patch do `registry.go`. **Escritos contra o `ollama/ollama`
real e validados lá dentro**: `gofmt -l cmd/` limpo, `go vet ./cmd/launch/`
limpo, `go build ./...` ok, e `go test ./cmd/launch/` — a suíte **inteira** —
verde.

Armadilha que só apareceu rodando a suíte de verdade: `TestListIntegrationInfos`
tem dois subtestes com exigências opostas. `follows_launcher_order` compara a
lista visível inteira contra `launcherIntegrationOrder` (toda integração
não-hidden precisa estar lá), enquanto `prioritizes_primary_launcher_integrations`
trava o **prefixo**. A entrada nova tem de ir no **fim**. A primeira tentativa
pôs `garraia` em quarto lugar e quebrou o segundo subteste — promover uma
integração no menu primário é decisão dos mantenedores do Ollama, não nossa.

**O que falta:** o passo que exige fork (`gh repo fork ollama/ollama`,
aplicar os três arquivos, abrir o PR). Não foi possível nesta sessão: o acesso
GitHub está escopado em `michelbr84/garrarust` e o `add_repo` recusa adds
cross-owner. Aceitar a integração é decisão dos mantenedores do Ollama.
Windows fica fora da primeira versão — o GarraIA não publica instalador
`.ps1` para web.

## Nota de segurança

O novo `POST /api/pull` usa o mesmo `base_url` que `/api/chat` e `/api/tags` já
usam neste crate com `reqwest::Client` puro. Mantive a postura existente por
consistência, mas é ponto explícito de revisão: se o veredito for aplicar o
`garraia_common::ssrf` (regra 14 do CLAUDE.md), ele deve cobrir os três
endpoints de uma vez, não só o novo.
