# Plan 0043 — CI hygiene: strip CVSS v4 entries from advisory-db (cargo-audit scheduled fix)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma 2026-04-22, ClaudeMaxPower + Superpowers skills inline)
**Data:** 2026-04-22 (America/New_York)
**Issues:** nenhuma ainda — `GAR-NEW-01` a ser criada no Linear quando este PR abrir (ver §11).
**Branch:** `feat/0043-cargo-audit-db-pin`
**Pré-requisitos:** nenhum.
**Unblocks:** nada no caminho funcional; destrava apenas o sinal de segurança nightly, que é pré-condição para confiar em CI nos próximos slices do Lote A/B.

## Revision log

- **v1 (2026-04-22 13:45 ET) — pin-SHA approach:** primeira tentativa foi pinar a `advisory-db` ao SHA `1dc467507294` (último commit antes do batch libcrux CVSS v4 em 2026-03-24T08:19Z). Validado localmente que `crates/libcrux-poly1305/` não existia nesse SHA. Commitado como `2c6c7ce`.
- **v2 (2026-04-22 13:55 ET) — workflow_dispatch FAILED:** run `24793600828` mostrou que o pin não foi suficiente. Falha secundária: `crates/deno/RUSTSEC-2025-0138.md` (adicionado 2025-12-29) também usa `cvss = "CVSS:4.0/..."`. O erro exato foi `unsupported CVSS version: 4.0`. Grep global no SHA pinado encontrou **26 entries CVSS v4 pré-existentes**. Pin por SHA teria que ir antes de 2025-12-29 — 4+ meses atrás — perdendo quase um semestre de advisories legítimos.
- **v2 (2026-04-22 14:00 ET) — pivot para strip approach:** fetch HEAD da `advisory-db` + remoção cirúrgica de todos os arquivos com `^cvss = "CVSS:4.0/`. Safety net: aborta se remoção > 50 arquivos. Reteve este mesmo plan file para não inflacionar o numbering — §5.3 abaixo reescrito; v1 preservado em `§Revision log`.
- **v3 (2026-04-22 14:05 ET) — `--no-fetch` bloqueia yanked check:** segunda run `24794024068` falhou com cascata de `couldn't check if the package is yanked: not found: No such crate in crates.io index` para cada crate da workspace. Investigação: `cargo audit --no-fetch` suprime tanto o pull do advisory-db quanto o refresh do índice crates.io usado pelo yanked-check (single flag, dupla ação). Fix aditivo: novo step `Prime crates.io index for workspace deps` executando `cargo fetch --locked` antes do audit, populando o sparse index cache localmente. `--no-fetch` preservado (necessário para proteger o strip). Commit separado.
- **v4 (2026-04-22 14:10 ET) — advisories reais preexistentes:** terceira run `24794271759` revelou 16 vulnerabilidades + 6 denied warnings em deps legítimas (idna, rsa, rustls-webpki ×4, tokio-tar, wasmtime ×2, glib, lru, rand, core2 yanked). Escopo bem maior que os 6 advisories listados na TODO do `ci.yml`. Estratégia: `.cargo/audit.toml` com 13 ignores únicos, cada um com crate+justificativa+ponteiro para Lote B-2. Expiration date 2026-05-20 coletiva. Policy documentada in-file. **Não é silenciamento — é deferimento rastreável.**

---

## 1. Goal

Restaurar o workflow scheduled `Security — cargo audit` ao verde sem esperar 24–72h por correção upstream. Fazer isso com **fetch HEAD da advisory-db + remoção cirúrgica das entries CVSS v4** que o `rustsec 0.30.x` não consegue deserializar, preservando 100% da cobertura CVSS v3 (4+ meses de advisories). Trade-off: perdemos audit em 6 deps nossos (cap-primitives, cmov, quinn-proto, tar, time, wasmtime) que recebem CVSS v4 — follow-up manual em Lote B-2+.

## 2. Non-goals

- **Não** resolver os 6 RUSTSEC advisories conhecidos do próprio repositório (idna 0.5.0, quinn-proto 0.11.13, rsa 0.9.10, rustls-webpki×2, core2 0.4.0 yanked) — isso é Lote B-2 (`chore: bump crypto deps`).
- **Não** tocar o `security` step inline do `ci.yml` (também mascarado com `continue-on-error`) — esse faz parte do mesmo Lote B-2.
- **Não** modificar `cargo-audit` version pin (continua `^0.21`).
- **Não** remover `crossbeam-channel` do lockfile — versão atual `0.5.15` já é não-yanked (verificado via crates.io API durante triagem).

