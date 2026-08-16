# Plan 0351 — onboarding do `install.sh` entrega provider ativo

**Status:** Entregue (PR #823)
**Autor:** Claude Opus 5 (sessão interativa 2026-08-16, America/New_York)
**Data:** 2026-08-16 (America/New_York)
**Branch:** `fix/gar-onboarding-provider-key-resolution`
**Epic:** `epic:cli` / onboarding
**Release:** `v0.3.0`

---

## §1 Sintoma reportado

Usuário instalou pelo comando do site (https://garraia.org/) num pod RunPod
limpo, configurou o provider OpenRouter no wizard, e não conseguiu conversar com
o Garra. Log:

```text
WARN garraia_gateway::bootstrap: skipping openrouter provider main: no API key
     (set api_key in config or OPENROUTER_API_KEY env var)
...
║  Total: 0 active / 1 configured     ║
║  ❌ main             no API key configured ║
```

## §2 Causa raiz

O caminho documentado pelo site falha **por construção** em qualquer máquina
nova. Não é config errada do operador.

1. `install.sh:258` roda `garraia init </dev/tty`.
2. O wizard (`wizard/mod.rs`, e o `wizard.rs` do v0.2.1) oferece três opções de
   storage com **default no índice 0, "Store in encrypted vault (recommended)"**.
3. `vault.set("OPENROUTER_API_KEY", &api_key)` cifra a chave em
   `credentials/vault.json` e deixa `llm_config.api_key = None` → o config sai
   com `llm.main.api_key: null`. (O v0.2.1 escreve a chave de mapa literal
   `main`, o que explica o nome no log.)
4. O wizard imprime `Set GARRAIA_VAULT_PASSPHRASE env var for server mode.` —
   um `println!` solto que sobe na tela e desaparece.
5. `install.sh:270` faz `exec garraia start` **no mesmo shell, sem exportar a
   passphrase**.
6. `resolve_api_key` percorre vault → config → env:
   - vault ✗ — `try_vault_get` (`garraia-security/src/credentials.rs:189`) exige
     `GARRAIA_VAULT_PASSPHRASE` e devolve `None`;
   - config ✗ — `api_key` é `null`;
   - env ✗ — nada exportado.
7. Provider pulado → gateway sobe sem nenhum provider → impossível conversar.

**O cofre é um buraco negro write-only para uso headless.** A chave está no
disco, cifrada, e o servidor não tem como abri-la. No v0.2.1 nem o
`warn_if_vault_locked` (plan 0250 / GAR-771) existia, então o operador só via o
"no API key" enigmático.

### §2.1 Segundo caminho com o mesmo sintoma

`POST /api/providers` (página Providers do console web) devolve
`201 {"status":"ok"}`, registra o provider em memória, e chama `try_vault_set`,
que retorna `false` **sem log** quando falta a passphrase
(`credentials.rs:202-206`). O `bool` era descartado em `router.rs:950`. O
provider funciona pelo resto da vida do processo e desaparece no restart.

### §2.2 Três regras divergentes de "esse provider tem chave?"

| Superfície | Regra | Consequência |
| --- | --- | --- |
| `bootstrap` (a verdade) | vault → config → env | decide se o provider sobe |
| `/health` | config `\|\|` env | reportava "no API key" para provider carregado do cofre |
| `admin/providers` | store SQLite do admin | `has_secret: true` para chave que o boot nunca lê |

## §3 Decisões

| Decisão | Escolha | Razão |
| --- | --- | --- |
| Storage default | `config.yml` com `0600` | Cofre com passphrase guardada ao lado do ciphertext não é mais seguro que um `0600`; e `gateway.api_key` já morava em texto no config. Cofre segue disponível para deploys que injetam a passphrase externamente. |
| Diretório de config | Mantém XDG | Trocar o default é migração breaking; `GARRAIA_CONFIG_DIR` já é a alavanca de consolidação. O que havia de real era drift de docs. |
| Release | `v0.3.0` no repo público | `install.sh` resolve `/releases/latest` em `michelbr84/GarraRUST`; sem tag nova o curl do site segue entregando o v0.2.1. |

## §4 Mudanças entregues

- **P0** wizard grava no `config.yml` por default; rótulo do cofre declara que
  exige a passphrase a cada start; aviso vira bloco destacado. Mesmo defeito e
  mesma correção para o token do Telegram.
- **P0** `merge_update` deixa de ser aditivo-only — backfilla chave nova em
  entradas `openrouter` pré-existentes sem chave, para que re-rodar `init`
  conserte um config quebrado. Nunca sobrescreve chave já definida; não aplica ao
  Ollama (que registra como `provider: openai` com placeholder).
- **P0** `garraia_config::harden_secret_file` aplica `0600` no `save` e nas três
  estratégias de escrita do wizard.
- **P1** `garraia_config::provider_keys` — fonte única (`provider_key_env`,
  `KeySource`, `resolve_api_key_source`). Remove 15 pares hardcoded em
  `build_agent_runtime` e 14 no handler de ativação; `/health` e
  `admin/providers` passam a consumir. `garraia-config` ganha dependência de
  `garraia-security` (sem ciclo: ambos dependem só de `garraia-common`).
- **P1** `config check` valida `llm:` — Error sem chave resolvível (consultando
  o cofre), Error para tipo desconhecido, Warning para `default_provider` ausente
  com `llm:` populado.
- **P1** `.env` carregado no topo de `Server::run` (rodava em `build_channels`,
  ~20 linhas depois de os providers já terem lido as env vars).
- **P1** `POST /api/providers` reporta `"persisted": bool` + WARN.
- **P2** banner marca `main ⚠ no API key` e mostra o arquivo em vigor.
- **P2** `docs/installation.md` (mandava editar `~/.garraia/config.yml`) e
  `README.md` (`<asset>.sha256` vs `SHA256SUMS`).

### §4.1 Mudança de comportamento declarada

Env var **vazia** passa a contar como ausente. `OPENROUTER_API_KEY=""` antes
registrava provider com credencial vazia que falhava com 401 opaco no primeiro
call; agora reporta o aviso acionável. Consistente com o tier de config, que já
ignorava string vazia.

## §5 Verificação

Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --exclude
garraia-desktop --all-targets -- -D warnings`, `garraia-config` 65 testes,
`garraia` 164, `garraia-gateway` 820 + 19 suítes de integração — todos verdes.

E2E em ambiente pristino (`env -i HOME=… PATH=… GARRAIA_CONFIG_DIR=…`,
replicando um pod novo):

| Cenário | Resultado |
| --- | --- |
| Config do pod (`api_key: null`) → `config check` | `[ERROR] llm.main` nomeando as três remediações, exit `2` (antes: silêncio) |
| Config do pod → `garra start` | banner `Provider main ⚠ no API key`, aviso com as três remediações, `0 active / 1 configured` |
| Chave no config → `garra start` | `configured openrouter provider: main`, **`1 active / 1 configured`** |
| Cofre presente sem passphrase | diagnóstico amigável do plan 0250 continua disparando (sem regressão) |

## §6 Fora de escopo (follow-ups)

- Endurecer o `install.sh` (validar que um provider ficou ativo antes do
  `exec start`) — considerado e não escolhido.
- Trocar o default de diretório para `~/.garraia`.
- Fazer o boot ler a store AES-GCM de `admin/secrets.rs` — 4º caminho de escrita
  de secret, hoje write-only no que diz respeito ao startup.
- Unificar `GARRAIA_VAULT_PASSPHRASE` (cofre) e `GarraIA_VAULT_PASSPHRASE`
  (fallback de auth), documentadas como distintas em `check.rs:134-139`.
  Confusão latente, merece issue própria.
- `mcp.json` vs `mcp:` no `config.yml` — duas superfícies de config MCP.
