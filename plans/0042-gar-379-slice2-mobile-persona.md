# Plan 0042 — GAR-379 slice 2: `[mobile]` section + `garra_persona` reads via config

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) (slice 2)
**Branch:** `feat/0042-gar-379-slice2-mobile-persona`
**Pré-requisitos:** [plan 0035](0035-gar-379-cli-config-check.md) (slice 1 shipped).
**Unblocks:** próximos slices de GAR-379 podem mover os demais env reads (secrets, OpenClaw, channels) um a um com a mesma cerimônia.

---

## 1. Goal

Entregar o **primeiro refactor narrow** do item "Remoção de leituras diretas de env/files em outros crates" de GAR-379: mover a leitura de `GARRA_MOBILE_PERSONA` (`crates/garraia-gateway/src/mobile_chat.rs:39`) para o schema tipado de `garraia-config`, preservando backward compat via env fallback.

Escopo:

1. Nova subseção `[mobile]` em `AppConfig` com campo `persona: Option<String>`.
2. `mobile_chat.rs::garra_persona()` consulta `state.config.mobile.persona` primeiro, cai em `GARRA_MOBILE_PERSONA` env depois, cai em `DEFAULT_PERSONA` por último.
3. `garraia config check` ganha uma validação: se `mobile.persona` está presente, precisa ter length ≥ 10 chars (senão emite WARN — caller provavelmente passou algo acidental).
4. Zero breaking change — operadores que usavam `GARRA_MOBILE_PERSONA` continuam funcionando.

## 2. Non-goals

- **Não** remove o fallback env. Isso é slice N+1 quando o config estiver validado em produção.
- **Não** refatora outros env reads do gateway (`GARRAIA_JWT_SECRET`, `GARRAIA_METRICS_TOKEN`, `OPENROUTER_API_KEY`, etc.). Cada um vira seu próprio slice narrow, com review de superfície de secret por par.
- **Não** expõe `mobile.persona` via admin API — permanece build-time/config-file-only.
- **Não** valida conteúdo da persona além de length mínima. Prompt engineering é out-of-scope para config validator.
- **Não** adiciona novo env var — o existente (`GARRA_MOBILE_PERSONA`) continua documentado em `.env.example` como override runtime.

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-config/src/model.rs` — nova struct `MobileConfig { persona: Option<String> }` + campo em `AppConfig`.
- `crates/garraia-config/src/check.rs` — nova regra `MOBILE_PERSONA_TOO_SHORT` (WARN quando `persona.len() < 10`).
- `crates/garraia-gateway/src/mobile_chat.rs` — `garra_persona(state)` agora recebe `&AppState` e consulta config antes do env; signature do helper muda.
- `plans/0042-gar-379-slice2-mobile-persona.md` (este).
- `plans/README.md` — entrada 0042.

Zero dependência nova. Zero schema change.

## 4. Acceptance criteria

1. `cargo check --workspace --exclude garraia-desktop` verde.
2. `cargo fmt --check --all` verde.
3. `cargo test -p garraia-config --lib` verde (novo unit test da regra de validação).
4. `cargo test -p garraia-gateway --lib mobile_chat` verde (novos testes do fallback 3-tier).
5. Config com `[mobile]` `persona = "string >= 10 chars"` faz `mobile_chat::garra_persona` retornar essa string, ignorando env e default.
6. Config sem `[mobile]` + env `GARRA_MOBILE_PERSONA` setado faz `garra_persona` retornar o env value.
7. Nem config nem env setados → `garra_persona` retorna `DEFAULT_PERSONA`.
8. `garraia config check --strict` com `persona = "hi"` (9 chars) exita 2.
9. `garraia config check` (não-strict) com persona curta emite WARN e exita 0.
10. `@code-reviewer` APPROVE.
11. `@security-auditor` APPROVE ≥ 8/10 (superfície é baixa — não é credential).
12. CI 9/9 green.
13. Linear GAR-379 comentada (slice 2 done).

## 5. Design rationale

### 5.1 Precedence: config > env > default

GAR-379 issue description lists precedence "CLI flags > Env vars > .garraia/config.toml > mcp.json > Defaults em código". Para o campo `persona`:

- Não há CLI flag para persona (nem deve ter — é texto multi-line).
- Config file é a fonte autoritativa desejada.
- Env var continua útil para deploys containerizados que preferem secrets via env.
- Default em código é o fallback seguro.

Portanto, a ordem no helper `garra_persona` fica **config > env > default**. Isso **diverge** da precedência do GAR-379 (que lista env acima de config), mas é coerente com o espírito do slice 1 (plan 0035): env vars continuam a funcionar mas o ponto de verdade é config-file.

Justificativa explícita: se o operator setar **ambos** config e env, o config prevalece — sinaliza que a migração config-first é efetiva. Um future slice pode inverter quando o env for marcado `#[deprecated]`.

### 5.2 Config helper recebe `&AppState`, não mais `()` com `OnceLock`

O atual `garra_persona()` usa `OnceLock` + env read lazy. Com config envolvido, o valor é derivado de `state.config` (já é `Arc<AppConfig>` shared). Lifecycle:

- Gateway startup → `AppConfig` carregado → `AppState` populado com `Arc<AppConfig>`.
- Handler invoca `garra_persona(state)` a cada chamada (cost: pointer chase + Option::as_ref).
- Sem `OnceLock` → hot-reload de config vira trivial (uma sessão futura).

