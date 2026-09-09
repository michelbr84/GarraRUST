# Referência da API REST

O GarraIA expõe uma API REST em `http://127.0.0.1:3888` (porta configurável). Todos os endpoints retornam JSON. O servidor também aceita conexões WebSocket em `/ws`.

---

## Autenticação

A maioria dos endpoints não requer autenticação quando acessados de `localhost`. Para acesso remoto, inclua o token via header:

```
Authorization: Bearer SEU_TOKEN
```

Os endpoints sob `/auth/` (autenticação mobile) usam JWT com expiração de 30 dias.

---

## Health & Status

### GET /health

Verifica se o servidor está operacional.

**Response 200:**
```json
{
  "status": "ok",
  "version": "0.9.0"
}
```

---

## Chat

### POST /api/chat

Envia uma mensagem ao agente e recebe a resposta.

**Body:**
```json
{
  "message": "Qual é a capital do Brasil?",
  "session_id": "minha-sessao",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5-20250929"
}
```

Campos opcionais: `provider`, `model` (usam o padrão da configuração se omitidos).

**Response 200:**
```json
{
  "response": "A capital do Brasil é Brasília.",
  "session_id": "minha-sessao",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5-20250929",
  "tokens_used": 42
}
```

**Response 400:** Corpo da requisição inválido ou `message` vazia.

**Response 503:** Provedor LLM indisponível.

### GET /api/chat

Retorna o histórico de mensagens de uma sessão.

**Query params:** `session_id` (obrigatório)

**Exemplo:**
```bash
curl "http://127.0.0.1:3888/api/chat?session_id=minha-sessao"
```

**Response 200:**
```json
{
  "session_id": "minha-sessao",
  "messages": [
    {"role": "user", "content": "Qual é a capital do Brasil?", "timestamp": "2026-04-06T10:00:00Z"},
    {"role": "assistant", "content": "A capital do Brasil é Brasília.", "timestamp": "2026-04-06T10:00:01Z"}
  ]
}
```

---

## Memória

### GET /api/memory

Retorna fatos extraídos das conversas pelo sistema de memória.

**Query params:** `session_id` (opcional), `query` (busca semântica, opcional)

**Exemplo:**
```bash
curl "http://127.0.0.1:3888/api/memory?query=preferencias+do+usuario"
```

**Response 200:**
```json
{
  "facts": [
    {
      "id": "f1a2b3",
      "content": "O usuário prefere respostas em português.",
      "source_session": "minha-sessao",
      "timestamp": "2026-04-06T09:30:00Z",
      "relevance_score": 0.94
    }
  ]
}
```

---

## Logs

### GET /api/logs

Retorna os logs recentes do servidor.

**Query params:** `level` (`debug`/`info`/`warn`/`error`), `limit` (padrão: 100), `channel` (filtrar por canal)

**Exemplo:**
```bash
curl "http://127.0.0.1:3888/api/logs?level=error&limit=20"
```

**Response 200:**
```json
{
  "logs": [
    {
      "level": "error",
      "message": "Falha na conexão com o provedor Anthropic",
      "timestamp": "2026-04-06T10:05:00Z",
      "context": {"provider": "anthropic", "error": "timeout"}
    }
  ],
  "total": 1
}
```

---

## Administração

### POST /api/admin/reload

Recarrega a configuração sem reiniciar o servidor.

**Response 200:**
```json
{"status": "reloaded", "timestamp": "2026-04-06T10:10:00Z"}
```

**Response 500:** Erro ao parsear o novo `config.yml`.

### POST /api/admin/shutdown

Desliga o servidor de forma controlada.

**Response 200:**
```json
{"status": "shutting_down"}
```

---

## MCP (Model Context Protocol)

### GET /api/mcp/status

Retorna o status de todos os servidores MCP configurados.

**Response 200:**
```json
{
  "servers": [
    {
      "name": "filesystem",
      "status": "connected",
      "tools_available": ["read_file", "write_file", "list_directory"],
      "pid": 12345
    },
    {
      "name": "web-search",
      "status": "disconnected",
      "error": "Processo encerrado inesperadamente"
    }
  ]
}
```

---

## Provedor e Modelo

### POST /api/model/override

Troca o modelo/provedor para todas as sessões novas (sem reiniciar).

**Body:**
```json
{
  "provider": "ollama",
  "model": "mistral"
}
```

**Response 200:**
```json
{
  "previous": {"provider": "anthropic", "model": "claude-sonnet-4-5-20250929"},
  "current": {"provider": "ollama", "model": "mistral"}
}
```

**Response 400:** Provedor não configurado ou modelo não disponível.

---

## Runtime

### GET /api/runtime/state

Retorna o estado atual do runtime do agente.

**Response 200:**
```json
{
  "state": "idle",
  "active_sessions": 2,
  "channels": {
    "telegram": {"status": "connected", "bot_username": "meu_bot"},
    "discord": {"status": "disconnected"}
  },
  "providers": {
    "anthropic": {"status": "available", "model": "claude-sonnet-4-5-20250929"},
    "ollama": {"status": "available", "model": "qwen3.8:latest"}
  },
  "uptime_seconds": 3600
}
```

