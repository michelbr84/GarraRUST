# VS Code — Configurar com GarraIA (OpenAI-compatible)

## Extensão recomendada: Continue.dev

[Continue](https://marketplace.visualstudio.com/items?itemName=Continue.continue) é a extensão mais completa que suporta **baseUrl customizado** + **apiKey customizado**, essencial para conectar ao GarraIA local.

## Pré-requisito: GarraIA rodando

Certifique-se de que o servidor está no ar:

```bash
cargo run -p garraia-gateway
# ou via Docker:
docker compose up
```

Por padrão ele escuta em `http://localhost:3888`.

## Instalação

1. Abra o VS Code
2. Instale a extensão **Continue** (ID: `Continue.continue`)
3. Clique no ícone do Continue na barra lateral → **"Open Config"** (ou edite `~/.continue/config.json`)
4. Substitua o conteúdo pelo arquivo [`continue-config.json`](./continue-config.json) deste diretório, ajustando:
   - `apiKey`: preencha com o valor de `GARRAIA_API_KEY` do seu `.env` (ou deixe vazio se sem autenticação)
   - `model`: o nome do modelo é passado ao gateway e pode acionar um provider específico via prefixo (ex: `openrouter/anthropic/claude-3.5-sonnet`, `anthropic/claude-sonnet-4-6`)
   - `apiBase`: `http://localhost:3888` para local, ou `https://seu-domínio.com` em produção

## Exemplo de config.json

```json
{
  "models": [
    {
      "title": "GarraIA",
      "provider": "openai",
      "model": "gpt-4o",
      "apiBase": "http://localhost:3888",
      "apiKey": ""
    }
  ],
  "allowAnonymousTelemetry": false
}
```

## Como a sessão funciona

- Cada conversa no Continue gera um `session_id` baseado no `user` field (ou UUID aleatório)
- O histórico é persistido no SQLite do GarraIA (via GAR-204)
- **A mesma sessão continua** se você reabrir o VS Code — o backend usa o DB como fonte de verdade

## Alternativas ao Continue

| Extensão | Suporte a baseUrl custom | Observação |
|----------|--------------------------|------------|
| **Continue** | ✅ | Recomendado |
| **CodeGPT** | ✅ | Interface mais simples |
| **Aider (via terminal)** | ✅ | `aider --openai-api-base http://localhost:3888` |
| **GitHub Copilot** | ❌ | Não aceita provider customizado |

## Verificação

Para confirmar que o gateway está respondendo:

```bash
curl http://localhost:3888/v1/models
# deve retornar JSON com a lista de modelos configurados
```

```bash
curl -X POST http://localhost:3888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Olá!"}],"stream":false}'
```
