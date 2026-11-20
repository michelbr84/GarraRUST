# ADR 0028 — Saída do `chrono`/`chrono-tz` unmaintained (RUSTSEC-2026-0543/0555)

- **Estado:** proposto — a escolha de alvo (seção [Decisão proposta](#decisão-proposta)) é do dono.
- **Data:** 2026-11-20, America/New_York.
- **Base auditada:** `4bb7693` (`main`).
- **Issue:** [#1938](https://github.com/michelbr84/GarraRUST/issues/1938). Exceção atual: PR #1939, `deny.toml` e `.cargo/audit.toml`, `EXPIRA 2026-12-31` (decisão D27 do dono).

## Problema

Em 2026-11-17 o RustSec marcou `chrono` (RUSTSEC-2026-0543) e `chrono-tz` (RUSTSEC-2026-0555) como `unmaintained`, sem versão segura. O `cargo-deny` só voltou ao verde com uma exceção que vence em 2026-12-31. O critério de fechamento da #1938 é **`chrono` e `chrono-tz` fora do `Cargo.lock`** (ou os advisories retirados upstream).

A pergunta deste ADR é se esse critério é alcançável antes do prazo, e por qual caminho. A resposta curta: **não é alcançável só com trabalho neste repositório**, porque três dependências de terceiros puxam `chrono` sem feature que o desligue. Por isso a migração do nosso código e a saída do `Cargo.lock` são decisões separadas.

## Evidências (medidas em `4bb7693`)

### Nosso código

- `chrono` é dependência direta de 9 crates: `agents`, `auth`, `channels`, `cli`, `common`, `db`, `gateway`, `hardware` (opcional, feature `automations`) e `workspace`. O `garraia-media` não a declara, mas liga a feature `chrono` do `lopdf`. `chrono-tz` é direta de `db` e `workspace`.
- 100 arquivos `.rs` mencionam `chrono` (17 deles em `tests/`); 58 só no `garraia-gateway`.
- O uso é estreito: `Utc::now` (104 ocorrências), `DateTime<Utc>`, `DateTime::from_timestamp` (28), `parse_from_rfc3339` (10), `to_rfc3339_opts(SecondsFormat::Secs, …)` (12), `Duration::{seconds,days,hours,minutes}`. Fuso horário (`chrono_tz`) aparece só nos dois `recurrence.rs` (`garraia-db` e `garraia-workspace`, ADR 0013).
- 29 arquivos usam `DateTime<Utc>` junto com `sqlx` — o tipo atravessa a fronteira do Postgres (`TIMESTAMPTZ`) em `FromRow`/`bind`.
- 22 arquivos do gateway usam `DateTime` em tipos com `utoipa` (feature `chrono` do `utoipa` no `Cargo.toml` do gateway).

### Quem puxa `chrono` além de nós (`cargo tree -i chrono -e features`)

| Crate | Como puxa | Dá para desligar? |
| --- | --- | --- |
| `teloxide-core 0.13.0` (Telegram) | dependência **obrigatória**; 40 arquivos do fonte usam `chrono` | **Não.** 0.13.0 é a última no crates.io |
| `rmcp 3.9.0` (MCP) | `schemars` com feature `chrono04` sempre que `schemars` está ligado, e `server` (default) liga `schemars` | **Não** na 3.9. A 3.11.0 existe e não foi inspecionada a fundo |
| `rrule 0.14.0` (recorrência do workspace) | `chrono` **e** `chrono-tz` obrigatórios, expostos na API | **Não.** 0.14.0 é a última |
| `sqlx 0.9.1` | feature `chrono` que nós ligamos | Sim, trocando por `time`. O `sqlx-postgres 0.9.1` tem módulos `chrono` e `time` e **não tem `jiff`** |
| `croner 4.0.3` | feature `chrono` default | Sim: tem feature `jiff` |
| `lopdf 0.47.0` | feature `chrono` que nós ligamos (já ligamos `jiff` também) | Sim |
| `serenity 0.12.5` via `poise` | feature `chrono` ligada explicitamente por nós em `poise` | Provavelmente sim; o `serenity` expõe `chrono` em tipos de modelo, precisa medir |
| `utoipa 6` | feature `chrono` ligada por nós | Sim, se os tipos mudarem |

O `chrono-tz` só chega por nós (`db`, `workspace`) e pelo `rrule`.

## Consequência direta

Mesmo migrando 100% do nosso código, **`chrono` continua no `Cargo.lock` por `teloxide-core`, `rmcp` e `rrule`**, e **`chrono-tz` continua por `rrule`**. A exceção não pode ser retirada em 2026-12-31 por esforço local. Renovar o prazo sem dizer isso seria a "renovação por inércia" que o comentário de 2026-11-17 na #1938 quis evitar; renová-lo **com** esta evidência é outra coisa.

`unmaintained` não é vulnerabilidade: o `cargo audit --deny unsound` do CI não é afetado. O risco real é o de qualquer dependência sem dono — um CVE futuro sem correção upstream.

## Opções

### Alvo da migração do nosso código

| Critério | A — `jiff` | B — `time` | C — ficar no `chrono` |
| --- | --- | --- | --- |
| Postgres via `sqlx 0.9` | **Sem suporte nativo**: precisa de newtype com `Encode`/`Decode` próprios ou conversão em cada `FromRow` | Nativo (`sqlx` feature `time`) | Nativo |
| Fuso horário (substituto do `chrono-tz`) | Embutido (tzdb) | Não tem; exige outro crate (`time-tz`/`tzdb`) | `chrono-tz` (unmaintained) |
| Cron (`croner`) | Feature `jiff` pronta | Não suportado pelo `croner 4` | Atual |
| `rrule` | Não suporta; continua exigindo `chrono` na fronteira | Idem | Atual |
| Já no `Cargo.lock` | Sim (0.2.28, via `lopdf`); a 1.0.1 já saiu | Sim (0.3.47, via `bollard`, `serenity`, `tracing-appender`…) | Sim |
| Tamanho da troca | ~100 arquivos + adaptador `sqlx` | ~100 arquivos + crate de tz | Zero |

Nenhum alvo cobre tudo sozinho: `jiff` resolve fuso e cron mas não tem `sqlx`; `time` resolve `sqlx` mas não tem fuso nem cron.

### Retirada da exceção

1. **Esperar upstream** (`teloxide-core`, `rmcp`, `rrule`) e só então remover as duas IDs.
2. **Substituir as três dependências.** `rrule` é a única pequena o bastante para considerar (só os dois `recurrence.rs`); `teloxide` e `rmcp` são a base dos canais Telegram e do MCP — trocar está fora de proporção para um advisory `unmaintained`.
3. **Contribuir upstream** (feature para desligar `chrono` no `teloxide-core`/`rmcp`). Depende de terceiros aceitarem.

## Decisão proposta

1. **Renovar a exceção com prazo, com esta evidência no `reason`** — por exemplo `EXPIRA 2027-06-30` —, mantendo o gate `scripts/security/check_deny_exceptions_expiry.py` e a regra da D27 de reavaliar a cada release upstream. Isso é decisão do dono (R5, exceção de segurança) e **não** é feito neste ADR.
2. **Não migrar o nosso código agora.** Migrar ~100 arquivos sem tirar o `chrono` do grafo troca risco de regressão em auth/timestamps de auditoria/JWT por ganho nenhum no critério da #1938. A migração só passa a valer quando ao menos `teloxide-core` e `rmcp` tiverem saída.
3. **Reduzir já a superfície que só nós escolhemos**, em PRs pequenas e independentes, sem trocar tipos públicos: desligar a feature `chrono` do `lopdf` (já usa `jiff`), avaliar a do `poise`/`serenity` e preferir `croner` com `jiff` se a troca ficar restrita a `garraia-hardware`/`garraia-db`. Cada uma remove arestas do grafo, não o pacote.
4. **Quando a migração vier, alvo `jiff`** com um newtype único na fronteira do `sqlx` (um módulo em `garraia-common`, testado contra `TIMESTAMPTZ` real), porque fuso e cron — as partes difíceis — já estão prontos nele, e o `chrono-tz` sai junto. `time` exigiria um segundo crate de fuso e não serve ao `croner`.
5. **Monitorar** `teloxide-core`, `rmcp` (começando pela 3.11, que não foi inspecionada aqui) e `rrule` a cada sonda de dependências, registrando na #1938.

## Consequências

- **Positivas:** o prazo deixa de ser uma armadilha que só se resolve renovando sem justificativa; o trabalho local fica restrito ao que muda o grafo de verdade.
- **Negativas:** `chrono` segue no binário por tempo indeterminado. Se aparecer um advisory de vulnerabilidade (não `unmaintained`), a exceção não cobre e a resposta passa a ser patch/fork, não migração.
- **Neutras:** nenhum código, `deny.toml` ou `.cargo/audit.toml` muda neste ADR.

## Links

- Issue #1938; PR #1939 (exceção, squash `73e89e4b`).
- `deny.toml` (seção `[advisories]`), `.cargo/audit.toml`, `scripts/security/check_deny_exceptions_expiry.py`.
- ADR 0013 (recorrência em duas camadas: `croner` no SQLite, `rrule` no workspace).
- Anúncio upstream citado pelos advisories: `chronotope/chrono#1768` (não lido nesta auditoria).