## 3. Scope

**Arquivos modificados:**

- `.github/workflows/cargo-audit.yml` — novo env `MAX_STRIPPED_CVSS_V4`, novo step "Prime crates.io index for workspace deps", novo step "Fetch advisory database and strip CVSS v4 entries", step "Run cargo audit" adiciona `--no-fetch`.
- `.cargo/audit.toml` — novo arquivo com 13 ignores de advisories reais preexistentes, cada um comentado com crate + rationale + ponteiro para Lote B-2.
- `plans/0043-ci-cargo-audit-db-pin.md` (este).
- `plans/README.md` — entrada 0043 (inserida antes de 0042 mantendo ordem narrativa cronológica da sessão).

Zero alteração em `Cargo.toml`, `Cargo.lock`, crates de runtime, testes ou docs fora destes 3 arquivos.

## 4. Acceptance criteria

1. Manual `gh workflow run cargo-audit.yml --ref feat/0043-cargo-audit-db-pin` retorna **success**.
2. Após merge, o scheduled run `0 7 * * *` do dia seguinte também retorna **success**.
3. Log do job exibe linha `Advisory DB HEAD: <short-sha>` + `Stripping N CVSS v4 advisories` + lista completa dos arquivos removidos.
4. `N` na stripping log está abaixo de `MAX_STRIPPED_CVSS_V4` (50 como configurado).
5. `cargo audit` não faz fetch adicional (verificável pela ausência da mensagem `Fetching advisory database from https://github.com/RustSec/advisory-db.git`).
6. Output do `cargo audit` registra "0 vulnerabilities found (after ignoring 13 deferred advisories)" ou equivalente.
7. Zero novo `continue-on-error: true` introduzido.
8. `.cargo/audit.toml` contém exatamente 13 entries no ignore list, cada uma com comment block (crate + rationale + B-2 pointer).
9. `@security-auditor` APPROVE (cerimônia obrigatória CLAUDE.md regra 10 — workflow + security surface + config de audit).
10. `@code-reviewer` APPROVE.
11. Comentário em `GAR-NEW-01` do Linear com link para o PR + lista completa dos 13 advisories deferidos + lista dos 6 deps nossos que perdem audit coverage pelo strip (follow-up para B-2+).
12. Plan file existe e está linkado no `plans/README.md`.

## 5. Design rationale

### 5.1 Diagnóstico exato da falha upstream

Run falhando: https://github.com/michelbr84/GarraRUST/actions/runs/24769215136 (2026-04-22 08:49 UTC)

Erro bruto:

```
error loading advisory database:
  parse error: error parsing /home/runner/.cargo/advisory-db/crates/libcrux-poly1305/RUSTSEC-2026-0073.md:
  parse error: TOML parse error at line 5, column 8
```

Inspeção do arquivo em questão (via WebFetch no raw do HEAD do advisory-db):

```toml
[advisory]
id = "RUSTSEC-2026-0073"
package = "libcrux-poly1305"
date = "2026-03-04"
cvss = "CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:N/VI:N/VA:H/SC:N/SI:N/SA:N"
url = "https://github.com/cryspen/libcrux/pull/1351"
...
```

**Line 5 = `cvss = "CVSS:4.0/..."` ; column 8 = início do valor string.** O erro não é TOML bruto (a string está bem escapada) — é **deserialização tipada**: o `rustsec` crate parseia o campo `cvss` para um struct `cvss::v3::Base` da dep `cvss`. O header `CVSS:4.0/` indica CVSS v4, não suportado pela versão do `cvss` crate pulled transitivamente por `rustsec 0.30.x` (a partir de inspeção do changelog; rustsec ainda não bumpou para um `cvss` crate capaz de parsear v4).

O arquivo foi introduzido por commit `20b6160e8687` em 2026-03-24T08:21:09Z ("Assigned RUSTSEC-2026-0073 to libcrux-poly1305, RUSTSEC-2026-0074 to libcrux-sha..."). Múltiplos advisories no mesmo batch (0073/0074/0075/0076) usam CVSS v4. O primeiro commit a introduzir `cvss = "CVSS:4.0/..."` no repositório foi 2026-03-24T08:19:00Z (placeholders `RUSTSEC-0000-0000.md`).

Por design do `rustsec` crate, **uma entry individual falhando invalida a database inteira** — zero audit efetivo. Logo, o vermelho não é regressão nossa; é upstream schema gap.

