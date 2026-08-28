# Capacidades extras via MCP — receitas com política de segurança

> Complemento de [docs/hardening-gateway.md](../hardening-gateway.md).
> As tools nativas (`bash`, `file_read`, `file_write`, `web_fetch`,
> `repo_search`, `git_diff`, agendamento) já cobrem o essencial de um
> agente de terminal. Capacidades ALÉM disso entram como **servidores
> MCP** — cada um com escopo mínimo, limites de recurso e secrets no
> cofre.

## Onde configurar

Servidores MCP vivem em `<config_dir>/mcp.json` (formato compatível com
Claude Desktop, chaves em camelCase) **ou** na seção `mcp:` do
`config.yml` (snake_case, com o extra `allowed_tools`). Campos de
política disponíveis:

| Campo (`mcp.json`) | Campo (`config.yml`) | Efeito |
|---|---|---|
| `memoryLimitMb` | `memory_limit_mb` | cap de memória virtual do processo (Unix) |
| `maxRestarts` / `restartDelaySecs` | `max_restarts` / `restart_delay_secs` | política de restart com backoff |
| `timeoutSecs` | `timeout` | timeout de inicialização |
| — | `allowed_tools` | **allowlist de tools do servidor** (vazio = todas) |
| `env` com valores sensíveis | idem | viram `vault:mcp.<server>.<KEY>` quando `GARRAIA_VAULT_PASSPHRASE` está setada |

## Receita 1 — Filesystem com escopo restrito

O provisionamento automático aponta o server `filesystem` para o `$HOME`
inteiro. Restrinja ao(s) diretório(s) que o agente realmente precisa:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/voce/projetos"],
      "memoryLimitMb": 512
    }
  }
}
```

E, se quiser só leitura, corte as tools de escrita na seção `mcp:` do
`config.yml`:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/voce/projetos"]
    memory_limit_mb: 512
    allowed_tools: ["read_file", "read_multiple_files", "list_directory", "search_files", "get_file_info", "directory_tree"]
```

## Receita 2 — Navegação web real (browser)

O `web_fetch` nativo baixa páginas; para interação real (JS, cliques,
screenshots) use um MCP de browser:

```json
{
  "mcpServers": {
    "browser": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-puppeteer"],
      "memoryLimitMb": 1024,
      "timeoutSecs": 60
    }
  }
}
```

Política recomendada: browser dá exfiltração bidirecional (o agente lê
E envia dados a qualquer site). Use com `tool_confirmation_enabled: true`
e prefira modos restritos nos canais públicos.

## Receita 3 — Banco de dados (Postgres) com credencial no cofre

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-postgres"],
      "env": {
        "DATABASE_URL": "postgres://usuario:senha@localhost:5432/app"
      },
      "memoryLimitMb": 512
    }
  }
}
```

Com `GARRAIA_VAULT_PASSPHRASE` exportada, no primeiro save o valor de
`DATABASE_URL` (nome contém pattern sensível? não — atenção: a detecção
cobre nomes com `key/token/secret/password/auth/credential/pass`; um
env chamado `DATABASE_URL` **não** é movido ao cofre automaticamente.
Prefira nomeá-lo `DB_PASSWORD` + montar a URL no server, ou aceite que
a URL fica no `mcp.json` e proteja o arquivo com `chmod 0600`). Use um
usuário de banco **read-only** para o agente sempre que possível.

## Receita 4 — "Controle de aplicativos" (estado honesto)

Não existe tool nativa para controlar aplicativos GUI. Os caminhos
reais, todos com implicações fortes de segurança:

1. **MCP dedicado por SO** — ex.: um server de automação (AppleScript no
   macOS, `xdotool`/AT-SPI no Linux, UIA no Windows). Nenhum vem
   embutido; ao adotar um de terceiros, revise o código antes (ele roda
   com os seus privilégios) e aplique `allowed_tools` + confirmação.
2. **Plugin WASM** (`--features plugins`) — sandbox com caps de memória
   e deadline, mas exige escrever o plugin.
3. Não usar o `bash` como atalho para isso em canal público — modos
   `ask`/`search` existem exatamente para esse contexto.

## Checklist ao adicionar QUALQUER servidor MCP

- [ ] Escopo mínimo (diretório, banco, credencial read-only)
- [ ] `memoryLimitMb` definido
- [ ] `allowed_tools` no `config.yml` quando o servidor expõe mais do que você quer
- [ ] Secrets com nome sensível (para irem ao cofre) + `GARRAIA_VAULT_PASSPHRASE` setada
- [ ] `tool_confirmation_enabled: true` no `agent`
- [ ] `garra mcp list` / `garra mcp inspect <nome>` para conferir o que ficou exposto
- [ ] Nunca um segundo shell via MCP (contorna o safety_gate do `bash` nativo)
