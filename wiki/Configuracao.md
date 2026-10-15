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
- Valide tudo com `garraia config check` (exit 0 ok · 2 warnings em `--strict` · 65 config inválida). O relatório aponta a fonte efetiva de cada valor e **nunca imprime secrets** (só `*_set: true`). Desde a v0.4.5 o mesmo check roda em todo `garraia start`/`restart`/`start -d` (#1247): os achados vão para o log, e só uma lista fechada recusa o boot com exit 78 (hoje, o TLS configurado pela metade); escotilha `GARRAIA_ALLOW_INVALID_CONFIG=1` (exatamente `1`). Um bind exposto sem `gateway.api_key` nem `GARRAIA_GATEWAY_API_KEY` também recusa o boot (#1261).
- **`agent.sandbox`** (novo na v0.4.3, default `off`): envolve as tools que executam processo num backend `docker`/`podman`/`ssh`. Seção ausente reproduz o comportamento anterior. Desde a v0.4.5, com `mode: all` entram `bash`, `run_tests`, `git_diff`, `code_review` e `repo_search` (#1225): a imagem precisa da toolchain (`git`/`cargo`/`rg`) ou a tool vai em `elevated`. `ssh` é execução remota, não sandbox, e é recusado para as tools de diretório. Em `execution.profile: standard`, o `garraia mcp-server` e o gateway registram `bash` com um sandbox `docker`/`podman` válido ou, desde a v0.4.8, em modo allowlist-only no host via `agent.bash_allowlist` (`HostComAllowlist`, #1272). Referência: [`docs/security/threat-model.md` §5.13](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md), schema em [`crates/garraia-config/src/sandbox.rs`](https://github.com/michelbr84/GarraRUST/blob/main/crates/garraia-config/src/sandbox.rs), exemplo em [`config.hardened.example.yml`](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml).
- **`execution.profile`** (novo na v0.4.4, ADR 0024, default `standard`): `isolated-pod` declara que o processo roda num pod **descartável** — poder total dentro do pod (dono do WhatsApp em 1:1 ganha o piso `code`; MCP `filesystem` na raiz `execution.pod_root`, senão `<data_dir>/workspace`, nunca `$HOME`), nada implícito fora. A env **`GARRAIA_EXECUTION_PROFILE`** vence o arquivo e nunca é gravada nele; valor inválido recusa o boot e sai como `Error` no `config check`; seção ausente = comportamento de hoje. Nunca é inferido de `/.dockerenv` ou cgroups. `pod_root` precisa ser absoluto e só vale em `isolated-pod` (`Warning` fora dele). Referência: [`docs/execution-profiles.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/execution-profiles.md) · [`docs/configuration.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/configuration.md).
- **`channels.whatsapp_linked.access`** (novo na v0.4.6, ADR 0025): quem fala com o agente e **até onde** cada um vai, por principal — não por lista única.

  ```yaml
  access:
    admission: restricted                    # restricted | open
    default: { level: chat, write: false }   # o DESCONHECIDO, so com `open`
    users:
      "+5511999998888": { level: read, write: false }
      "+5511977776666": { role: owner }
      "+5511955554444": { blocked: true }
    groups:
      enabled: false
      default: { level: read, write: false }
      "120363@g.us": { level: chat }
  ```

  Três regras que a seção **não** escolhe. **(1) Compatibilidade:** sem `access:`, o comportamento é o da v0.4.5 — `allow` segue valendo (usuário *sem teto*: o piso de modo decide), `owners` segue valendo (dono) e `reply_in_groups` segue ligando os grupos. Nível e `write` só existem onde foram declarados; `access.users` vence o legado para a mesma identidade, e `access.groups.enabled` declarado vence o `reply_in_groups` (#1501). Quem entrou por código (`/pair`) é a exceção: credencial fraca, teto `read`. **(2) Fail-closed:** nível desconhecido é `chat`; `chat` com `write: true` é `chat`; `admission` desconhecida é `restricted`; o `default` de `open` nunca chega a `full` (#1390). Cada normalização deixa um aviso — sem número — que o boot e o `config check` mostram. **(3) Nível é teto, não piso:** o valor compõe por E com o modo da sessão — **só tira, nunca põe**. Edite pela CLI (`garraia whatsapp access|level|write|block|unblock|owner|unowner|remove`) ou pela página *WhatsApp Access* do console, que usam o mesmo motor e deixam rastro no audit; grupos valem na mensagem seguinte, sem restart (#1412). Referência: [ADR 0025](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0025-classes-de-capacidade-e-politica-de-acesso-v2.md) · [`docs/whatsapp.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md).
- **Retenção de memória e do ledger de runs** (fatia de dados na v0.4.6, #1436): política em `garraia-config::retention` (`memory.enabled`/`max_age_days`/`interval_hours` e `run_ledger.retention_days`), aplicada por `garraia-db::retention` e coberta pelo `config check`. A página do console e o `PATCH /admin/api/retention` ficam para o ciclo seguinte.
- **`hardware.*`** (`mqtt`, `home_assistant`, `automations`): nada liga sem a seção — o registro de dispositivos nasce vazio (fail-closed). Os adapters Serial/USB e GPIO existem na crate atrás das features `hardware-serial`/`hardware-gpio`, mas ainda sem chave de config nem wiring no boot do gateway (#1130). Referência: [`docs/hardware.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) · [`docs/hardware-skills.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md).
