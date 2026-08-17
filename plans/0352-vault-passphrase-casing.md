# Plan 0352 — Unificar as duas grafias da passphrase do cofre (issue #824)

> **Status:** Implementado em 2026-08-17 (America/New_York).
> **Origem:** issue #824, levantada durante o plan 0351 / PR #823 e deliberadamente
> deixada fora daquele escopo (§6 do plan 0351).

## 1. Problema

Duas env vars quase idênticas coexistiam com significados diferentes, e errar a
caixa falhava em silêncio:

| Variável | Consumidor (antes) | Efeito |
| --- | --- | --- |
| `GARRAIA_VAULT_PASSPHRASE` (all-caps) | `garraia-security::try_vault_get/set` | abre o cofre de credenciais |
| `GarraIA_VAULT_PASSPHRASE` (mixed-case) | `garraia-config::auth::AuthConfig::from_env` | fallback do JWT secret |

Exportar a grafia "errada" não produzia erro: o cofre não abria (provider
pulado no boot com "no API key" enigmático) ou `/auth/*` respondia 503.

## 2. Decisão (opções 3 + 1 da issue)

Aceitar **ambas as grafias em cada consumidor**, com warnings de detecção —
zero breaking change:

- **Cofre** (`garraia-security`): nova fonte única
  `vault_passphrase_from_env()` — prefere `GARRAIA_VAULT_PASSPHRASE`
  (canônica), cai para `GarraIA_VAULT_PASSPHRASE` com `warn!` de deprecation.
  Valores vazios contam como ausentes nos dois tiers. Usada por
  `try_vault_get`, `try_vault_set`, `try_vault_delete_prefix`,
  `provider_keys::vault_passphrase_available` e pelo early-out de
  `mcp/persistence.rs::encrypt_to_vault`.
- **Auth** (`garraia-config::auth`): `GARRAIA_VAULT_PASSPHRASE` vira o
  **último** fallback do JWT secret. Precedência preservada:
  `GARRAIA_JWT_SECRET` > `GarraIA_VAULT_PASSPHRASE` >
  `GARRAIA_VAULT_PASSPHRASE`. A mixed-case fica à frente da all-caps de
  propósito: deploys que setam as duas grafias com valores diferentes
  continuam assinando JWT com o mesmo secret de antes.
- **`garraia config check`**: dois warnings novos —
  1. deprecation sempre que a mixed-case estiver setada;
  2. alerta de split quando as duas grafias têm valores **diferentes**
     (comparação de igualdade apenas; valores nunca aparecem no output).
  O warning "neither ... is set" passa a citar as três vars aceitas.
- **Diagnostics** (`/api/diagnostics` check `secrets.jwt`): passa a considerar
  a all-caps também.

### Fora do escopo (deliberado)

- `admin/shared.rs::derive_encryption_key` NÃO ganhou o fallback mixed-case:
  faria deploys que só têm a mixed-case setada re-derivarem a chave do admin
  store (antes caíam no `master.key` gerado), quebrando secrets já
  criptografados. Fica como está.
- `settings_handler.rs` `secrets.jwt_secret` continua reportando presença
  apenas de `GARRAIA_JWT_SECRET` (contrato de UI já era var-específico).
- Renomear a var de auth (opção 2 da issue) — deprecation primeiro; um slice
  futuro pode remover a mixed-case após ciclo de aviso.

## 3. Arquivos tocados

- `crates/garraia-security/src/credentials.rs` — consts + helper + 3 call
  sites + 2 testes novos (lock de env próprio).
- `crates/garraia-security/src/lib.rs` — re-exports.
- `crates/garraia-config/src/provider_keys.rs` — const re-apontada +
  `vault_passphrase_available` via helper.
- `crates/garraia-config/src/auth.rs` — fallback triplo em `from_env` /
  `require_from_env` + docs + `EnvSnapshot` cobre all-caps + 2 testes novos.
- `crates/garraia-config/src/check.rs` — `jwt_env_set` inclui all-caps +
  2 findings novos + 3 testes novos.
- `crates/garraia-config/src/lib.rs` — `ENV_TEST_LOCK` crate-wide (auth::tests
  e check::tests mutam as mesmas vars no mesmo binário).
- `crates/garraia-gateway/src/{mcp/persistence.rs,diagnostics_handler.rs,state.rs}`.
- Docs: `docs/auth-config.md`, `.env.example`, `CHANGELOG.md`, `CLAUDE.md`.

## 4. Verificação

- `cargo test -p garraia-security -p garraia-config` — 96 testes verdes,
  incluindo os 7 novos.
- `cargo test -p garraia-gateway --lib` — 820 verdes.
- `cargo clippy -p garraia-security -p garraia-config -p garraia-gateway
  --all-targets` — limpo.
- Redaction invariant: os testes novos asseguram que os valores das
  passphrases nunca aparecem nas mensagens dos findings.