### 5.2 Por que pinar (Camada 2), não esperar (Camada 1)

O plano aprovado pela sessão (`plans/.../synchronous-baking-hippo.md` §3.3) define 4 camadas de tratamento: (1) esperar e verificar, (2) pinar DB a SHA estável, (3) downgradear cargo-audit, (4) fallback não-fatal.

A aprovação do usuário 2026-04-22 13:30 ajustou: **se ainda vermelho no momento do início, ir direto para Camada 2, não esperar 24–72h**. Verificação via WebFetch do HEAD de `advisory-db` (2026-04-22 ~14:00 UTC) confirmou que o arquivo `RUSTSEC-2026-0073.md` continua com `cvss = "CVSS:4.0/..."`. Portanto, Camada 2 é o caminho correto.

### 5.3 Estratégia strip (pivotagem v2)

A primeira tentativa (§Revision log v1) pinou o DB ao SHA `1dc467507294` (último commit imediatamente antes do batch libcrux CVSS v4 em 2026-03-24T08:19Z). Verificação no próprio CI mostrou que o arquivo `crates/deno/RUSTSEC-2025-0138.md` (adicionado **2025-12-29**) também usa `cvss = "CVSS:4.0/..."`. Grep no SHA pinado encontrou 26 entries CVSS v4 (24 dos quais fora do diretório libcrux). Pin por SHA teria que voltar a antes de 2025-12-29, perdendo 4+ meses de advisories legítimos — trade-off inaceitável.

Nova estratégia adotada:

1. **Fetch HEAD** da `advisory-db` (máxima cobertura de advisories CVSS v3 recentes).
2. **Strip** todos os arquivos cuja linha TOML `cvss = "CVSS:4.0/` bate com regex `^cvss = "CVSS:4\.0/`.
3. **Safety net**: se N stripado > 50, abortar o job — força re-triage quando upstream eventualmente bulk-migrar para v4.
4. Log explícito de cada arquivo removido — auditor vê exatamente o que ficamos cegos.

Medição atual (2026-04-22 ao meio-dia ET): `39` arquivos CVSS v4 no HEAD da advisory-db.

### 5.3.1 Cobertura que perdemos (deps GarraRUST)

Intersecção entre os 39 arquivos CVSS v4 stripados e nossa `Cargo.lock`:

| Crate | Versão em lock | Ação futura |
|---|---|---|
| cap-primitives | 3.4.5 | Verificar advisories manualmente em B-2+ |
| cmov | 0.5.3 | idem |
| quinn-proto | 0.11.13 | **já listado no ci.yml TODO para bump** (B-2) — dupla-visibilidade |
| tar | 0.4.45 | manual |
| time | 0.3.47 | manual |
| wasmtime | 28.0.1 | **também listado em `.cargo/audit.toml` ignore list** (§5.6 abaixo) com advisories CVSS v3 reais; dupla visibilidade |

Follow-up: após o fix deste PR, Lote B-2 (RUSTSEC bumps) deve incluir revisão manual dos advisories CVSS v4 desses 6 crates para decidir bump ou ignore justificado. Registrado em `GAR-NEW-01` comment + no plano mestre da sessão.

### 5.6 `.cargo/audit.toml` com 13 ignores justificados

Run v3 (`24794271759`) tornou visível o volume real de advisories pendentes: **16 vulnerabilities + 6 denied warnings** em 13 advisory IDs únicos (muitas dups por crate ser usado em múltiplas versões da árvore de deps). Escopo bem maior que a TODO original do `ci.yml` (6 advisories listados).

Estratégia: criar `.cargo/audit.toml` com `[advisories].ignore = [...]` listando todos os 13 IDs, cada um precedido de:

- Nome do crate + versão afetada
- Descrição curta do advisory
- **Rationale de deferimento**: por que não corrigir AGORA (no A-0) e o que B-2 deve fazer
- Expiration coletiva 2026-05-20

Categorias:

1. **Vulnerabilities reais** (9 IDs): idna, rsa, wasmtime, tokio-tar, rustls-webpki ×4.
2. **Unsound** (3 IDs): glib, lru, rand.
3. **Yanked** (1 ID): core2.

**Não é silenciamento — é deferimento rastreável.** O PR que fecha cada advisory em B-2 remove a linha correspondente do arquivo (e o comentário serve como checklist natural). Reviewer entra no arquivo e sabe exatamente o que falta.

