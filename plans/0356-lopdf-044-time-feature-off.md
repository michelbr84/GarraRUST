# Plan 0356 — lopdf 0.42 → 0.44 com a feature `time` desligada

**Status:** 🚧 Em revisão
**Branch:** `claude/lopdf-formatitem-bug-y6f4a8`
**Origem:** Dependabot PR #851 (`dependabot/cargo/lopdf-0.43.0`), 7 jobs de CI vermelhos
**Data:** 2026-08-29 (America/New_York)

## Goal

Destravar o bump do lopdf — parado desde a 0.43 por um bug de compilação upstream — e
aproveitar a janela para fechar estruturalmente o ignore de RUSTSEC-2026-0192
(`ttf-parser` não mantido) no `deny.toml`.

## Root cause

A partir da 0.43.0 o `src/datetime.rs` do lopdf tem, em `mod time_impl`:

```rust
date.format(&FormatItem::StringLiteral("%Y%m%d%H%M%SZ")).unwrap()
```

Isso está errado em dois níveis: `%Y%m%d…` é sintaxe strftime (o crate `time` usa
`[year][month]…`), e `StringLiteral` emite o texto verbatim — ou seja, mesmo compilando,
o impl produziria a string literal `D:%Y%m%d%H%M%SZ` em vez de uma data.

**Nuance verificada empiricamente** (importante para não documentar a causa errada): a
variante `StringLiteral` **existe** a partir do `time 0.3.49` — foi introduzida na 0.3.48
(yanked), e `Literal(&[u8])` virou `#[deprecated]` em favor dela. O `Cargo.lock` deste
repo fixa `time 0.3.47`, anterior à variante. Daí o erro ser de compilação:

```
error[E0599]: no variant, associated function, or constant named `StringLiteral`
found for enum `BorrowedFormatItem<'a>`
  --> lopdf-0.43.0/src/datetime.rs:103:46
```

Conferido nos tarballs do crates.io: 0.3.47 não tem a variante; 0.3.49 e 0.3.55 têm.

Upstream: J-F-Liu/lopdf#518, corrigido no master em `1efa2702` (PR #527, 2026-07-16).
**Nenhum release publicado tem o fix** — a 0.44.0 é de 2026-07-10, seis dias antes.
Dependência git está fora de questão: o `deny.toml` tem `[sources] allow-git = []` e não
existe `[patch]` no `Cargo.toml` raiz.

## Fix

O `time_impl` é inteiramente `#[cfg(feature = "time")]` (linhas 92-142). Fora do gate
ficam só `convert_utc_offset`, `pub struct DateTime(String)` e o `impl Object`. O único
outro site de `feature = "time"` no crate está em `creator.rs:207/212`, dentro de
`#[cfg(test)] pub mod tests` — nunca compilado por consumidores.

O `garraia-media` não faz **nenhuma** interop de data/hora com o lopdf: `CreationDate` e
`ModDate` saem por `Object::as_str()` como bytes crus e passam por `String::from_utf8_lossy`
(`src/pdf.rs:136-143`). Desligar a feature é, portanto, preservador de comportamento.

```toml
lopdf = { version = "0.44", default-features = false, features = ["chrono", "jiff", "rayon"] }
```

`chrono`/`jiff`/`rayon` são mantidas — é exatamente o conjunto default da 0.42 menos a
feature quebrada, então o delta de comportamento é zero. `rayon` é o caminho de parse
paralelo de object streams do lopdf (com fallback sequencial em `cfg(not(feature = "rayon"))`).

## Por que 0.44 e não 0.43

Mesmo esforço, e a 0.44 moveu o `ttf-parser` para trás da feature nova `font_embedding`
(não-default; gateia apenas `mod font`/`FontData` e `Document::add_font`, que não usamos).
Resultado confirmado pelo `cargo update`:

```
Removing ttf-parser v0.25.1
```

Isso fecha **RUSTSEC-2026-0192** estruturalmente — é literalmente o que o comentário do
ignore no `deny.toml` pedia ("wait for lopdf upstream to migrate ... then bump lopdf again").

A 0.44 também adiciona limites contra decompression bombs (`LoadOptions.max_decompressed_size`,
`extract_text_with_limit`, `get_page_content_with_limit`), relevantes porque o `garraia-media`
parseia PDFs vindos de upload. Adotá-los é follow-up, não escopo deste plan.

Quebra de API da 0.44: `Document::get_page_content` passa de `Result<Vec<u8>>` para
`Vec<u8>`. **Zero call sites** no workspace — `crates/garraia-media/src/pdf.rs` é o único
consumidor de lopdf e usa apenas `Document::load`, `load_mem`, `get_pages`, `extract_text`,
`get_dictionary`, `trailer` + `Dictionary::get`, `Object::as_reference` e `Object::as_str`,
todos inalterados na 0.44. Nenhuma linha de código de produção mudou neste plan.

## Cobertura de runtime

Os 4 testes reais de PDF em `src/pdf.rs` estão `#[ignore]`d desde `fix/ci-triage-2026-04-15`
("pre-existing PDF extraction failure"), então o `cargo test` provava apenas que o crate
*compila* contra o lopdf — num bump de parser isso não vale nada.

