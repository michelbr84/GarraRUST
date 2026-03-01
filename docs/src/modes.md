# Modos de Execução (Agent Modes)

Este documento descreve o sistema de Modos de Execução do GarraIA, incluindo conceitos fundamentais, API e configuração.

## Conceitos Fundamentais

### O que é um "Modo"?

Um **modo** é uma **estratégia de execução** que define como o agente deve agir em uma determinada sessão. Diferente de "personalidades", os modos focam em **comportamento operacional**:

- Quais ferramentas estão disponíveis
- Quais são os limites de execução
- Quais configurações de LLM usar
- Qual é o system prompt base

### Precedência de Modo

O modo é resolvido nesta ordem de prioridade:

1. **Header `X-Agent-Mode`** (maior prioridade)
2. **Comando `/mode <nome>`** (via chat)
3. **Preferência por canal** (configuração por canal)
4. **Preferência por usuário** (futuro)
5. **Default** (`ask` para Telegram, `auto` para outros)

## Modos Disponíveis

| Modo | Descrição | Ferramentas |
|------|-----------|-------------|
| `auto` | Decide automaticamente via heurísticas | todas (limitado) |
| `search` | Busca e inspeção sem modificar | read-only |
| `architect` | Design e planejamento | limitadas |
| `code` | Implementação ativa | todas |
| `ask` | Apenas perguntas (padrão Telegram) | opcional |
| `debug` | Análise de erros e logs | read-only + bash |
| `orchestrator` | Execução multi-etapas | todas |
| `review` | Revisão de código | read-only |
| `edit` | Edição pontual | arquivo + bash |

### Padrões por Canal

```rust
// No ModeEngine::new()
channel_defaults.insert("telegram".to_string(), "ask".to_string());
channel_defaults.insert("web".to_string(), "auto".to_string());
channel_defaults.insert("vscode".to_string(), "auto".to_string());
channel_defaults.insert("discord".to_string(), "ask".to_string());
channel_defaults.insert("whatsapp".to_string(), "ask".to_string());
```

## Tool Policy

Cada modo tem uma política de ferramentas que define:

- **Allowed** (whitelist): Lista de ferramentas permitidas
- **Denied** (blacklist): Lista de ferramentas negadas
- **Required**: Ferramentas obrigatórias
- **Whitelist Mode**: Se `true`, nega tudo que não está na lista de allowed

### Exemplos de Política

**Search Mode** (read-only):
```json
{
  "allowed": ["file_read", "repo_search", "list_dir", "web_search", "web_fetch"],
  "denied": ["file_write", "bash"],
  "whitelist_mode": true
}
```

**Code Mode** (todas permitidas):
```json
{
  "whitelist_mode": false
}
```

## API de Modos

### Headers HTTP

| Header | Descrição |
|--------|-----------|
| `X-Agent-Mode` | Forçar um modo específico |
| `X-Session-Id` | ID da sessão para persistência |

### Comandos Telegram

```
/mode          - Mostra o modo atual
/mode <nome>   - Muda para o modo especificado
/mode clear    - Reseta para o modo padrão
/modes         - Lista todos os modos disponíveis
```

### Exemplo de Uso

```bash
# Forçar modo debug via header
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Agent-Mode: debug" \
  -d '{"messages": [{"role": "user", "content": "Error in main.rs"}]}'
```

## Integração com OpenAI API

O endpoint `/v1/chat/completions` suporta:

- **Streaming SSE** via `stream: true`
- **Tool Calling** via `tools` e `tool_choice`
- **Tool Choice**: `"none"`, `"auto"`, `"required"`, ou `{"type": "function", "function": {"name": "..."}}`

### Exemplo com Tools

```json
{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "Liste arquivos em src/"}],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "list_dir",
        "description": "Lista diretório",
        "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
      }
    }
  ],
  "tool_choice": "auto"
}
```

## Auto Mode Router

Quando o modo é `auto`, o sistema usa heurísticas para determinar o modo apropriado:

| Padrão na Mensagem | Modo Resolve |
|-------------------|--------------|
| Path de arquivo (`src/`, `C:\`) | `search` ou `debug` |
| "implementar", "criar", "refatorar" | `code` |
| "o que é", "explique", "?" | `ask` |
| "erro", "stacktrace", "panic" | `debug` |
| "roadmap", "design", "arquitetura" | `architect` |
| "review", "analisar diff" | `review` |

## Configuração Avançada

### Modos Customizados

Você pode criar modos customizados via API:

```json
POST /api/modes/custom
{
  "name": "rust_strict",
  "description": "Rust com políticas restritivas",
  "base_mode": "code",
  "tool_policy": {
    "allowed": ["file_read", "file_write"],
    "denied": ["bash"],
    "whitelist_mode": true
  },
  "llm_config": {
    "temperature": 0.3,
    "max_tokens": 4096
  }
}
```

### Limites por Modo

| Modo | Max Tool Loops | Timeout | Max Turns |
|------|---------------|---------|-----------|
| search | 10 | 15s | 5 |
| ask | 5 | 10s | 3 |
| code | 50 | 30s | 20 |
| orchestrator | 100 | 60s | 30 |

## Continue/VS Code Integration

Para usar com Continue, configure no `config.yaml`:

```yaml
models:
  - name: garra-auto
    provider: openai
    apiBase: http://localhost:3000/v1
    headers:
      X-Agent-Mode: auto

  - name: garra-code
    provider: openai  
    apiBase: http://localhost:3000/v1
    headers:
      X-Agent-Mode: code

  - name: garra-debug
    provider: openai
    apiBase: http://localhost:3000/v1
    headers:
      X-Agent-Mode: debug
```

## Observabilidade

Os logs incluem informações de modo:

```
OpenAI API request: session_id=xxx, user_id=xxx, model=gpt-4, stream=true, mode=debug, tool_choice=auto
```

## FAQ

### Por que o Telegram usa "ask" por padrão?

O modo `ask` é mais seguro para canais de chat porque:
- Não permite escrita em arquivos
- Não executa comandos bash
- Limita o uso de ferramentas

Isso evita ações acidentais destructive em ambientes compartilhados.

### Como mudar o modo padrão de um canal?

No código, ajuste o `ModeEngine`:

```rust
let mut engine = ModeEngine::new();
engine.set_channel_default("telegram", "code"); // Mudar default do Telegram
```

### Posso desabilitar ferramentas completamente?

Sim, use `tool_choice: "none"` na requisição ou configure o modo `ask` que já tem ferramentas limitadas por padrão.
