# Novidades da v0.5.0

> 🇬🇧 [English version](Whats-New-v0.5.0) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.0)

Release estável de 1 de novembro de 2026 — substitui a v0.4.10 como release `latest`.
Validada como rc1 (30/10) e rc2 (30/10); as correções achadas na validação
dos RCs estão aqui.

A versão é de uma frente: **o turno de chat fica sob o controle do
usuário**. O corte de parede de 120 s que parava respostas longas no meio
saiu do caminho por padrão — e quem precisa de teto volta a configurar, com
a janela agora herdada por toda a CLI e não só pelo gateway.

## O que muda de comportamento

- **Chat sem teto de tempo por padrão (#1751).** `garra chat` deixa de ter
  teto de parede de 120 s; `--timeout-secs` vira opt-in e o texto já
  transmitido fica no histórico marcado como parcial. `0` = sem teto é o
  default do chat interativo.
- **MSRV sobe para 1.97 (#1714, #1750).** `wasmtime` 50 (sem a feature
  `cache`, tirando o zstd vulnerável do lockfile) e o binário novo exigem
  toolchain Rust 1.97+. CI, scripts e docs atualizados.
- **`/model` aceita presets e variantes do OpenRouter (#1752, #1764).**
  `@preset/<slug>` e os sufixos de roteamento `:floor`/`:nitro`/`:online`/
  `:exacto` são validados com uma chamada mínima de 1 token; `--model` na
  linha de comando aceita a mesma string.
- **A API diz quem respondeu (#1733).** `provider`/`model`/`fallback` no
  corpo e headers `X-AI-Provider`/`X-AI-Model`; modelo inválido vira 4xx
  sanitizado em vez de 500 com o corpo cru (#1726).
- **Fallback e health honestos (#1735, #1743).** `fallback_providers` entra
  quando o primário recusa a credencial, e o `/api/health` deixa de mostrar
  verde com chave recusada.
- **TTS da Web volta a tocar (#1754)** e `/clear` apaga o histórico
  persistido (#1723).
- **Windows de primeira classe (#1697, #1722, #1720, #1737).** O MCP
  `filesystem`, o `file_jail`, o `garraia rollback` e o diagnóstico de voz
  foram corrigidos; a suíte unitária roda também em Windows e macOS no CI.

## Correções do trem de validação (rc2 → estável)

- **200 vazio entra no circuito de retry/fallback (#1776):** um `Ok` sem
  texto e sem ferramenta re-tenta uma vez, cai para os fallbacks e registra
  uma falha no breaker por turno.
- **CLI herda os timeouts da config (#1778):** providers montados pela CLI
  respeitam `config.timeouts` (antes: client cru, chat contra Ollama/
  llama.cpp rodava sem teto).
- **3xx vira diagnóstico próprio (#1779):** redirecionamento não cai mais no
  ramo de erro 4xx/5xx com corpo vazio; a `Location` entra na mensagem,
  sanitizada. Política de redirect intocada (regra 14).
- **`code_review` com teto próprio (#1780):** a chamada LLM interna da
  ferramenta ganha timeout próprio (180 s) e o modo `review` sobe para
  120 s — a revisão deixa de morrer antes de o provider responder.
- **Gate reclassifica PowerShell (#1781):** `[Environment]::OSVersion` deixa
  de ser falso-positivo do substring `environ` e `Invoke-WebRequest`/
  `Invoke-RestMethod`/`Invoke-Expression` (e os alias `iwr`/`irm`/`iex`) por
  extenso deixam de passar livre.
- **`garra memory stats` honesto (#1782):** deixa de prometer conserto que o
  reindex não faz.
- **`garra update` não convida a rebaixar (#1772)** e **wasmtime 50.0.1
  fecha quatro advisories do 50.0.0 (#1774)** — os dois já na rc2.

## Como instalar

Windows: `irm https://garraia.org/install.ps1 | iex` · Linux/macOS:
`curl -fsSL https://garraia.org/install.sh | sh` · Docker: `docker pull
ghcr.io/michelbr84/garraia:0.5.0` — e o `garra update` a partir do 0.4.10.
