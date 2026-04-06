# Conectar LM Studio / Ollama (modelos locais)

Execute modelos de linguagem localmente, sem custo de API e sem enviar dados para serviços externos.

---

## Visão geral

O GarraIA suporta dois backends de modelos locais:

| Backend | Caso de uso | Compatibilidade |
|---------|-------------|-----------------|
| **Ollama** | Modelos open-source simples de usar (Llama, Mistral, Gemma) | Linux, macOS, Windows |
| **LM Studio** | Interface gráfica + servidor local compatível com OpenAI | Linux, macOS, Windows |

Ambos expõem uma API compatível com OpenAI em `http://localhost:PORT/v1`, o que permite configurá-los de forma idêntica no GarraIA.

---

## Opção A — Ollama

### 1. Instalar o Ollama

**Linux / macOS:**

```bash
curl -fsSL https://ollama.com/install.sh | sh
```

**Windows:** Baixe o instalador em [https://ollama.com/download](https://ollama.com/download)

### 2. Baixar um modelo

```bash
# Llama 3.1 8B — bom equilíbrio entre qualidade e velocidade
ollama pull llama3.1

# Mistral 7B — ótimo para português
ollama pull mistral

# Gemma 3 — compacto, roda em hardware modesto
ollama pull gemma3

# Listar modelos instalados
ollama list
```

### 3. Iniciar o servidor Ollama

O servidor inicia automaticamente ao executar `ollama pull` ou pode ser iniciado explicitamente:

```bash
ollama serve
# Servidor ouvindo em http://localhost:11434
```

### 4. Configurar no GarraIA

Abra `~/.garraia/config.yml`:

```yaml
llm:
  local:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"
```

Para usar múltiplos modelos locais simultaneamente:

```yaml
llm:
  llama:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"

  mistral:
    provider: ollama
    model: mistral
    base_url: "http://localhost:11434"
```

### 5. Verificar a conexão

```bash
curl http://127.0.0.1:3888/api/chat \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"message": "Olá!", "session_id": "teste-local"}' | jq .response
```

---

## Opção B — LM Studio

### 1. Instalar o LM Studio

Baixe em [https://lmstudio.ai](https://lmstudio.ai) e instale normalmente.

### 2. Baixar um modelo no LM Studio

1. Abra o LM Studio
2. Vá em **Discover** (aba de busca)
3. Pesquise por `llama`, `mistral` ou `qwen`
4. Clique em **Download** no modelo desejado

Recomendações para hardware comum:

| RAM disponível | Modelo recomendado |
|----------------|--------------------|
| 8 GB | Llama 3.2 3B Q4 |
| 16 GB | Mistral 7B Q4 |
| 32 GB | Llama 3.1 13B Q4 |
| 64 GB+ | Llama 3.1 70B Q4 |

### 3. Ativar o servidor local

1. No LM Studio, vá em **Local Server** (ícone `<->` na barra lateral)
2. Selecione o modelo que deseja servir
3. Clique em **Start Server**
4. O servidor ficará disponível em `http://localhost:1234`

### 4. Configurar no GarraIA

```yaml
llm:
  lmstudio:
    provider: openai          # LM Studio é compatível com a API OpenAI
    model: local-model        # O nome exato aparece no LM Studio
    base_url: "http://localhost:1234/v1"
    api_key: "lm-studio"      # Qualquer valor; LM Studio não valida a chave
```

---

## Trocar o modelo em tempo real

Sem reiniciar o servidor, use a API de override de modelo:

```bash
# Listar modelos disponíveis
curl http://127.0.0.1:3888/api/providers | jq .

# Trocar para um modelo específico nesta sessão
curl -X POST http://127.0.0.1:3888/api/model/override \
  -H "Content-Type: application/json" \
  -d '{"provider": "ollama", "model": "mistral"}'
```

---

## Dicas de desempenho

**Habilitar aceleração por GPU:**

Para Ollama com NVIDIA:
```bash
# Verifique se a GPU está sendo utilizada
ollama run llama3.1 "teste"
# A saída deve mostrar: "using GPU acceleration"
```

Para LM Studio: ative **GPU Offload** nas configurações do modelo antes de iniciar o servidor.

**Configurar o limite de tokens de contexto:**

Modelos locais costumam ter janela de contexto menor. Ajuste no `config.yml`:

```yaml
agent:
  max_context_tokens: 8192   # Reduza se o modelo não suportar mais
  max_tokens: 2048
```

---

## Resolução de problemas

**Erro "connection refused" ao usar Ollama:**

```bash
# Verifique se o Ollama está rodando
curl http://localhost:11434/api/tags
# Se falhar, inicie manualmente:
ollama serve &
```

**Respostas muito lentas:**

- Verifique se a GPU está sendo utilizada (para Ollama: `ollama ps`)
- Reduza `max_tokens` no config
- Use um modelo menor (ex: `gemma3` em vez de `llama3.1`)

**Modelo não encontrado:**

```bash
# Liste os modelos instalados no Ollama
ollama list

# Baixe o modelo necessário
ollama pull NOME_DO_MODELO
```
