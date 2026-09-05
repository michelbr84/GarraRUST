# Integração Hermes ↔ GarraIA (MCP + A2A)

> Guia prático de interoperação entre o GarraIA e um agente externo
> ("Hermes" — qualquer host MCP: Claude Code, Claude Desktop, ou outro
> agente com cliente MCP). Cobre os dois sentidos e o desenho da
> conversa autônoma bidirecional.

## 1. Hermes → Garra: `garra_ask` como ferramenta nativa (pronto hoje)

O `garra mcp-server` expõe a ferramenta **`garra_ask`** via MCP sobre
stdio (rmcp, protocolo `2025-11-25`). É self-contained: **não** precisa
do daemon `garra start` — instancia o provider LLM in-process
(`crates/garraia-cli/src/mcp_server.rs`).

### Registro no Claude Code / Hermes (CLI)

```bash
claude mcp add garraia -- /caminho/para/garra mcp-server
```

Ou em qualquer host que leia `mcp.json` (Claude Desktop etc.):

```json
{
  "mcpServers": {
    "garraia": {
      "command": "/caminho/para/garra",
      "args": ["mcp-server"]
    }
  }
}
```

Neste repositório, o [`.mcp.json`](../../.mcp.json) da raiz já registra o
servidor apontando para o build local (`./target/release/garra`) — toda
sessão Claude/Hermes aberta no repo ganha `garra_ask` automaticamente.

### Contrato da ferramenta

`garra_ask` recebe `{message, provider?, model?, timeout_secs?,
system_prompt?}` (defaults: `openrouter` / `openrouter/free`; bounds no
schema; `additionalProperties: false`) e devolve o envelope
`garra.ask.v1` como conteúdo de texto, com `isError` espelhando
`ok`. O provider precisa de credencial configurada no ambiente do
GarraIA (`~/.garraia/config.yml`, cofre ou env var) — a chave nunca
transita pelo MCP.

Para smoke tests **sem chave nenhuma**, compile com a feature dev
(`cargo build --release -p garraia --features dev-echo-provider`) e
chame `{"provider": "echo"}` — o enum do schema só inclui `echo` nesse
build; produção permanece sem o caminho keyless.

### Teste real executado (protocolo completo, keyless)

Transcript de `initialize → tools/list → tools/call` contra o binário
com `dev-echo-provider` (evidência colada do teste executado em
2026-08-29, já contra o rmcp 2.2 — ver plan 0358):

```text
>>> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25",
     "capabilities":{},"clientInfo":{"name":"hermes-e2e-test","version":"1.0.0"}}}
<<< {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25",
     "capabilities":{"tools":{}},"serverInfo":{"name":"rmcp","version":"2.2.0"}}}
>>> {"jsonrpc":"2.0","method":"notifications/initialized"}
>>> {"jsonrpc":"2.0","id":2,"method":"tools/list"}
<<< ... tools: ["garra_ask"], provider enum: ["ollama","anthropic","openai","openrouter","echo"] ...
>>> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"garra_ask",
     "arguments":{"message":"Responda apenas: HERMES-GARRA-OK","provider":"echo"}}}
<<< {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":
     "{\"answer\":\"[echo] Responda apenas: HERMES-GARRA-OK\",\"latency_ms\":0,
       \"model\":\"openrouter/free\",\"ok\":true,\"provider\":\"echo\",
       \"schema\":\"garra.ask.v1\"}"}],"isError":false}}
```

> Nota: o campo `model` do envelope reporta o default do schema quando o
> caller não passa `model` — com `provider: echo` o modelo é ignorado
> pelo provider (passe `"model": "echo-stub"` para um envelope 100%
> coerente).

## 2. Garra → Hermes: Hermes como servidor MCP do Garra (pronto hoje, zero código)

O GarraIA já é **cliente MCP** (stdio; Streamable HTTP atrás da feature
`mcp-http`). O Claude Code expõe um servidor MCP próprio via
`claude mcp serve`. Logo, para o Garra chamar o Hermes como ferramenta:

```json
// ~/.garraia/mcp.json (formato compatível com Claude Desktop)
{
  "mcpServers": {
    "hermes": {
      "command": "claude",
      "args": ["mcp", "serve"]
    }
  }
}
```

As tools expostas pelo Hermes aparecem no runtime do Garra com nomes
`hermes.<tool>` e ficam disponíveis ao agente como ferramentas nativas.
Com o §1 + §2 configurados, cada lado chama o outro como ferramenta —
**bidirecionalidade por tool-calling**, sem código novo.

