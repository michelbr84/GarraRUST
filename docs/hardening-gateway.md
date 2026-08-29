# Hardening do Gateway — ferramentas de execução com política de segurança

> As ferramentas de execução do GarraIA (`bash`, `file_read`, `file_write`,
> `web_fetch`, `schedule_heartbeat`) **já vêm ativas por padrão** — não há
> flag para "ligá-las". O que existe, e este guia cobre, é a **política em
> volta delas**: quem alcança o gateway, o que exige confirmação, o que é
> bloqueado sempre, e como adicionar novas capacidades (via MCP) sem abrir
> a máquina.

Complementa (não substitui): [docs/security.md](security.md),
[checklist de segurança](src/security/checklist.md) e
[.env.example](../.env.example).

## 1. Os dois perfis de exposição

### Perfil A — loopback (default, recomendado)

```yaml
gateway:
  host: "127.0.0.1"   # default do binário — só a própria máquina alcança
  port: 3888
```

Com bind em loopback, a superfície de rede é zero para terceiros. Os
canais de mensageria (Telegram/Discord/…) continuam funcionando — eles
fazem *egress* para as APIs dos provedores; ninguém precisa alcançar a
sua porta 3888. **Se você não tem um motivo concreto para expor o
gateway, este perfil encerra o assunto.**

Atenção: a env var `HOST` sobrescreve o bind (runtimes de container
costumam setar `HOST=0.0.0.0`). Se o seu gateway apareceu em `0.0.0.0`
sem você pedir, procure por essa variável no ambiente.

### Perfil B — exposto na rede (`0.0.0.0`) — requisitos mínimos

Um gateway exposto sem autenticação é **execução remota de comandos
aberta**: qualquer um que alcance a porta usa o tool `bash`. Se precisa
expor, TODOS os itens abaixo são obrigatórios, não opcionais:

```yaml
gateway:
  host: "0.0.0.0"
  port: 3888
  # Valor LITERAL no arquivo (não há interpolação de env no config —
  # gere com `openssl rand -hex 32` e proteja o arquivo com chmod 0600):
  api_key: "<token-forte-aqui>"
  session_tokens_required: true   # exige token de sessão nas rotas /api/*
  session_ttl_secs: 86400         # validade do token (1 dia)
  session_idle_secs: 3600         # corte por inatividade (1h)
  allowed_origins:                # vazio = allow-all; liste explicitamente
    - "https://seu-dominio.exemplo"
  rate_limit:
    per_second: 1
    burst_size: 60
```

- **TLS**: os binários release atuais **não** incluem a feature `tls`
  (compilável com `--features tls` a partir do código). O caminho
  suportado hoje é um **reverse proxy com TLS** (Caddy/nginx/Traefik) na
  frente do gateway em loopback — ou seja, muitas vezes o Perfil A + um
  proxy exposto resolve melhor que `0.0.0.0` direto.
- **Métricas**: mantenha `GARRAIA_METRICS_BIND=127.0.0.1:9464` e use
  `GARRAIA_METRICS_TOKEN`/`GARRAIA_METRICS_ALLOW` se precisar raspar de
  fora (ver `.env.example`).
- Verifique com `garra config check` após editar — ele valida o schema e
  reporta a precedência efetiva.

## 2. Confirmação humana para comandos arriscados

O tool `bash` tem **dois tiers** de proteção
(`crates/garraia-common/src/safety_gate.rs`):

| Tier | Config | Exemplos | Comportamento |
|---|---|---|---|
| **DENY_LIST** (perigoso) | sempre ativo, sem opt-out | `rm -rf /`, fork bomb, `mkfs`, `dd if=`, `shutdown`, `git push --force origin main`, `curl … \| sh` | **Bloqueado sempre**, mesmo com confirmação desligada |
| **CONFIRM_LIST** (arriscado) | `agent.tool_confirmation_enabled: true` | `rm -r`, `git reset --hard`, `drop table`, `truncate`, `kill`, `taskkill` | Pausa e pede "sim"/"yes" **apenas se a flag estiver ligada** |

