---
name: garra-cli-operator
description: Operar Garra (mcp/ask/chat) com guardrails de custo, timeout, sandbox e opt-in para openrouter/auto.
triggers:
  - garra
  - garra ask
  - garra chat
  - mcp__garra
  - garra-cli-operator
dependencies: []
---

# Garra CLI Operator

Esta skill define como o Claude Code chama o Garra como **agente auxiliar local**. O Garra oferece três canais com perfis de risco diferentes; esta skill obriga a escolha correta, o custo correto e a verificação pós-execução.

Pré-condições empíricas (verificadas 2026-05-11):

- `garra chat --provider openrouter` SEM `--model` resolve para `openrouter/auto` (caro). **Sempre passar `--model` explicitamente.**
- `garra chat` NÃO tem `--timeout-secs` próprio — só `timeout(1)` do shell.
- REPL do `chat` termina com `/exit` ou `/quit` (banner anuncia `/exit`).

---

## 1. Decision tree — qual canal escolher

| Cenário | Canal | Justificativa |
|---------|-------|---------------|
| Pergunta simples, LLM-only, sem arquivo | `mcp__garra__garra_ask` | In-process, JSON parseável, audit-tested LLM-only |
| Pergunta simples para script/pipeline (stdout puro, exit code Unix) | `garra ask --json …` | Exit codes `0/2/65/69/74/124`, sem banner ANSI |
| Garra precisa ler arquivo/projeto/tool interna | `garra chat …` via stdin pipe | Tools (`file_read`, etc.) só existem no runtime do `chat` |
| Tarefa complexa que justifica `openrouter/auto` | mesmo `chat`, com **autorização explícita do usuário** | Política de custo locked-in 2026-05-11 (`docs/cli-mcp-server.md`) — nunca auto-upgrade |

Default sempre `openrouter/free`. `openrouter/auto` é **opt-in explícito** — exige o usuário pedir "use auto" ou aprovar a tarefa complexa no turno.

---

## 2. Regras absolutas (segurança e custo)

- **NUNCA** rodar `garra chat` sem prefixo `timeout N` no shell.
- **SEMPRE** passar `--provider` E `--model` explicitamente em `chat`. Sem `--model`, o `chat` resolve para `openrouter/auto`.
- **NUNCA** invocar `openrouter/auto` sem autorização explícita do usuário no turno corrente.
- **NUNCA** pedir ao Garra para ler `.env`, `~/.garraia/config.yml`, vaults, tokens, chaves API, secrets de qualquer tipo.
- **NUNCA** permitir escrita do Garra fora de `target/garra-cli-smoke/`. Esse é o único diretório-sandbox. Qualquer outro path requer aprovação explícita do usuário no turno.
- **SEMPRE** rodar `git status --short` antes E depois de qualquer chamada `chat` que possa escrever.
- **NUNCA** fazer commit automático de arquivos gerados pelo Garra — usuário sempre revisa o diff primeiro.
- **NUNCA** confiar cegamente: qualquer fato concreto que o Garra afirmar sobre um arquivo deve ser verificado via `Read`/`Grep`/`Glob`.

---

## 3. Comandos canônicos

### A. Pergunta simples — preferir MCP

Use a tool `mcp__garra__garra_ask` com `message="..."`. Sem flags adicionais; default já é `openrouter/free`.

### B. Pergunta simples via CLI (JSON pipeline, scripts)

```bash
garra ask --provider openrouter --model openrouter/free \
  --json --timeout-secs 30 "<pergunta>"
```

`garra ask` é LLM-only (audit-tested). Sem acesso a arquivos.

### C. Leitura de arquivo via `chat` (smoke, free)

```bash
printf '<instrução para o Garra>\n/quit\n' \
  | timeout 90 garra chat --provider openrouter --model openrouter/free
```

`chat` tem tools registradas no runtime — consegue ler arquivos do diretório de trabalho.

### D. Análise complexa read-only — `auto` só com autorização

```bash
# Só rode esta variante se o usuário disse "use auto" ou autorizou a tarefa complexa.
printf '<tarefa complexa, NÃO editar arquivos>\n/quit\n' \
  | timeout 180 garra chat --provider openrouter --model openrouter/auto
```

### E. Teste de escrita em sandbox — APROVAÇÃO EXPLÍCITA OBRIGATÓRIA