Policy in-file documenta requisitos para adicionar novo ignore (referência de ID + rationale + owner). `.cargo/audit.toml` in VCS — não é `.gitignore`d — para garantir reprodutibilidade exata entre dev local e CI.

Alternativa considerada (e rejeitada): usar `severity_threshold = "high"` para filtrar só os mais graves. Rejeitado porque (a) oculta o escopo real, (b) o campo `severity` é nullable em advisories CVSS v3 (muitos advisories nossos listam `Severity: -` porque não estão scored), (c) cria falsa impressão de "audit limpo" sem o trabalho de fix.

### 5.5 `--no-fetch` + `cargo fetch --locked`

`cargo audit --no-fetch` é crítico: sem ele, cargo-audit faz `git pull` no advisory-db automaticamente quando detecta um repo local, **sobrescrevendo nosso strip step**. Verificado empiricamente no código fonte (v0.21.2) do rustsec crate.

Efeito colateral: `--no-fetch` também suprime o refresh do sparse index `~/.cargo/registry/index/`, usado internamente pela verificação de yanked (`--deny yanked`). Sem o refresh, o index contém apenas as deps de `cargo-audit` (baixadas por `cargo install`), não as nossas — daí o erro cascata "No such crate in crates.io index: <nossa-dep>".

Fix: novo step `Prime crates.io index for workspace deps` que executa `cargo fetch --locked` em `$GITHUB_WORKSPACE`. Isso popula o sparse index cache para TODAS as crates da nossa Cargo.lock, satisfazendo o yanked-check sem precisar que o cargo-audit re-fetch o index.

Ordem de steps é crítica:

1. `cargo install cargo-audit`
2. `cargo fetch --locked` **(nosso índice primeiro)**
3. Fetch advisory-db + strip CVSS v4
4. `cargo audit --no-fetch --deny unsound --deny yanked`

Se steps 2 e 3 forem invertidos, funciona igualmente, mas manter essa ordem espelha o fluxo mental "primeiro deps nossas, depois DB externa".

### 5.4 Fetch shallow + regex strip + `rm` seguro

Desde a pivotagem v2, não usamos mais pin por SHA — apenas fetch de HEAD (`git fetch --depth=1 origin main`). A manipulação subsequente:

```bash
mapfile -t stripped < <(grep -rlE '^cvss = "CVSS:4\.0/' crates/ || true)
if [ "${#stripped[@]}" -gt 0 ]; then
  rm -- "${stripped[@]}"
fi
```

Decisões:

- **Regex ancorada** em `^cvss = "CVSS:4\.0/` evita matches espúrios em prose (descrições podem mencionar "CVSS v4" textualmente).
- **`mapfile`** (bash 4+) garante split correto de paths com espaços (zero no advisory-db hoje, mas defesa).
- **`rm --`** força que nenhum filename começando com `-` seja interpretado como flag.
- **`|| true`** na composição com grep evita bash set -euo abortar quando o grep retorna zero matches (ideal quando rustsec eventualmente corrigir).

### 5.5 Flag `--no-fetch` para `cargo audit`

`cargo audit 0.21.2` expõe `-n / --no-fetch` ("do not perform a git fetch on the advisory DB") — verificado no source em `rustsec/rustsec:cargo-audit/v0.21.2/cargo-audit/src/commands/audit.rs`. Sem isso, `cargo audit` detecta o diretório já presente mas **tenta fazer pull** na base clonada, o que sobrescreveria o pin com o HEAD atual quebrado. A flag é o enforcement do pin.

### 5.6 Expiration date (`2026-05-20`)

Pin deve ser temporário, nunca permanente — senão o audit vira teatro ao longo do tempo. Escolhi **4 semanas** (prazo suficiente para rustsec ter tempo de shippar support a CVSS v4 ou para o advisory-db ter downgrade para v3 nos libcrux entries), documentado inline no comentário do workflow e no Linear `GAR-NEW-01`. Revisão obrigatória em 2026-05-20: ou bump do pin para SHA recente se ainda broken, ou remoção do pin se resolvido.

## 6. Testing strategy

### 6.1 Local (Windows + git bash)

- **Fetch HEAD** de advisory-db reproduzido com sucesso em `/tmp/adb-head`.
- **Strip regex** validada: `grep -lrE '^cvss = "CVSS:4\.0/' crates/` retornou 39 arquivos, cada um iniciando com `crates/<pkg>/RUSTSEC-*.md` conforme esperado.
- **Intersecção com Cargo.lock** feita via shell loop: 6 deps presentes (ver §5.3.1).
- **`cargo audit` local pulado:** cargo-audit não está instalado no Windows, e instalá-lo exige compilar várias deps crypto. Custo não justifica vs. `workflow_dispatch` imediato pós-push.