O custo de uma resolution por chamada é desprezível (µs). O trade-off vale o ganho de testabilidade + reload.

### 5.3 WARN (não ERROR) para persona curta

Plan 0035 estabeleceu o padrão: findings WARN são *suspeitas* (valor plausível mas fora da faixa razoável); ERROR é *inválido*. Uma persona de 3 chars ainda é LEGALMENTE uma persona — só parece um erro do operator. WARN permite strict mode flag via `config check --strict` para CI sem quebrar dev.

### 5.4 Por que não remover env ainda?

Plan 0035 shippou `config check` que *reporta* presença de env vars. Remover agora o fallback env obrigaria operators em prod a editar `config.yaml` no mesmo release — cross-cutting migration. Slice seguinte pode marcar env `#[deprecated]` quando houver telemetria confirmando que nenhum caller usa mais.

## 6. Testing strategy

### 6.1 Unit — `garraia-config`

- `mobile_config_defaults`: `MobileConfig::default()` retorna `persona = None`.
- `check_persona_too_short_emits_warn`: `ConfigCheck::run` com `persona = Some("hi")` produz um `Finding` com `severity = Warn`, `code = "MOBILE_PERSONA_TOO_SHORT"`.
- `check_persona_happy_no_finding`: `persona = Some("Você é um assistente útil.")` não emite finding.

### 6.2 Unit — `garraia-gateway::mobile_chat`

- `garra_persona_prefers_config`: state com `mobile.persona = Some("foo")` e env `GARRA_MOBILE_PERSONA=bar` → retorna `"foo"`.
- `garra_persona_falls_back_to_env`: state com `mobile.persona = None` e env set → retorna env value.
- `garra_persona_falls_back_to_default`: state com persona `None` e env unset → retorna DEFAULT_PERSONA.

Testes usam `serial_test` ou limpeza explícita do env para evitar cross-contamination.

## 7. Security review triggers

- **SEC-L (baixa surface)**: `persona` não é credential — é prompt text. Mesmo que caia em audit log, o risco é reputacional (operator pode ter escrito algo embaraçoso), não PII/secret.
- **SEC-L tracing**: se o handler logar a persona (não o faz hoje), devemos manter `skip()` em qualquer future `tracing::instrument` que a toque, para evitar eco do prompt em OTLP.
- **SEC-L reload injection**: se no futuro o config hot-reload entrar em jogo, o novo valor de persona passa a vigorar para próximos request; não há race security-relevant porque persona é read-only e string.

Zero HIGH/MEDIUM esperado. Review focará em convenções + testabilidade.

## 8. Rollback plan

`git revert` do commit. Zero schema change, zero dependência nova, zero env var removida — rollback é trivial.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| `garra_persona` agora precisa `&AppState` — callers no `mobile_chat.rs` precisam propagar | BAIXO | Único caller é dentro de handler `chat()` que já tem `state`. Single-file edit. |
| Operator com env setado e config novo esquecido acha que nada mudou | BAIXO | Comportamento idêntico — env ainda é lido; apenas config prevalece quando ambos presentes. Documentado no plan. |
| `cargo test -p garraia-gateway --lib mobile_chat` polui env do processo | MÉDIO | Usar `std::env::set_var`/`remove_var` em `serial_test::serial` OU restringir testes a lock único. Implementado via guard manual. |
| Config value > MB degrada startup | BAIXO | 10k char persona é absurdo; validator pode adicionar cap futuro (slice N+1). |

## 10. Open questions

- **Q1**: Deveríamos mover `GARRA_MOBILE_PERSONA` env var para CLAUDE.md regra 6 como "nunca logar"? → **Não** — não é secret. Manter em `.env.example` como plain-text override documentado.
- **Q2**: Config hot-reload recebe a nova persona sem restart? → **Sim no design**, **não garantido neste slice**: `AppState::config` é `Arc<AppConfig>`; o watcher em `garraia-config::watcher` precisaria propagar a reference atualizada. Sem teste dedicado, reload fica fora de escopo.

## 11. Future work (slices 3+)

- Mover `GARRAIA_JWT_SECRET` para `[auth] jwt_secret` (com `SecretString` wrapper). Security review obrigatório.
- Mover `OPENROUTER_API_KEY` / `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` para `[llm.*] api_key` — alguns já estão, confirmar redaction.
- Marcar env vars originais `#[deprecated]` quando telemetria confirmar que callers migraram.
- Hot-reload test coverage (`garraia-config::watcher` + `AppState` swap).

## 12. Definition of done

- [ ] `MobileConfig` + campo em `AppConfig` implementados.
- [ ] `check.rs` validação nova.
- [ ] `mobile_chat.rs::garra_persona` recebe `&AppState` e usa precedência config > env > default.
- [ ] Unit tests verdes.
- [ ] `cargo check/clippy/fmt/test` verdes workspace-wide.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8/10.
- [ ] PR aberto.
- [ ] CI 9/9 green.
- [ ] PR merged.
- [ ] Linear GAR-379 comentada (slice 2 done — issue permanece "Done" mas o comentário documenta o progresso incremental).
- [ ] `plans/README.md` atualizado.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
