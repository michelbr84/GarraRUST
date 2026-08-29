# Configuração

Duas fontes: o arquivo **`config.yml`** (`~/.garraia/config.yml`, com hot-reload — editar aplica sem reiniciar) e **variáveis de ambiente** para secrets. Referências canônicas:

- [`docs/configuration.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/configuration.md) — referência completa do `config.yml`
- [`.env.example`](https://github.com/michelbr84/GarraRUST/blob/main/.env.example) — todas as variáveis, comentadas, em 18 seções
- [`docs/auth-config.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/auth-config.md) — **matriz de precedência de auth** (leia antes de configurar JWT)
- [`mcp.json.example`](https://github.com/michelbr84/GarraRUST/blob/main/mcp.json.example) — servidores MCP
- [`.garraignore`](https://github.com/michelbr84/GarraRUST/blob/main/README.pt-BR.md#configura%C3%A7%C3%A3o) — padrões de exclusão de arquivos

## Mapa rápido do `.env.example`

| Quero configurar… | Seção |
|---|---|
| Provedor LLM | LLM Provider API Keys (pelo menos uma) |
| Login/JWT do gateway | Auth (JWT + refresh + mobile) — ver nota abaixo |
| Cofre de credenciais | Vault Encryption (AES-256-GCM) |
| Telegram/Discord/etc. | Channel Tokens |
| Porta/host do gateway | Gateway Configuration |
| Voz (STT/TTS) | Voice |
| Busca na web | Web Search (`BRAVE_API_KEY`) |
| Postgres multi-tenant | Database + Group Workspace (Fase 3) |
| Uploads/S3 | Object Storage (Fase 3.5) |
| Métricas/tracing | Observabilidade |
| Embeddings/RAG | Embeddings + RAG/Memória de longo prazo |

## Notas que evitam dor de cabeça

- **`GARRAIA_JWT_SECRET` é env-only e fail-closed**: sem ele, os endpoints de auth respondem **503** de propósito (nunca há fallback inseguro).
- Precedência de secrets: `GARRAIA_JWT_SECRET` > `GarraIA_VAULT_PASSPHRASE` (grafia mista, deprecated) > `GARRAIA_VAULT_PASSPHRASE` — as duas grafias da passphrase são aceitas em todos os consumidores (issue #824).
- Valide tudo com `garra config check` (exit 0 ok · 2 warnings em `--strict` · 65 config inválida). O relatório aponta a fonte efetiva de cada valor e **nunca imprime secrets** (só `*_set: true`).