---

## Sessões

### POST /api/sessions

Cria uma nova sessão. O id é gerado pelo gateway (UUID); quando
`gateway.session_tokens_required` está ligado, a resposta traz também o cookie
`garraia_session` que as chamadas seguintes precisam repetir (ou mandar como
`Authorization: Bearer`).

**Body** (todos os campos opcionais):
```json
{
  "agent_id": "reachy_voice",
  "mode": "search",
  "working_dir": "/home/joao/projetos/alpha"
}
```

- `agent_id` — agente nomeado para a sessão.
- `mode` — modo do agente já na criação: um nativo (`auto`, `search`,
  `architect`, `code`, `ask`, `debug`, `orchestrator`, `review`, `edit`) ou o
  nome de um modo customizado (`POST /api/modes/custom`). É validado como no
  `POST /api/mode/select` e gravado como modo **escolhido**, então a política
  de ferramentas dele já vale na primeira mensagem (#1028).
- `working_dir` — diretório de trabalho da sessão: a base dos caminhos
  relativos que as ferramentas de arquivo (`file_read`, `list_dir`,
  `repo_search`, …) recebem. Tem de ser um diretório existente sob uma das
  raízes permitidas (`GARRAIA_PROJECT_ROOTS`; padrão, o home do usuário) —
  a mesma regra do `path` de `POST /api/projects`.

Outros campos do corpo são ignorados.

**Response 201:**
```json
{
  "session_id": "0f3b7c1e-2d4a-4b8e-9c6f-1a2b3c4d5e6f",
  "agent_id": "reachy_voice",
  "mode": "search",
  "working_dir": "/home/joao/projetos/alpha"
}
```

`mode` volta na grafia gravada (`search`; um customizado vem como foi criado,
ex.: `Auditor`) e `working_dir` volta canonicalizado (symlinks resolvidos);
ambos são `null` quando não foram pedidos.

**Response 400:** `mode` desconhecido, ou `working_dir` fora das raízes
permitidas (um corpo só para todas as variantes, como no `POST /api/projects`,
para não virar oráculo de existência de diretório) — nenhuma sessão é criada.

**Response 503:** `mode` pedido sem `session_store` disponível (não há onde
gravar a política, então o gateway não finge que aplicou).

---

## Projetos

### POST /api/projects

Cria um novo projeto de agente.

**Body:**
```json
{
  "name": "Analisador de Documentos",
  "description": "Agente para analisar contratos PDF",
  "system_prompt": "Você é um especialista em análise de contratos..."
}
```

**Response 200:**
```json
{
  "id": "proj_a1b2c3",
  "name": "Analisador de Documentos",
  "created_at": "2026-04-06T10:20:00Z"
}
```

### GET /api/projects/{id}/files

Lista os arquivos associados a um projeto.

**Response 200:**
```json
{
  "project_id": "proj_a1b2c3",
  "files": [
    {
      "id": "file_x1y2z3",
      "name": "contrato_exemplo.pdf",
      "size_bytes": 204800,
      "uploaded_at": "2026-04-06T09:00:00Z"
    }
  ]
}
```

**Response 404:** Projeto não encontrado.

---

## Skins (Personalização)

### GET /api/skins

Lista os temas visuais disponíveis (cliente desktop Tauri).

**Response 200:**
```json
{
  "skins": [
    {"id": "default", "name": "GarraIA Dark", "active": true},
    {"id": "light", "name": "GarraIA Light", "active": false}
  ]
}
```

### POST /api/skins

Aplica um tema.

**Body:** `{"skin_id": "light"}`

**Response 200:** `{"status": "applied", "skin_id": "light"}`

---

## Plugins

### POST /api/plugins/install

Instala um plugin WASM.

**Content-Type:** `multipart/form-data`

**Campos:**
- `file`: Arquivo `.wasm` (obrigatório)
- `name`: Identificador único (obrigatório)
- `description`: Descrição (opcional)

**Exemplo:**
```bash
curl -X POST http://127.0.0.1:3888/api/plugins/install \
  -F "file=@meu_plugin.wasm" \
  -F "name=meu-plugin" \
  -F "description=Ferramenta customizada"
```

**Response 200:**
```json
{
  "id": "meu-plugin",
  "status": "installed",
  "tools_registered": ["minha_ferramenta"]
}
```

**Response 400:** Arquivo WASM inválido.

### GET /api/plugins

Lista todos os plugins instalados.

**Response 200:**
```json
{
  "plugins": [
    {
      "id": "meu-plugin",
      "description": "Ferramenta customizada",
      "status": "active",
      "tools": ["minha_ferramenta"],
      "installed_at": "2026-04-06T10:30:00Z"
    }
  ]
}
```

### DELETE /api/plugins/{id}

Remove um plugin instalado.

**Response 200:** `{"status": "removed", "id": "meu-plugin"}`

**Response 404:** Plugin não encontrado.

---

## Autenticação Mobile (JWT)

### POST /auth/register

Registra um novo usuário mobile.

**Body:** `{"email": "usuario@exemplo.com", "password": "senha_segura_123"}`

**Response 200:** `{"token": "eyJ...", "user_id": "usr_a1b2c3", "email": "usuario@exemplo.com"}`

**Response 400:** Senha com menos de 8 caracteres ou e-mail inválido.

**Response 409:** E-mail já cadastrado.

### POST /auth/login

Autentica um usuário mobile existente.

**Body:** `{"email": "usuario@exemplo.com", "password": "senha_segura_123"}`

**Response 200:** `{"token": "eyJ...", "user_id": "usr_a1b2c3", "email": "usuario@exemplo.com"}`

**Response 401:** Credenciais inválidas.

**Response 404:** Usuário não encontrado.

### GET /me

Retorna os dados do usuário autenticado.

**Header:** `Authorization: Bearer SEU_JWT`

**Response 200:** `{"user_id": "usr_a1b2c3", "email": "usuario@exemplo.com", "created_at": "2026-03-01T00:00:00Z"}`

**Response 401:** Token inválido ou expirado.

---

## Chat Mobile (JWT obrigatório)

### POST /chat

Envia uma mensagem ao agente via cliente mobile.

**Header:** `Authorization: Bearer SEU_JWT`

**Body:** `{"message": "Resuma este documento.", "session_id": "mobile-usr_a1b2c3"}`

**Response 200:** `{"response": "O documento trata de...", "session_id": "mobile-usr_a1b2c3"}`

**Response 401:** Token inválido.

### GET /chat/history

Retorna o histórico de chat do usuário mobile.

**Header:** `Authorization: Bearer SEU_JWT`

**Query params:** `limit` (padrão: 50), `offset` (padrão: 0)

**Response 200:**
```json
{
  "messages": [
    {"role": "user", "content": "Olá!", "timestamp": "2026-04-06T08:00:00Z"},
    {"role": "assistant", "content": "Olá! Como posso ajudar?", "timestamp": "2026-04-06T08:00:01Z"}
  ],
  "total": 2,
  "has_more": false
}
```

---

## WebSocket

### WS /ws

Conexão WebSocket para chat, opcionalmente com o texto chegando token a token.

**URL de conexão:** `ws://127.0.0.1:3888/ws?token=SUA_CHAVE`

A chave é a `gateway.api_key`; ela também é aceita como `?api_key=` ou no
header `Authorization: Bearer`. A query existe porque o handshake WebSocket
de um navegador não leva header.

#### Mensagens do cliente

Assim que a conexão abre, o servidor manda um `connected` com o `session_id`.
A primeira mensagem do cliente pode ser um `init` (sessão nova) ou um
`resume`; qualquer outra coisa já é tratada como um turno de conversa.

```json
{"type": "init"}
{"type": "resume", "session_id": "…", "session_token": "…"}
{"content": "Explique o que é Rust.", "stream": true}
{"type": "stop", "session_id": "…"}
```

| Campo do turno | Efeito |
|---|---|
| `content` | O texto do usuário. Uma mensagem que não seja JSON com `content` é tratada como o próprio texto. |
| `stream` | `true` liga os frames intermediários abaixo. Ausente ou `false`, a resposta chega só no `message` final — o contrato de sempre. |
| `provider` | Sobrescreve o provider para este turno. |
| `model` | Sobrescreve o modelo para este turno. |

O `stop` cancela o turno em andamento. Sem `session_id` vale para a sessão da
própria conexão; com `session_id` diferente, o servidor responde
`session_mismatch` e não cancela nada. Mensagens enviadas enquanto um turno
roda entram numa fila e são processadas na sequência.

#### Mensagens do servidor

| Tipo | Quando | Campos |
|---|---|---|
| `connected` | ao abrir a conexão | `session_id`, `session_token` |
| `resumed` | após um `resume` aceito | `session_id`, `history_length`, `session_token` |
| `delta` | só com `stream: true` | `session_id`, `content` — um pedaço do texto |
| `tool_started` | só com `stream: true` | `session_id`, `name`, `detail` |
| `tool_finished` | só com `stream: true` | `session_id`, `name`, `success`, `summary`, `duration_ms` |
| `message` | sempre, ao fim do turno | `session_id`, `content` — a resposta inteira |
| `stopped` | após um `stop` aceito | `session_id` |
| `error` | falha | `session_id`, `code`, `message` |

`tool_finished` **não** carrega a saída da ferramenta: ela pode ter dezenas de
KiB, e o frame existe para dizer que a ferramenta terminou.

O `message` final é sempre enviado, com o texto completo, mesmo quando os
`delta` já entregaram o mesmo conteúdo — é ele o valor autoritativo, e é o que
mantém um cliente que ignora os frames novos funcionando sem mudança.

Um turno cancelado não é persistido: nem no histórico da sessão, nem na
memória. O efeito de uma ferramenta que já rodou não é desfeito.

Códigos de `error`: `prompt_injection_detected`, `agent_error`,
`rate_limited`, `message_too_large`, `session_mismatch`, `turn_in_progress`.

**Response 401:** Chave ausente ou inválida (a conexão é recusada antes do upgrade WebSocket).
