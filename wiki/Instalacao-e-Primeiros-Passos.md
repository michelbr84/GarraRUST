# Instalação e Primeiros Passos

## Instalação em 1 comando (Linux / macOS)

```bash
curl -fsSL https://garraia.org/install.sh | sh
```

O script baixa o binário da release mais recente (verificado por SHA-256 via `SHA256SUMS`), roda `garra init` (wizard de provedor LLM + cofre criptografado de credenciais) e `garra start`.

Variáveis úteis do instalador:

| Variável | Efeito |
|---|---|
| `GARRAIA_SKIP_INIT=1` | pula o wizard `garra init` |
| `GARRAIA_SKIP_START=1` | instala sem iniciar o agente |
| `GARRAIA_BOOTSTRAP_LOCAL=0` | suprime os prompts de GPU/Ollama dentro do wizard (repassado ao `init`) |
| `GARRAIA_VERSION=vX.Y.Z` | fixa uma release em vez de usar a mais recente |
| `GARRAIA_INSTALL_DIR=<dir>` | instala em outro diretório |

Espelhos do mesmo script (sincronizados automaticamente):

```bash
curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
```

## Instalação em 1 comando (Windows)

```powershell
irm https://garraia.org/install.ps1 | iex
```

Irmão Windows do `install.sh`: verifica o SHA-256, instala `garraia.exe` em
`%LOCALAPPDATA%\Programs\GarraIA`, registra no PATH do usuário e encadeia
`init` + `start`. Sem privilégio de administrador.

Para passar flags — `irm | iex` não recebe argumentos:

```powershell
& ([scriptblock]::Create((irm https://garraia.org/install.ps1))) -SkipSetup
```

Espelhos:

```powershell
irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
```

Os binários e instaladores não são assinados, então o SmartScreen avisa na
primeira execução do MSI do desktop. Detalhes em
[`docs/installation.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/installation.md).

**Binários pré-compilados:** Linux (x86_64 e ARM64 a partir da v0.3.2), macOS e
Windows — cada um também como archive (`.tar.gz`, ou `.zip` no Windows). Veja a
[página de releases](https://github.com/michelbr84/GarraRUST/releases).

**Requisito mínimo (Linux):** os binários publicados exigem **glibc ≥ 2.35** (Ubuntu 22.04+, Debian 12+) — o `install.sh` verifica isso antes de baixar e aborta com instruções se a distro for mais antiga. Em musl (Alpine) ou glibc anterior, compile do source. Detalhes em [`docs/installation.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/installation.md).

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

Para o app desktop (Tauri, Windows MSI) e detalhes por plataforma, veja o guia completo: [docs/installation.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/installation.md) · [Início Rápido do README](https://github.com/michelbr84/GarraRUST/blob/main/README.pt-BR.md#in%C3%ADcio-r%C3%A1pido) · [Deploy com Docker](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment.md).