## 3. Conversa autônoma bidirecional: recomendação = A2A

Estado atual no código:

| Peça | Estado |
|---|---|
| Servidor A2A (`GET /.well-known/agent.json`, `POST /a2a/tasks`, get/cancel) | ✅ Completo e **multi-turno stateful** (sessão `a2a:{task_id}`) — `crates/garraia-gateway/src/a2a.rs` |
| Cliente A2A (`A2AClient`: fetch card, create/get/cancel task) | ✅ Consumido pela tool **`a2a_send`** (issue #929) — `crates/garraia-gateway/src/tools/a2a_send_tool.rs` |
| Auth nas rotas A2A | ⚠️ Nenhuma (bind loopback é a única proteção) — hardening necessário antes de expor |

Os dois lados funcionam hoje: outro agente conversa **com** o Garra por
HTTP+JSON puro (contexto preservado entre turnos, ver teste §4), e o
Garra **inicia** conversas pela tool `a2a_send` (issue #929).

### `a2a_send(url, message, task_id?)`

Habilite listando os pares permitidos — sem isso a tool recusa tudo:

```yaml
agent:
  a2a_peers:
    - "http://127.0.0.1:8787"   # Hermes local
```

O match é por base URL normalizada **exata** (scheme + host + porta) —
`http://hermes.local.evil.com` não casa com `http://hermes.local` — e a
lista é lida a cada chamada, então revogar um par vale sem restart.
Guardrails embutidos, todos deny-by-default:

- **SSRF guard** (regra 14): mesmo par listado passa por `vet_url` +
  client pinado nos IPs vetados (`IpScope::AllowPrivate`: LAN/loopback
  sim; link-local/CGNAT/multicast, nunca). Allowlist de config não
  substitui o pin — um DNS que mude de resposta entre a config e a
  chamada é exatamente o ataque que o pin torna inerte.
- **Teto de turnos**: 10 por sessão a cada 10 minutos. Recusa não
  consome cota. Dois agentes em loop sem teto é incidente com data.
- **Timeout por turno**: 120s (o par também roda um LLM).
- A resposta volta com o `task_id`; repassá-lo continua a mesma
  conversa (sessão `a2a:{task_id}` do lado servidor).

**Follow-ups que continuam abertos** (fora do escopo da #929):
1. Comando `garra a2a talk <url>` para conversa supervisionada via CLI.
2. Critério de parada semântico (objetivo atingido/sem progresso) — o
   teto de turnos é o piso mecânico, não a política.
3. Hardening do servidor A2A (token de pareamento igual aos canais)
   antes de qualquer bind fora do loopback — as rotas seguem sem auth,
   e a tool nova torna a topologia de loop menos teórica: trate isso
   antes de expor qualquer um dos lados fora da máquina.

Com (1) dos dois lados — o Hermes já tem tool-calling — a conversa
autônoma vira: um lado abre task A2A no outro, alterna turnos até o
critério de parada, e cada mensagem fica auditável no histórico de
sessão de ambos.

## 4. Teste real A2A multi-turno (keyless)

Gateway com provider `echo` (config mínima, sem segredos — mesmo modelo
do CI), dois turnos na mesma task provando estado conversacional:

```text
GET /.well-known/agent.json
  -> { "name": "garraia", "version": "0.3.3", "capabilities": {...}, "skills": [...] }

POST /a2a/tasks  {"id":"hermes-conv-1","message":{"role":"user","parts":[
                  {"type":"text","text":"Oi Garra, aqui é o Hermes. Turno 1."}]}}
  -> status "completed", artifact: "[echo] Oi Garra, aqui é o Hermes. Turno 1."

POST /a2a/tasks  (MESMO id "hermes-conv-1", turno 2)
  -> status "completed", artifact: "[echo] Turno 2 da mesma conversa."

GET /a2a/tasks/hermes-conv-1 -> status: completed
```

> Semântica do multi-turno: o registro da *task* reflete o último POST;
> o contexto conversacional vive no histórico de sessão `a2a:{id}`, que
> o runtime re-hidrata a cada turno (`a2a.rs::create_task` →
> `hydrate_session_history`/`persist_turn`). Reusar o mesmo `id` é o
> contrato de continuidade.

Config usada:

```yaml
llm:
  echo:
    provider: echo
    model: echo-stub
agent:
  default_provider: echo
```
