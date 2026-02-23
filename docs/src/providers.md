# Provedores (Providers)

O GarraIA suporta **14 provedores de modelos LLM**. Três são implementações nativas com APIs específicas. Os restantes utilizam o formato compatível com a API de chat completions da OpenAI e são construídos sobre o `OpenAiProvider`, utilizando URLs base personalizadas.

Todos os provedores suportam:

* Respostas em streaming
* Uso de ferramentas (tools)
* Execução em tempo real

---

## Resolução da chave de API

Para cada provedor, as chaves de API são resolvidas na seguinte ordem de prioridade:

1. **Vault de credenciais**
   `~/.garraia/credentials/vault.json`
   (requer `GARRAIA_VAULT_PASSPHRASE`)

2. **Arquivo de configuração**
   Campo `api_key` dentro da seção `llm:` no `config.yml`

3. **Variável de ambiente**
   Variável específica do provedor (listada abaixo)

---

# Provedores nativos

## Anthropic Claude

Modelos Claude com streaming nativo (SSE) e suporte a ferramentas via API Anthropic Messages.

| Campo                | Valor                        |
| -------------------- | ---------------------------- |
| Tipo                 | `anthropic`                  |
| Modelo padrão        | `claude-sonnet-4-5-20250929` |
| URL base             | `https://api.anthropic.com`  |
| Variável de ambiente | `ANTHROPIC_API_KEY`          |

Exemplo:

```yaml
llm:
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
```

---

## OpenAI

Modelos GPT via API Chat Completions da OpenAI.

| Campo                | Valor                    |
| -------------------- | ------------------------ |
| Tipo                 | `openai`                 |
| Modelo padrão        | `gpt-4o`                 |
| URL base             | `https://api.openai.com` |
| Variável de ambiente | `OPENAI_API_KEY`         |

Exemplo:

```yaml
llm:
  gpt:
    provider: openai
    model: gpt-4o
```

---

## Ollama (local)

Executa modelos localmente. Não requer chave de API.

| Campo                | Valor                    |
| -------------------- | ------------------------ |
| Tipo                 | `ollama`                 |
| Modelo padrão        | `llama3.1`               |
| URL base             | `http://localhost:11434` |
| Variável de ambiente | Nenhuma                  |

Exemplo:

```yaml
llm:
  local:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"
```

---

# Provedores compatíveis com OpenAI

Esses provedores utilizam o formato compatível com OpenAI:

```text
Authorization: Bearer <API_KEY>
```

---

## OpenRouter

O OpenRouter permite acessar múltiplos provedores e modelos através de uma única API compatível com OpenAI.

Permite alternar facilmente entre:

* Claude
* GPT-4
* DeepSeek
* Llama
* Mistral
* e muitos outros

| Campo                | Valor                          |
| -------------------- | ------------------------------ |
| Tipo                 | `openrouter`                   |
| Modelo padrão        | `openai/gpt-4o`                |
| URL base             | `https://openrouter.ai/api/v1` |
| Variável de ambiente | `OPENROUTER_API_KEY`           |

Exemplo:

```yaml
llm:
  openrouter:
    provider: openrouter
    model: openai/gpt-4o
    base_url: "https://openrouter.ai/api/v1"
```

Exemplos de modelos:

```yaml
llm:
  claude:
    provider: openrouter
    model: anthropic/claude-3.5-sonnet

  deepseek:
    provider: openrouter
    model: deepseek/deepseek-chat

  llama:
    provider: openrouter
    model: meta-llama/llama-3.1-70b-instruct
```

Site oficial:

