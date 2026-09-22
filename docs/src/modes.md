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

A whitelist cobre **ferramenta MCP também** (#1264). Uma ferramenta de
servidor MCP se chama `servidor__ferramenta`; para liberar um servidor
inteiro sem listar ferramenta por ferramenta, declare o prefixo na
`allowed` com a sintaxe `servidor/*` — `meu-servidor/*` cobre
`meu-servidor__<qualquer nome>`, e só ele. Nome completo na lista
(`meu-servidor__consulta`) libera só aquela. `denied` vence o prefixo.

Aviso importante: `whitelist_mode: true` com `allowed` vazia **permite
tudo** — é compatibilidade preservada, não proteção. O runtime emite um
aviso por turno nesse caso (`#1264`); popule a lista ou desligue a flag.

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

### `tool_program` (#1226)

`tool_program` é uma ferramenta **intrínseca** do runtime: o modelo manda
uma lista de passos (`{"steps": [{"tool": "...", "args": {...}, "as": "..."}]}`)
e o runtime os executa em sequência, no mesmo turno, sem voltar ao LLM entre
um passo e outro. Ela não é registrada como as outras: aparece na lista que o
modelo vê sempre que existe ao menos uma ferramenta real registrada, e passa
pelo mesmo filtro do modo.

**Quais modos nativos a expõem.** Os perfis que não usam whitelist: `auto`,
`code` e `ask` (e a sessão sem modo escolhido). Os perfis com whitelist
(`search`, `architect`, `debug`, `orchestrator`, `review`, `edit`) não a
listam em `allowed`, então não a expõem. Com `/mode auto`, vale o perfil do
modo que a heurística escolher para a mensagem — e, se ela não classificar,
o portão aberto, que expõe. O teste
`tool_program_exposto_so_nos_perfis_nativos_sem_whitelist` fixa essa tabela —
mudar a exposição de um modo nativo tem de ser decisão deliberada.

**Decisão (#1226 S-C): os perfis nativos com whitelist ficam sem
`tool_program`.** `search`, `architect`, `debug`, `orchestrator`, `review` e
`edit` não ganham `tool_program` em `allowed`, e isso é definitivo, não
pendência. Os motivos:

- Uma whitelist é o contrato explícito do que o modelo pode chamar naquele
  modo. Acrescentar um envelope alarga a superfície que o operador aceitou,
  mesmo com cada passo ainda passando pelo portão.
- Esses fluxos são majoritariamente de leitura e de poucos passos: ganham
  pouco com lote, e um lote torna o turno mais difícil de acompanhar.
- Nos modos que já expõem (`auto`, `code`, `ask`), o gate por passo deixa um
  programa estritamente equivalente às chamadas avulsas, então a exposição
  ali não concede privilégio novo.

Quem quiser o envelope num fluxo de whitelist cria um perfil customizado e
lista `tool_program` em `allowed` (abaixo); `denied` continua vencendo. O
teste `tool_program_exposto_so_nos_perfis_nativos_sem_whitelist` é a guarda
de regressão desta decisão.

**Como liberar ou negar num perfil customizado.** Pelo nome, como qualquer
ferramenta. Num perfil com whitelist, liste `tool_program` em `allowed`; em
qualquer perfil, `denied: ["tool_program"]` a desliga (e vence tudo):

```json
{
  "allowed": ["tool_program", "file_read", "repo_search"],
  "whitelist_mode": true
}
```

Liberar `tool_program` **não** libera nenhuma outra ferramenta. O que vale
dentro do programa é o mesmo que vale fora dele:

- **Gate por passo.** Cada passo passa pelo mesmo `ToolGate` do modo (o
  despacho é recursivo, com um único ponto de consulta ao portão no fonte).
  Um passo negado encerra o programa ali — a resposta traz os passos já
  executados e `parou_no_passo` — e os seguintes não rodam. No `ask`, um
  programa não alcança `bash`, `file_write` nem `device_execute`.
- **Orçamento.** Um programa de N passos custa **1 + N** chamadas contra
  `max_per_turn`/`max_per_task`: o envelope mais uma por passo. Esgotar só o
  teto do turno para o programa com o relatório parcial (o loop segue na
  próxima volta, como seguiria com chamadas avulsas); esgotar a tarefa aborta
  a conversa.
- **Detecção de loop.** Os passos entram na janela de assinaturas; o envelope,
  não. Três passos idênticos em sequência — no mesmo programa ou em programas
  de um passo repetidos volta após volta — abortam a conversa, como três
  chamadas avulsas idênticas.
- **Confirmação humana (GAR-187).** Um passo que pede confirmação pausa o
  programa. O humano lê só o pedido daquele passo, nunca a saída dos passos
  anteriores; o modelo recebe o pedido mais o relatório parcial (passos
  executados, `parou_no_passo` e as variáveis salvas em `vars`). Depois do
  "sim", o modelo reenvia só os passos a partir do pausado, e a aprovação
  cobre só aquele pedido.
- **Eventos.** Cada passo emite o seu `tool_started`/`tool_finished` dentro
  do par do próprio programa; todo início tem o seu fim, inclusive o de um
  passo negado.
- **Variáveis.** `as` guarda a saída de um passo, que tem de ser um número
  inteiro (senão o passo falha); `"$nome"` num passo seguinte vira esse
  número. Um `"$nome"` sem valor faz o passo falhar antes de rodar — nunca
  chega à ferramenta como texto.

**Limites.** No máximo 16 passos (`MAX_PROGRAM_STEPS`); teto agregado de 120 s
(`PROGRAM_AGGREGATE_TIMEOUT_SECS`), checado entre passos e somado ao timeout
por passo; `tool_program` dentro de `tool_program` é recusado.

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

> **Limite conhecido (v0.4.5):** o prompt do modo chega ao provider em todo
> turno, e o `max_tokens` do modo tambem, salvo quando o chamador ou a config
> (`agent.max_tokens`) definem o seu (precedencia chamador > runtime > modo; o
> CLI sempre define 4096). A `temperature` do modo ainda nao chega: os turnos
> de chat, canais e API mandam o pedido sem temperatura, e o provider usa o
> default dele. Manda-la mudaria o pedido de todo turno com modo, porque os
> modos embutidos declaram de 0.3 a 0.7, e essa mudanca fica para quando o
> provider souber omitir o parametro nos modelos que o recusam.

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
