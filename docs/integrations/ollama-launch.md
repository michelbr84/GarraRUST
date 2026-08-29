# GarraIA + Ollama — modelo padrão, `--model` e `ollama launch`

Três coisas diferentes que costumam ser confundidas:

| O quê | Onde vive | Estado |
|---|---|---|
| Modelo Ollama padrão (`qwen3.8:latest`) | Este repositório | ✅ disponível |
| `garraia --model <tag>` | Este repositório | ✅ disponível |
| `ollama launch garraia` | Repositório `ollama/ollama` | ⏳ depende de PR upstream |

---

## 1. Modelo padrão

Sempre que o GarraIA precisa de um modelo Ollama e ninguém disse qual, ele
usa **`qwen3.8:latest`** — que o Ollama resolve para `qwen3.8:27b`
(Q4_K_M, ~18 GB, 262 144 tokens de contexto, texto + imagem + tools).

A string vive em três lugares, cada um com um teste que trava a sua cópia:

| Lugar | Constante |
|---|---|
| `crates/garraia-agents/src/ollama.rs` | `DEFAULT_MODEL` |
| `crates/garraia-cli/src/chat.rs` | `hardcoded_default_model("ollama")` |
| `crates/garraia-cli/src/wizard/local_stack.rs` | `DEFAULT_OLLAMA_MODEL_TAG` |

Trocar o padrão significa editar as três — os testes falham se só uma mudar.

### Escolhendo outro modelo no `garraia init`

O wizard pergunta qual modelo local baixar, com `qwen3.8:latest` como
primeira opção. Também dá para **digitar qualquer tag do Ollama**, incluindo
referências de registry (`hf.co/user/repo:Q4_K_M`):

```text
Qual modelo local o Garra deve usar?
> Qwen 3.8 27B — padrao (~18 GB, 256K de contexto, visao + tools)
  Qwen 3 8B — leve (~5 GB, roda em GPU modesta)
  Qwen 3 14B GGUF — o padrao anterior (~9 GB)
  Llama 3.1 8B (~4.7 GB)
  Outro — digitar a tag do Ollama
  Pular o download por enquanto
```

Numa máquina modesta, prefira `qwen3:8b`: 18 GB de pesos exigem bastante
VRAM, e o download é demorado.

---

## 2. `garraia --model <tag>`

```bash
garraia --model qwen3.8         # abre o REPL direto no modelo local
garraia --model qwen3.8 -y      # e baixa sem perguntar, se faltar
garraia                         # o mesmo, com o modelo padrão
garraia ask --model qwen3.8 "oi"
```

Como a resolução funciona, em ordem:

1. **`--url`** — endpoint explícito (LM Studio, vLLM) ganha de tudo.
2. **Sonda do Ollama local.** A tag é normalizada (`qwen3.8` →
   `qwen3.8:latest`, porque `GET /api/tags` só reporta a forma explícita) e
   procurada no daemon local. Acerto exato → provider Ollama, e acabou.
3. **`agent.default_provider`** do `config.yml`.
4. Cadeia legada: Ollama → Anthropic → OpenAI → OpenRouter.

A sonda tem timeout de 2 s e só vence com acerto exato, então
`--model gpt-4o` nunca é sequestrado para um provider local. E se o
`config.yml` já declara aquele nome sob um provider de nuvem (por exemplo
`llm.openai.model: gpt-4o`), o caminho do Ollama é pulado inteiro — sem isso
o `--model gpt-4o` acabaria oferecendo "baixar gpt-4o do Ollama", um
download que não pode dar certo.

### Modelo ausente

| Contexto | O que acontece |
|---|---|
| Terminal interativo | pergunta se quer baixar, e mostra o progresso |
| `-y` / `--yes` | baixa direto |
| Pipe, CI, `ask --json` | **nunca** pergunta; imprime `ollama pull <tag>` no stderr e segue |

O download usa `POST /api/pull` do próprio daemon, não `ollama pull`: o
binário do Ollama pode não estar no `PATH`, e quando `OLLAMA_BASE_URL`
aponta para outro host um shell-out baixaria na máquina errada. O progresso
vai para o **stderr**, então o stdout de `ask --json` continua sendo uma
única linha JSON.

---

## 3. Configuração headless

Para instalar e configurar sem nenhum prompt — instaladores desatendidos,
imagens de container, ou um launcher de terceiros:

```bash
curl -fsSL https://garraia.org/install.sh | sh -s -- --skip-setup
garraia config set-model --model qwen3.8:latest
garraia start
```