O default é `false` — o hard-block continua valendo, mas comandos
"arriscados" executam sem perguntar. Para um agente com `bash` em
máquina pessoal, ligue:

```yaml
agent:
  tool_confirmation_enabled: true
  max_tool_calls: 50   # teto de chamadas de tool por tarefa
```

## 3. Restringir tools por contexto: modos (ToolPolicy)

Cada modo de execução carrega uma allow/deny-list de tools
(`crates/garraia-runtime/src/mode.rs`) — este é o mecanismo legítimo
para "menos poder por padrão":

| Modo | Tools permitidas |
|---|---|
| `ask` (default nos messengers) | nenhuma — só conversa |
| `search` | `file_read`, `repo_search` (bash read-only) |
| `review` | `file_read`, `git_diff` |
| `edit` | `file_read`, `file_write` (sem bash) |
| `code` | `file_read`, `file_write`, `bash` |
| `debug` | `file_read`, `bash`, `repo_search` (sem write) |
| `auto` / `orchestrator` | sem restrição de lista |

Precedência: header `X-Agent-Mode` > comando `/mode` > default do canal
(Telegram/Discord/WhatsApp → `ask`; web/API → `auto`) > `ask`. Na
prática: **nos canais de chat o agente nasce sem tools** e só ganha
poder quando alguém pede um modo explicitamente — mantenha assim; evite
fixar `code`/`auto` como modo permanente em canal público.

## 4. Quem pode falar com o agente: allowlist + pareamento

Usuários desconhecidos nos canais precisam de código de pareamento
(deny-by-default). O arquivo é **único e global** —
`<config_dir>/allowlist.json` (ex.: `~/.garraia/allowlist.json` ou
`~/.config/garraia/allowlist.json`) — e vale para **todos** os canais:
um usuário aprovado no Telegram também está aprovado no Discord. O
primeiro usuário a parear vira `owner`. Revise a lista periodicamente
(`/users` no chat).

## 5. Secrets: cofre para MCP e chaves de provider

- Exporte `GARRAIA_VAULT_PASSPHRASE` no ambiente do gateway. Com ela,
  env vars sensíveis dos servidores MCP (`key`, `token`, `secret`,
  `password`, `auth`, `credential`, `pass` no nome) são movidas para o
  cofre AES-256-GCM no primeiro save e o `mcp.json` guarda apenas
  referências `vault:mcp.<server>.<KEY>`. **Sem a passphrase, caem em
  plaintext com warning.**
- O default do wizard `garra init` para chaves de provider é
  `config.yml` (modo 0600). Escolha a opção do cofre no wizard e
  mantenha a passphrase exportada a cada start para ter criptografia em
  repouso.

## 6. Adicionando capacidades via MCP — com política

Novas capacidades (navegar em browser real, banco de dados, filesystem
com escopo) entram como **servidores MCP**, cada um atrás da mesma
política. Receitas prontas e regras por servidor (`allowed_tools`,
`memory_limit_mb`, secrets via vault) em
[docs/integrations/mcp-capacidades.md](integrations/mcp-capacidades.md).

Regra de ouro: **não** adicione um segundo shell via MCP — o `bash`
nativo já existe e passa pelo safety_gate; um shell MCP contornaria os
dois tiers.

## 7. Config de referência endurecido

Arquivo completo comentado, validado com `garra config check`:
[`config.hardened.example.yml`](../config.hardened.example.yml).

## Limitações conhecidas (honestas)

1. `gateway.api_key` só aceita valor literal no arquivo — não há
   interpolação de env no config (a linha `GARRAIA_API_KEY` do
   `.env.example` é aspiracional; suportá-la de verdade é follow-up).
2. Auth do gateway local é **opt-in** hoje (`api_key`/
   `session_tokens_required` desligados por default) — endurecer esse
   default está no roadmap.
3. "Controle de aplicativos" (GUI/automação de apps) não existe como
   tool — ver a nota em mcp-capacidades.md.
4. TLS embutido exige build com `--features tls`; binários release
   atuais servem HTTP puro (use reverse proxy).