```bash
# REQUER: usuário aprovou a escrita NESTE TURNO.
# REQUER: usuário autorizou openrouter/auto (ou trocar para --model openrouter/free).
mkdir -p target/garra-cli-smoke
printf 'Crie target/garra-cli-smoke/<file>.txt com <conteúdo>. Não edite mais nada.\n/quit\n' \
  | timeout 120 garra chat --provider openrouter --model openrouter/auto
git status --short  # validar escopo da escrita
```

**Regra:** escrita só em `target/garra-cli-smoke/`. Nenhum outro path.

---

## 4. Validação pós-execução (obrigatória)

Após qualquer chamada `chat`:

1. Exit code do pipeline (`$?` no Bash, `$LASTEXITCODE` no PowerShell).
2. Banner mostra `Provider: <esperado>` e `Model: <esperado>` — se model veio diferente do pedido, abortar.
3. `git status --short` — comparar com snapshot pré-call. Arquivo fora de `target/garra-cli-smoke/` é red flag imediato.
4. Se houve escrita: `Read` o arquivo, validar conteúdo, mostrar diff ao usuário.
5. Em erro do provider: reportar `error.kind` (`provider_error`, `timeout`, `usage`) sem mascarar; fingerprints `sk-…` já vêm redacted pelo `sanitize_provider_error`.

---

## 5. Falhas comuns

| Sintoma | Causa provável | Correção |
|---------|----------------|----------|
| Banner mostra `Model: openrouter/auto` quando esperado era `free` | `--model` omitido | Sempre passar `--model` explícito |
| `error.kind: provider_error` 401 | `OPENROUTER_API_KEY` ausente do shell | Verificar env vars do terminal que invocou |
| Pipeline trava | `chat` é REPL — esperando stdin | Sempre terminar input com `\n/quit\n` E prefixar `timeout` |
| `timeout: command not found` | PowerShell nativo, ou Windows sem GNU coreutils | Ver §6 (Windows/Git Bash) |
| `garra: command not found` | binário stale ou não instalado | `garra --version`; reinstalar via plano de update |
| Confusão entre `garra` e `garraia` | binário legacy renomeado em GAR-579+ | Usar sempre `garra` (atual); `garraia` é apenas placeholder histórico |
| Output poluído por ANSI | banner do `chat` em stdout | `tail -N` o output e parsear só a parte relevante |

---

## 6. Windows / Git Bash — nota sobre `timeout`

O comando `timeout` se comporta diferente conforme o shell:

- **Git Bash (MSYS2/MinGW), WSL, Linux, macOS:** `timeout 90 <cmd>` é o GNU coreutils `timeout(1)` — mata o processo após N segundos. ✓ funciona com os comandos C, D, E.
- **PowerShell nativo (Windows):** `timeout` é um binário Windows diferente que **pausa** por N segundos esperando tecla — NÃO é coreutils. Use uma destas alternativas:
  - Rodar o comando dentro de Git Bash (recomendado).
  - Usar `Start-Job { … } | Wait-Job -Timeout 90` no PowerShell.
  - Confiar na interrupção manual do usuário (último recurso).
- **CMD legado:** evitar; usar Git Bash.

Quando o ambiente não tem `timeout(1)` funcional, **prefira `garra ask` em vez de `garra chat`** sempre que possível — `garra ask` tem `--timeout-secs` próprio que funciona em qualquer shell.

---

## 7. Cross-references

- [`docs/cli-ask.md`](../docs/cli-ask.md) — surface completa de `garra ask` (flags, exit codes, JSON envelope `garra.ask.v1`).
- [`docs/cli-mcp-server.md`](../docs/cli-mcp-server.md) — wrapper MCP, política de custo locked-in 2026-05-11.
- `crates/garraia-cli/src/ask.rs` — fonte do path `ask` (audit-tested LLM-only).
- `crates/garraia-cli/src/chat.rs` — fonte do path `chat` e tools registradas.
- [`skills/superpowers-bridge.md`](superpowers-bridge.md) — skill irmã, mapeia outras skills locais.

---

## 8. Critérios de sucesso de uma chamada

- Garra respondeu dentro do `timeout`.
- `Provider:` e `Model:` no banner batem com o pedido.
- Nenhum secret/PII vazou em stdout/stderr/logs.
- Nenhuma escrita fora de `target/garra-cli-smoke/` aconteceu.
- Qualquer arquivo criado foi validado por `Read`.
- Usuário recebeu resumo claro: o que o Garra disse, o que foi verificado independentemente, qual o exit code.
