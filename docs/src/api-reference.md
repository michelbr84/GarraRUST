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
    "ollama": {"status": "available", "model": "llama3.1"}
  },
  "uptime_seconds": 3600
}
```

---

## Sessões

### POST /api/sessions

Cria uma nova sessão explicitamente.

**Body:**
```json
{
  "session_id": "projeto-alpha",
  "metadata": {
    "user": "joao",
    "channel": "api"
  }
}
```

**Response 200:**
```json
{
  "session_id": "projeto-alpha",
  "created_at": "2026-04-06T10:15:00Z"
}
```

**Response 409:** Session ID já existe.

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

Conexão WebSocket para chat em tempo real com streaming de tokens.

**URL de conexão:** `ws://127.0.0.1:3888/ws?token=SEU_TOKEN`

**Mensagem de entrada:**
```json
{
  "type": "chat",
  "message": "Explique o que é Rust.",
  "session_id": "ws-teste"
}
```

**Mensagens de saída (streaming):**
```json
{"type": "token", "content": "Rust"}
{"type": "token", "content": " é uma linguagem"}
{"type": "done", "session_id": "ws-teste", "total_tokens": 150}
```

| Tipo | Descrição |
|------|-----------|
| `token` | Fragmento de token da resposta em streaming |
| `done` | Resposta completa; inclui `total_tokens` |
| `error` | Erro durante a geração; inclui `message` |
| `tool_call` | O agente está chamando uma ferramenta |
| `tool_result` | Resultado da ferramenta chamada |

**Response 401:** Token ausente ou inválido (a conexão é recusada antes do upgrade WebSocket).
