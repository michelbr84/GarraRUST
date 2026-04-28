# Início Rápido

Coloque o GarraIA funcionando em menos de 5 minutos.

---

## Pré-requisitos

| Requisito | Versão mínima | Verificação |
|-----------|---------------|-------------|
| Rust | 1.92 | `rustc --version` |
| Git | qualquer | `git --version` |
| Chave de API LLM | — | Anthropic, OpenAI ou Ollama local |

Para usar modelos locais sem custo, o [Ollama](https://ollama.com) elimina a necessidade de chave de API.

---

## Passo 1 — Instalar

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
```

O script instala o binário `garraia` em `~/.local/bin/` e o adiciona ao PATH automaticamente.

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
```

**Compilar a partir do código-fonte:**

```bash
git clone https://github.com/michelbr84/GarraRUST.git
cd GarraRUST
cargo build --release
# O binário estará em: target/release/garraia (ou garraia.exe no Windows)
```

Verifique a instalação:

```bash
garraia --version
# garraia 0.9.0
```

---

## Passo 2 — Configurar

Execute o assistente de configuração interativo:

```bash
garraia init
```

O assistente vai:

1. Criar o diretório `~/.garraia/`
2. Solicitar a senha do cofre de credenciais (AES-256-GCM)
3. Perguntar qual provedor LLM você deseja usar
4. Armazenar a chave de API de forma segura no cofre

Alternativamente, crie o arquivo de configuração manualmente em `~/.garraia/config.yml`:

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  principal:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    # A chave de API é resolvida automaticamente:
    # cofre criptografado > config.yml > variável de ambiente ANTHROPIC_API_KEY

agent:
  system_prompt: "Você é o GarraIA, um assistente de inteligência artificial útil."
  max_tokens: 4096

memory:
  enabled: true
```

Para usar um modelo local com Ollama (sem custo de API):

```yaml
llm:
  principal:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"
```

---

## Passo 3 — Iniciar o servidor

```bash
garraia start
```

Saída esperada:

```
[INFO] GarraIA v0.9.0 iniciando...
[INFO] Cofre de credenciais desbloqueado
[INFO] Provedor LLM: anthropic (claude-sonnet-4-5-20250929)
[INFO] Servidor HTTP ouvindo em http://127.0.0.1:3888
[INFO] Gateway pronto.
```

---

## Passo 4 — Seu primeiro chat

Envie uma mensagem via API REST:

```bash
curl -s -X POST http://127.0.0.1:3888/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Olá! Quem é você?", "session_id": "teste"}' | jq .
```

Resposta esperada:

```json
{
  "response": "Olá! Eu sou o GarraIA, um assistente de inteligência artificial...",
  "session_id": "teste",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5-20250929"
}
```

Verifique o status do servidor:

```bash
curl http://127.0.0.1:3888/health
# {"status":"ok","version":"0.9.0"}
```

---

## Próximos passos

- **Conectar ao Telegram:** [Guia de configuração do Telegram](./guides/connect-telegram.md)
- **Usar modelo local:** [Conectar LM Studio / Ollama](./guides/add-lm-studio.md)
- **API completa:** [Referência da API REST](./api-reference.md)
- **Segurança:** [Arquitetura de segurança](./security/architecture.md)

---

## Resolução de problemas comuns

**Porta 3888 já em uso:**

```bash
# Altere a porta no config.yml
gateway:
  port: 4000
```

**Erro de autenticação com o provedor LLM:**

```bash
# Verifique as credenciais no cofre
garraia vault list

# Atualize uma chave
garraia vault set ANTHROPIC_API_KEY sk-ant-...
```

**O servidor não inicia (Windows):**

Certifique-se de que o binário tem permissão de execução. Execute o PowerShell como Administrador se necessário.
