# Instalação e Primeiros Passos

## Instalação em 1 comando (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
```

O script baixa o binário da release mais recente (verificado por SHA-256 via `SHA256SUMS`), roda `garra init` (wizard de provedor LLM + cofre criptografado de credenciais) e `garra start`.

Variáveis úteis do instalador:

| Variável | Efeito |
|---|---|
| `GARRAIA_SKIP_INIT=1` | pula o wizard `garra init` |
| `GARRAIA_SKIP_START=1` | instala sem iniciar o agente |
| `GARRAIA_BOOTSTRAP_LOCAL=1` | usa artefatos locais em vez de baixar |

Se o `raw.githubusercontent.com` devolver HTTP 429 (rate-limit), use o espelho da release:

```bash
curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh
```

**Windows:** baixe o binário na [página de releases](https://github.com/michelbr84/GarraRUST/releases). Binários pré-compilados: Linux (x86_64 e ARM64 a partir da v0.3.2), macOS e Windows.

## Primeiros passos

```bash
garra init      # escolhe o provedor LLM e grava a chave no cofre AES-256-GCM
garra start     # inicia o agente (use --daemon para segundo plano, --with-voice para voz)
garra ask "resuma este arquivo" # pergunta única, sem chat interativo
garra status    # verifica se está rodando
```

## Atualização e rollback

```bash
garra update    # auto-atualização com verificação SHA-256
garra rollback  # volta para a versão anterior
```

> `garra update` retorna 404 em instalações anteriores à v0.2.1 — nesse caso, reinstale com o one-liner acima.

## Build a partir do código-fonte

```bash
git clone https://github.com/michelbr84/GarraRUST.git && cd GarraRUST
cargo build --release -p garraia          # requer Rust 1.94+
cargo build --release -p garraia --features plugins   # com suporte a plugins WASM
```

Para o app desktop (Tauri, Windows MSI) e detalhes por plataforma, veja o guia completo: [docs/installation.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/installation.md) · [Início Rápido do README](https://github.com/michelbr84/GarraRUST/blob/main/README.md#in%C3%ADcio-r%C3%A1pido) · [Deploy com Docker](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment.md).