Adicionado `test_lopdf_roundtrip_smoke`: constrói um PDF de 1 página com o **próprio writer
do lopdf**, serializa por `Document::save_to` e lê de volta por
`PdfProcessor::extract_text_from_bytes`, exercendo writer → reader → xref → content stream
→ extração de texto. Não reaproveita `create_test_pdf` de propósito: aqueles são bytes
escritos à mão com offsets de `xref` inválidos, que é justamente a causa dos 4 `#[ignore]`.

Validado como guarda real: **verde na 0.44 e também na 0.42** (com o bump em `git stash`),
ou seja, não codifica o comportamento de uma versão só. Os 4 `#[ignore]` pré-existentes não
foram tocados — dívida separada, fora de escopo.

## Out of scope

- Destravar os 4 testes `#[ignore]`d (fixture com xref inválido) — dívida desde 2026-04-15.
- Adotar as APIs de limite da 0.44 (`LoadOptions.max_decompressed_size`, `extract_text_with_limit`).
- Bump `time 0.3.47 → 0.3.55` — mexe em serenity/tauri/aws-sdk/tracing-appender/cookie e é
  desnecessário aqui, já que o `time_impl` nunca é compilado. Merece PR próprio.

## File structure

| Arquivo | Mudança |
|---|---|
| `crates/garraia-media/Cargo.toml` | `lopdf = "0.42"` → tabela `default-features = false` + comentário de justificativa |
| `Cargo.lock` | `cargo update -p lopdf --precise 0.44.0` |
| `deny.toml` | remove o ignore de RUSTSEC-2026-0192 (12 linhas) |
| `.cargo/audit.toml` | linha de closed history no SYNC NOTE (comentário) |
| `crates/garraia-media/src/pdf.rs` | `build_minimal_pdf()` + `test_lopdf_roundtrip_smoke` |
| `CHANGELOG.md` | `[Unreleased]` → Changed + Added + Security |
| `TODO.md` | lopdf sai da lista de upgrades adiados; rmcp permanece |
| `plans/README.md` | linha de índice 0356 |

Diff do `Cargo.lock` restrito ao subgrafo do lopdf, como reportado pelo cargo:
`lopdf 0.42.0→0.44.0`, `aes 0.8.4→0.9.3`, `cbc 0.1.2→0.2.1`, `ecb 0.1.2→0.2.1`,
`cipher 0.4.4→0.5.2`, `inout 0.1.4→0.2.2`, `block-padding 0.3.3→0.4.2`,
`bitflags 2.11.1→2.13.1`, `+cpubits 0.1.1`, `+weezl 0.2.1`, `-md-5 0.10.6`,
`-ttf-parser 0.25.1`.

## Acceptance criteria

- [x] `cargo +1.95 check -p garraia-media` limpo (o E0599 desaparece)
- [x] `cargo tree -i ttf-parser` não casa nenhum pacote
- [x] `cargo tree -e features -i lopdf` mostra chrono/jiff/rayon e **não** `time`
- [x] `cargo test -p garraia-media` verde, com `test_lopdf_roundtrip_smoke` passando
- [x] `cargo fmt --check --all` limpo
- [x] `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` limpo
- [x] `cargo +1.95 check --workspace --exclude garraia-desktop --locked` limpo
- [x] `cargo deny check` limpo, sem `advisory-not-detected`
- [x] `cargo audit --deny unsound` limpo

## Risk register

| Risco | Prob. | Mitigação |
|---|---|---|
| Código não-gated do lopdf precisa de `time` | Muito baixa — os dois únicos sites de `feature = "time"` são o `time_impl` e um `#[cfg(test)]` interno | `cargo check -p garraia-media` prova; verde |
| `extract_text` regride entre 0.42 e 0.44 sem quebrar compilação | Média — é o risco real, dado que os 4 testes estão ignorados | `test_lopdf_roundtrip_smoke` (novo) |
| `aes 0.8 → 0.9` muda o path de PDF criptografado | Baixa | sem call site de PDF criptografado; mudança interna ao lopdf |
| Esquecer o `deny.toml` e virar `advisory-not-detected` | Certa se esquecido | removido neste PR; `cargo deny check` valida |
| Alguém "restaura" a feature `time` no futuro | Média, ao longo do tempo | comentário longo no `Cargo.toml` nomeando a issue upstream e a condição de saída |

Rollback: nada aqui toca runtime, config, migrations ou API pública — `git revert` simples
(ou `git revert -m 1 <merge-sha>` pós-squash) restaura `lopdf = "0.42"`, o subgrafo antigo,
o `ttf-parser` e seu ignore. Se só o smoke test der problema em algum OS da matriz, reverter
apenas aquele commit; o fix de dependência é independente.

## Follow-ups

1. Adotar `LoadOptions::max_decompressed_size` / `extract_text_with_limit` no `garraia-media`
   — hardening real para PDFs de upload não confiáveis.
2. Bump `time 0.3.47 → 0.3.55` como PR próprio.
3. Quando sair um lopdf > 0.44.0: confirmar que `1efa2702` está nele
   (`grep StringLiteral` no `src/datetime.rs` publicado), remover `default-features = false`
   e o bloco de comentário.
4. Destravar os 4 testes de PDF `#[ignore]`d.

## Cross-references

- Dependabot PR #851 (lopdf 0.43.0) — superseded por este trabalho
- Upstream J-F-Liu/lopdf#518 / PR #527 / commit `1efa2702`
- plan 0259 (lopdf 0.34 → 0.40) — precedente direto de bump com compat fix
- `deny.toml` RUSTSEC-2026-0192 / GAR-895 (health run 146)