`config set-model` escreve **uma** entrada no `llm:` e a torna
`agent.default_provider`. Tudo o mais no `config.yml` fica intacto, e o
`default_provider` anterior — se apontava para uma entrada que ainda existe
— é rebaixado para o começo de `fallback_providers` em vez de ser
descartado. O arquivo é gravado com modo `0600`.

```
garraia config set-model --model <tag>
    [--provider-key ollama-launch]              # chave no mapa llm:
    [--provider openai]                          # tipo do provider
    [--base-url http://127.0.0.1:11434/v1]       # endpoint
```

Os defaults descrevem o endpoint **compatível com OpenAI** do Ollama
(`/v1`), que é o que launchers locais usam. Para o caminho nativo do Ollama,
passe `--provider ollama --base-url http://127.0.0.1:11434` — aí a entrada
fica sem `api_key`, que é o correto para o protocolo nativo.

### Flags do `install.sh`

`curl … | sh` não tem como passar variáveis de ambiente para o shell do
pipe; `curl … | sh -s -- <flags>` tem. Por isso o installer aceita flags:

| Flag | Equivale a |
|---|---|
| `--skip-setup` | `GARRAIA_SKIP_INIT=1` + `GARRAIA_SKIP_START=1` |
| `--skip-init` | `GARRAIA_SKIP_INIT=1` |
| `--skip-start` | `GARRAIA_SKIP_START=1` |
| `--no-local` | `GARRAIA_BOOTSTRAP_LOCAL=0` |
| `--version <tag>` | `GARRAIA_VERSION=<tag>` |
| `--install-dir <dir>` | `GARRAIA_INSTALL_DIR=<dir>` |

Uma variável de ambiente já definida pelo chamador ganha da flag
correspondente, então automação existente não muda de comportamento.

---

## 4. `ollama launch garraia` — o que falta

O `ollama launch` existe de verdade (Ollama v0.15.0+) e serve para abrir um
agente de terceiros já apontado para um modelo do Ollama:

```bash
ollama launch hermes --model qwen3.8
```

**Mas não dá para se registrar de fora.** O registro de integrações é uma
slice Go compilada dentro do binário do Ollama
([`cmd/launch/registry.go`](https://github.com/ollama/ollama/blob/main/cmd/launch/registry.go)) —
não há manifesto, plugin, nem namespace no ollama.com. As integrações
existentes (Claude Code, Codex, Hermes, OpenCode, …) entraram por pull
request no repositório `ollama/ollama`.

O que este repositório já entrega é o lado do GarraIA do contrato, modelado
no que o runner do Hermes (`cmd/launch/hermes.go`) faz:

| O runner precisa de | GarraIA fornece |
|---|---|
| Instalação não-interativa | `install.sh --skip-setup` |
| Escrever o modelo na config do agente | `garraia config set-model` (ou o YAML direto) |
| Uma chave de provider própria do launcher | `ollama-launch` (default do `set-model`) |
| Endpoint compatível com OpenAI | `http://127.0.0.1:11434/v1`, `api_key: ollama` |
| Detecção de versão | `garraia --version` |
| Abrir o agente | `garraia chat` |

O `config.yml` que um runner escreveria:

```yaml
llm:
  ollama-launch:
    provider: openai
    model: qwen3.8:latest
    api_key: ollama
    base_url: http://127.0.0.1:11434/v1
agent:
  default_provider: ollama-launch
```

### O patch já está escrito e validado

O código Go da integração está versionado neste repositório em
[`contrib/ollama-launch/`](../../contrib/ollama-launch/) — `garraia.go`,
`garraia_test.go` e o patch do `registry.go`, mais o passo a passo para
abrir o PR. Ele foi escrito contra o `ollama/ollama` real e validado lá
dentro: `gofmt` limpo, `go vet` limpo, `go build ./...` ok e a **suíte
`./cmd/launch/` inteira verde**, incluindo os 10 testes novos.

Falta só o passo que exige um fork: `gh repo fork ollama/ollama`, aplicar
os três arquivos e abrir o PR.

Enquanto o PR upstream não é aceito, o equivalente manual é:

```bash
curl -fsSL https://garraia.org/install.sh | sh -s -- --skip-setup
garraia config set-model --model qwen3.8:latest
garraia --model qwen3.8
```

> Aceitar a integração é decisão dos mantenedores do Ollama. O suporte a
> Windows fica de fora da primeira versão: o GarraIA não publica um
> instalador `.ps1` para web (só `scripts/build-installer.ps1`, que faz
> build local de MSI).
