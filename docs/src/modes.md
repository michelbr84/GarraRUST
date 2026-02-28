# Modos de Execução

Este documento define o contrato de "Modo" para o GarraIA, estabelecendo como estratégias de execução são selecionadas e aplicadas.

## Definição

**Modo = Estratégia de Execução**

Um modo no GarraIA não é uma "personalidade" do agente, mas sim uma estratégia que determina:
- Quais tools estão disponíveis
- Quais prompts de sistema são usados
- Quais parâmetros de LLM (temperatura, max_tokens, etc.)
- Quais limites (max loops, timeouts)

## Modos Disponíveis

| Modo | Descrição | Tools Permitidas |
|------|-----------|------------------|
| `auto` | Decide automaticamente baseado no contexto | Depende do modo resolvido |
| `search` | Busca e inspeção sem modificar | file_read, repo_search, bash (read-only) |
| `architect` | Análise de arquitetura e design | file_read, repo_search |
| `code` | Desenvolvimento e implementação | file_read, file_write, bash |
| `ask` | Consulta e explicação | Todas (preferência textual) |
| `debug` | Debugging e análise de erros | file_read, bash, repo_search |
| `orchestrator` | Execução multi-etapas com planos | Todas |
| `review` | Revisão de código e diffs | file_read, git_diff |
| `edit` | Edição focada | file_read, file_write |

## Precedência de Modo

O modo é resuelto seguindo esta ordem de precedência (maior para menor):

1. **Header HTTP**: `X-Agent-Mode` (ex: no endpoint OpenAI)
2. **Comando do chat**: `/mode <nome>` (ex: `/mode code`)
3. **Preferência por canal**: Configuração específica por canal (Telegram, Web, etc.)
4. **Preferência por usuário**: Configuração por user_id
5. **Default**: O padrão do sistema (`ask` para Telegram, `auto` para API)

### Exemplos de Precedência

```bash
# Header tem maior precedência
curl -X POST http://localhost:3888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Agent-Mode: debug" \
  -d '{"messages": [{"role": "user", "content": "Meu código está dando panic"}]}'

# Comando no chat
/mode code
/refatore essa função

# Default por canal
# Telegram: ask (padrão - não quebra comportamento atual)
# Web/Continue: auto (inteligente)
```

## Implementação

### Enum AgentMode

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Auto,
    Search,
    Architect,
    Code,
    Ask,
    Debug,
    Orchestrator,
    Review,
    Edit,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::Ask // Padrão seguro para Telegram
    }
}
```

### Struct ModeProfile

```rust
#[derive(Debug, Clone)]
pub struct ModeProfile {
    pub name: AgentMode,
    pub description: String,
    pub system_prompt_template: String,
    pub tool_policy: ToolPolicy,
    pub llm_defaults: LlmDefaults,
    pub limits: ModeLimits,
}

pub struct ToolPolicy {
    pub allowed: Vec<ToolName>,
    pub denied: Vec<ToolName>,
    pub read_only: Vec<ToolName>,
    pub required: Option<ToolName>,
}

pub struct LlmDefaults {
    pub temperature: f32,
    pub max_tokens: u32,
    pub top_p: f32,
}

pub struct ModeLimits {
    pub max_loops: u32,
    pub timeout_secs: u64,
}
```

## Canal Padrão

Para **não quebrar o Telegram** (canal principal atual):

- **Telegram**: Default = `ask` (mantém comportamento atual)
- **OpenAI API**: Default = `auto` (inteligente)
- **Web UI**: Default = `auto` (inteligente)
- **Continue/VS Code**: Default = `auto` (inteligente)

## API Interna

```rust
// Obter perfil de modo
pub fn get_mode_profile(mode: AgentMode) -> &'static ModeProfile;

// Listar todos os modos disponíveis
pub fn list_modes() -> Vec<(&'static str, &'static str)>;

// Resolver modo (com precedência)
pub fn resolve_mode(
    header: Option<&str>,
    command: Option<&str>,
    channel: &str,
    user_id: &str,
) -> AgentMode;
```

## Histórico

- **v1.0** (2026-02): Contrato inicial com 9 modos
