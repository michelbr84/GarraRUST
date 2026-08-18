# Plan 0354 — Fix do POST /v1/me/anonymize (coluna fantasma + email não anonimizado)

**Data:** 2026-08-18 · **Branch:** `fix/anonymize-users-email` · **Origem:** descoberta colateral
do cleanup de 2026-08-18 (investigação do Mutation Testing vermelho desde 2026-04-28).

## Problema

`POST /v1/me/anonymize` (LGPD art. 12 / GDPR art. 4(5), plan 0345 / GAR-888) retornava
**500 em toda chamada**:

1. `garraia_auth::anonymize_identity` fazia `UPDATE user_identities SET login = $1`,
   mas a coluna `login` **não existe** em nenhuma das 32 migrations deste repo
   (migration 001 define `provider_sub`; a coluna `login` existe só no schema do
   repo interno — código portado sem o schema, violação da regra 14 do CLAUDE.md).
2. Mesmo se a coluna existisse, o handler nunca anonimizava `users.email` — que é
   onde o email PII mora neste schema (citext UNIQUE), junto com
   `user_identities.provider_sub` (a chave de login: o signup insere o email ali e
   `verify_internal` casa `provider_sub = $email`; o comentário da migration 001
   dizendo "equals users.id::text" está desatualizado vs a implementação).
3. Bug latente adicional: o token anônimo usava só **8 hex** do UUID. Em produção os
   ids são UUIDv7 (time-ordered): usuários criados na mesma janela de ~65 s
   compartilham o prefixo de 32 bits → colisão nos UNIQUEs de `users.email` e
   `(provider, provider_sub)` → 500 no segundo anonymize.

Por que ninguém viu: os 16 binários de integração do garraia-auth têm
`required-features = ["test-support"]` e o `cargo test --workspace` do CI de PR os
**pula silenciosamente**. O único executor era o cargo-mutants semanal
(mutants.yml), vermelho havia 16 semanas — o `seed()` de `password_change.rs`
inseria a coluna fantasma e quebrava o baseline inteiro (exit 4) antes de
qualquer mutação.

## Decisão

- `anonymize_identity` (LoginPool, BYPASSRLS — a tabela é FORCE RLS, invisível ao
  `garraia_app`, regra 12): anonimiza `user_identities.provider_sub` com token
  determinístico de **UUID completo** `anon-<32 hex>@garraanon.local`. Retorna
  `u64` (linhas afetadas; 0 = sem identidade internal, não é erro).
- Handler `anonymize_me`: anonimiza **também `users.email`** (mesmo token) dentro
  da transação atômica do app_pool que já flipa `status`/`display_name`/sessions/
  audit (+ `updated_at = now()`, responsabilidade do caller por schema).
  `password_hash` fica intacto — `users.status != 'active'` é o gate autoritativo
  de login (`verify_internal` recusa com caminho constant-time).
- Seam entre os dois commits (LoginPool e app_pool): sem 2PC; falha entre eles
  cura com retry do endpoint (o UPDATE do provider_sub é idempotente e o guard
  409 só dispara após o commit do app-side).
- **Sem migration nova** — a alternativa (criar a coluna `login`) importaria schema
  do repo interno sem necessidade.

## Testes

- `password_change.rs`: `seed()` corrigido (sem coluna fantasma; `provider_sub` =
  email lowercase, espelhando o signup real). Testes novos:
  `anonymize_identity_replaces_provider_sub` (token exato de 32 hex),
  `anonymize_identity_leaves_other_users_untouched` (WHERE clause),
  `anonymize_identity_returns_zero_for_unknown_user` (contrato de 0 linhas).
- Todos os testes do binário agora são `#[serial_test::serial]`: cada um segura
  conexão do LoginPool (max 5) através de hashing lento dentro de tx — 6
  concorrentes estouravam o acquire timeout (flake real observada:
  "pool timed out while waiting for an open connection").
- **Gap estrutural fechado:** job novo `auth-integration` no ci.yml roda
  `cargo test -p garraia-auth --features test-support` no PR path (ubuntu,
  Docker, pre-pull pgvector/pg16 como no mutants.yml). Não entra nos required
  checks do ruleset por ora.

## Efeitos esperados

- Endpoint volta a funcionar e a anonimização cobre os dois lugares onde o email
  vive. Baseline do cargo-mutants destravado → workflow semanal deve voltar a
  ficar verde no próximo agendamento (conferir domingo).
- Follow-up fora deste plan: teste HTTP de integração do endpoint no
  garraia-gateway (hoje não existe suite HTTP para /v1/me/anonymize).