### 6.2 CI (GitHub Actions)

- Pré-merge: `gh workflow run cargo-audit.yml --ref feat/0043-cargo-audit-db-pin` assim que a branch subir, observar resultado via `gh run watch`.
- Pós-merge: próximo scheduled run automático às 07:00 UTC confirma estabilidade (primeira run "de verdade" após o fix).

### 6.3 Verificação que o fix é correto (e não só "verde")

Três sinais no log do job:

- Step `Fetch advisory database and strip CVSS v4 entries` imprime `Advisory DB HEAD: <sha>` + `Commit date: <iso>`.
- Mesmo step imprime `Stripping N CVSS v4 advisories (threshold 50):` seguido da lista completa de arquivos removidos.
- Step `Run cargo audit` **não** exibe `Fetching advisory database from https://github.com/RustSec/advisory-db.git` (`--no-fetch` corta esse caminho).

Se algum dos três falhar, o workflow pode ficar "verde por acidente" (ex: fallback silencioso a algum cache). Todos são gates manuais no review.

## 7. Security review triggers

- **SEC-L (low surface, but mandatory review per CLAUDE.md rule 10)**: modifica workflow de segurança. security-auditor deve validar:
  - SHA não é manipulável (é immutável por design Git).
  - Comentário in-file documenta expiration — evita rot.
  - `--no-fetch` fecha a janela de "HEAD sneak-in".
  - Nenhum secret novo introduzido no workflow.
- **SEC-L (documentação):** pin de 4 semanas é janela aceitável para não perder advisories novos? Resposta: janela de 4 semanas vs. janela de "indefinido red" é trivialmente melhor. O pin expiring em 2026-05-20 força re-triage.

Zero risco HIGH ou MEDIUM esperado.

## 8. Rollback plan

`git revert` do commit. Workflow volta ao estado anterior (fetch HEAD, sem pin) — continuará vermelho até upstream corrigir, mas nenhum outro workflow é afetado. Zero schema change, zero crate mutation.

## 9. Open questions

- Ao expirar (2026-05-20), quem re-triage? Proposta: criar issue com data de expiração agora; usuário owner do Linear GAR responde em 2026-05-20. Documentado no Linear `GAR-NEW-01`.

## 10. Superpowers usage

Conforme plano mestre desta sessão §10: **não usar** Superpowers neste slice. Edit de yaml + 1 plan file é trivial. Skills inline (Read/Edit/Bash/WebFetch + rigor manual) bastam. Esta decisão será reiterada no PR description.

## 11. Linear impact

Ao abrir o PR, criar `GAR-NEW-01` com:

- **Título:** "cargo-audit nightly red: upstream advisory-db schema gap (CVSS v4)"
- **Projeto:** Fase 5 — Qualidade, Segurança & Compliance
- **Labels:** `security`, `ci-hygiene`
- **Priority:** `Urgent`
- **Description:** resumo executivo + link para este plan file + link para o run vermelho + expiration date explícita (2026-05-20).
- **Assignee:** me (current).

Após merge, **fechar** a issue como Done (slice é one-shot, não multi-slice).

## 12. Checklist de execução

- [x] Triagem: ler o log da run vermelha, identificar line 5 column 8.
- [x] Root cause: WebFetch do file `RUSTSEC-2026-0073.md` e identificação do `CVSS:4.0/`.
- [x] Encontrar commit safe: `1dc467507294adffdc8e0a5548d97a58f77d111f`.
- [x] Validar que `--no-fetch` existe em `cargo-audit 0.21.2`.
- [x] Edit `.github/workflows/cargo-audit.yml`.
- [x] Criar plan file (este arquivo).
- [x] Atualizar `plans/README.md`.
- [ ] Commit + push da branch.
- [ ] Criar `GAR-NEW-01` no Linear.
- [ ] `gh workflow run cargo-audit.yml --ref feat/0043-cargo-audit-db-pin`.
- [ ] Se verde: abrir PR com `gh pr create`.
- [ ] Review: dispatch `@security-auditor` + `@code-reviewer`.
- [ ] Merge após approvals + CI verde.
- [ ] Comentar `GAR-NEW-01` com link do PR + fechar.