[https://openrouter.ai](https://openrouter.ai)

---

## Sansa

| Campo         | Valor                     |
| ------------- | ------------------------- |
| Tipo          | `sansa`                   |
| Modelo padrão | `sansa-auto`              |
| URL base      | `https://api.sansaml.com` |
| Variável      | `SANSA_API_KEY`           |

```yaml
llm:
  sansa:
    provider: sansa
    model: sansa-auto
```

---

## DeepSeek

| Campo         | Valor                      |
| ------------- | -------------------------- |
| Tipo          | `deepseek`                 |
| Modelo padrão | `deepseek-chat`            |
| URL base      | `https://api.deepseek.com` |
| Variável      | `DEEPSEEK_API_KEY`         |

```yaml
llm:
  deepseek:
    provider: deepseek
    model: deepseek-chat
```

---

## Mistral

| Campo         | Valor                    |
| ------------- | ------------------------ |
| Tipo          | `mistral`                |
| Modelo padrão | `mistral-large-latest`   |
| URL base      | `https://api.mistral.ai` |
| Variável      | `MISTRAL_API_KEY`        |

```yaml
llm:
  mistral:
    provider: mistral
    model: mistral-large-latest
```

---

## Gemini

| Campo         | Valor                                                      |
| ------------- | ---------------------------------------------------------- |
| Tipo          | `gemini`                                                   |
| Modelo padrão | `gemini-2.5-flash`                                         |
| URL base      | `https://generativelanguage.googleapis.com/v1beta/openai/` |
| Variável      | `GEMINI_API_KEY`                                           |

```yaml
llm:
  gemini:
    provider: gemini
    model: gemini-2.5-flash
```

---

## Falcon

| Campo         | Valor                     |
| ------------- | ------------------------- |
| Tipo          | `falcon`                  |
| Modelo padrão | `tiiuae/falcon-180b-chat` |
| URL base      | `https://api.ai71.ai/v1`  |
| Variável      | `FALCON_API_KEY`          |

```yaml
llm:
  falcon:
    provider: falcon
    model: tiiuae/falcon-180b-chat
```

---

## Jais

| Campo         | Valor                      |
| ------------- | -------------------------- |
| Tipo          | `jais`                     |
| Modelo padrão | `jais-adapted-70b-chat`    |
| URL base      | `https://api.core42.ai/v1` |
| Variável      | `JAIS_API_KEY`             |

```yaml
llm:
  jais:
    provider: jais
    model: jais-adapted-70b-chat
```

---

## Qwen

| Campo         | Valor                                                    |
| ------------- | -------------------------------------------------------- |
| Tipo          | `qwen`                                                   |
| Modelo padrão | `qwen-plus`                                              |
| URL base      | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` |
| Variável      | `QWEN_API_KEY`                                           |

```yaml
llm:
  qwen:
    provider: qwen
    model: qwen-plus
```

---

## Yi

| Campo         | Valor                            |
| ------------- | -------------------------------- |
| Tipo          | `yi`                             |
| Modelo padrão | `yi-large`                       |
| URL base      | `https://api.lingyiwanwu.com/v1` |
| Variável      | `YI_API_KEY`                     |

```yaml
llm:
  yi:
    provider: yi
    model: yi-large
```

---

## Cohere

| Campo         | Valor                                     |
| ------------- | ----------------------------------------- |
| Tipo          | `cohere`                                  |
| Modelo padrão | `command-r-plus`                          |
| URL base      | `https://api.cohere.com/compatibility/v1` |
| Variável      | `COHERE_API_KEY`                          |

```yaml
llm:
  cohere:
    provider: cohere
    model: command-r-plus
```

---

## MiniMax

| Campo         | Valor                          |
| ------------- | ------------------------------ |
| Tipo          | `minimax`                      |
| Modelo padrão | `MiniMax-Text-01`              |
| URL base      | `https://api.minimaxi.chat/v1` |
| Variável      | `MINIMAX_API_KEY`              |

```yaml
llm:
  minimax:
    provider: minimax
    model: MiniMax-Text-01
```

---

## Moonshot

| Campo         | Valor                        |
| ------------- | ---------------------------- |
| Tipo          | `moonshot`                   |
| Modelo padrão | `kimi-k2-0711-preview`       |
| URL base      | `https://api.moonshot.cn/v1` |
| Variável      | `MOONSHOT_API_KEY`           |

```yaml
llm:
  moonshot:
    provider: moonshot
    model: kimi-k2-0711-preview
```

---

# Troca de provedor em tempo de execução

Listar provedores:

```bash
curl http://127.0.0.1:3888/api/providers
```

Adicionar provedor:

```bash
curl -X POST http://127.0.0.1:3888/api/providers \
  -H "Content-Type: application/json" \
  -d '{"provider":"openrouter","api_key":"sk-..."}'
```

---

# Múltiplas instâncias

```yaml
llm:
  claude:
    provider: openrouter
    model: anthropic/claude-3.5-sonnet

  gpt4:
    provider: openrouter
    model: openai/gpt-4o

  deepseek:
    provider: openrouter
    model: deepseek/deepseek-chat
```