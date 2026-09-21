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
- Valide tudo com `garra config check` (exit 0 ok · 2 warnings em `--strict` · 65 config inválida). O relatório aponta a fonte efetiva de cada valor e **nunca imprime secrets** (só `*_set: true`). É um **comando opt-in** (também rodado por `garra doctor`), não um gate de boot: um erro no relatório não impede o gateway de subir (#1247).
- **`agent.sandbox`** (novo na v0.4.3, default `off`): envolve a tool `bash` num backend `docker`/`podman`/`ssh`. Seção ausente reproduz o comportamento anterior; hoje só `bash` é envolvida, e `ssh` é execução remota, não sandbox. Referência: [`docs/security/threat-model.md` §5.13](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md), schema em [`crates/garraia-config/src/sandbox.rs`](https://github.com/michelbr84/GarraRUST/blob/main/crates/garraia-config/src/sandbox.rs), exemplo em [`config.hardened.example.yml`](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml).
- **`hardware.*`** (`mqtt`, `home_assistant`, `serial`, `gpio`, `automations`): nada liga sem a seção — o registro de dispositivos nasce vazio e cada transporte é uma feature de compilação. Referência: [`docs/hardware.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) · [`docs/hardware-skills.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md).
