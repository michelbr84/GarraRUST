# Primeiros Passos

## Início Rápido

A forma mais rápida de começar é utilizando o script de instalação:

```bash
# Instalar (Linux, macOS)
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh

# Configuração interativa — escolha seu provedor de LLM e armazene suas chaves de API em um cofre criptografado
garraia init

# Iniciar o GarraIA
garraia start
```

---

## Compilar a partir do código-fonte

Você também pode compilar o GarraIA a partir do código-fonte, caso tenha o Rust instalado (versão 1.85 ou superior).

```bash
cargo build --release
./target/release/garraia init
./target/release/garraia start
```

No Windows:

```powershell
target\release\garraia.exe init
target\release\garraia.exe start
```

---

## Configuração

O GarraIA procura seu arquivo de configuração no seguinte caminho:

```text
~/.garraia/config.yml
```

Exemplo de configuração:

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    # api_key é resolvida automaticamente na seguinte ordem:
    # cofre criptografado > config.yml > variável de ambiente ANTHROPIC_API_KEY

  ollama-local:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"

channels:
  telegram:
    type: telegram
    enabled: true
    bot_token: "seu-bot-token"  # ou variável de ambiente TELEGRAM_BOT_TOKEN

agent:
  system_prompt: "Você é o GarraIA, um assistente de inteligência artificial útil."
  max_tokens: 4096
  max_context_tokens: 100000

memory:
  enabled: true

# Servidores MCP para ferramentas externas
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

---

## Migração a partir do OpenClaw

Se você estiver migrando do OpenClaw, pode utilizar a ferramenta de migração integrada para importar suas habilidades, configurações de canais e credenciais.

```bash
garraia migrate openclaw
```

Opções disponíveis:

```bash
# Visualizar mudanças sem aplicá-las
garraia migrate openclaw --dry-run

# Especificar um diretório personalizado do OpenClaw
garraia migrate openclaw --source /caminho/para/openclaw
