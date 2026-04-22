# Plan 0043 — CI hygiene: pin RustSec advisory-db SHA (cargo-audit scheduled fix)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma 2026-04-22, ClaudeMaxPower + Superpowers skills inline)
**Data:** 2026-04-22 (America/New_York)
**Issues:** nenhuma ainda — `GAR-NEW-01` a ser criada no Linear quando este PR abrir (ver §11).
**Branch:** `feat/0043-cargo-audit-db-pin`
**Pré-requisitos:** nenhum.
**Unblocks:** nada no caminho funcional; destrava apenas o sinal de segurança nightly, que é pré-condição para confiar em CI nos próximos slices do Lote A/B.

---

## 1. Goal

Restaurar o workflow scheduled `Security — cargo audit` ao verde sem esperar 24–72h por correção upstream. Fazer isso **pinando a RustSec advisory database a um SHA conhecidamente bom** e documentando critério de expiração do pin para que a pinagem não vire bit-rot.

## 2. Non-goals

- **Não** resolver os 6 RUSTSEC advisories conhecidos do próprio repositório (idna 0.5.0, quinn-proto 0.11.13, rsa 0.9.10, rustls-webpki×2, core2 0.4.0 yanked) — isso é Lote B-2 (`chore: bump crypto deps`).
- **Não** tocar o `security` step inline do `ci.yml` (também mascarado com `continue-on-error`) — esse faz parte do mesmo Lote B-2.
- **Não** modificar `cargo-audit` version pin (continua `^0.21`).
- **Não** remover `crossbeam-channel` do lockfile — versão atual `0.5.15` já é não-yanked (verificado via crates.io API durante triagem).

## 3. Scope

**Arquivos modificados:**

- `.github/workflows/cargo-audit.yml` — novo env `ADVISORY_DB_SHA`, novo step "Fetch pinned advisory database", step "Run cargo audit" adiciona `--no-fetch`.
- `plans/0043-ci-cargo-audit-db-pin.md` (este).
- `plans/README.md` — entrada 0043 (inserida antes de 0042 mantendo ordem narrativa cronológica da sessão).

Zero alteração em `Cargo.toml`, `Cargo.lock`, crates de runtime, testes ou docs fora destes 3 arquivos.

## 4. Acceptance criteria

1. Manual `gh workflow run cargo-audit.yml --ref feat/0043-cargo-audit-db-pin` retorna **success**.
2. Após merge, o scheduled run `0 7 * * *` do dia seguinte também retorna **success**.
3. Log do job exibe linha `Advisory DB pinned to: 1dc46750` (ou equivalente) confirmando que o fetch do SHA correto aconteceu.
4. `cargo audit` não faz fetch adicional (verificável pela ausência da mensagem `Fetching advisory database from https://github.com/RustSec/advisory-db.git`).
5. Zero novo `continue-on-error: true` introduzido.
6. `@security-auditor` APPROVE (cerimônia obrigatória CLAUDE.md regra 10 — workflow + security surface).
7. `@code-reviewer` APPROVE.
8. Comentário em `GAR-NEW-01` do Linear com link para o PR.
9. Plan file existe e está linkado no `plans/README.md`.

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

### 5.3 Escolha do SHA

Critérios para o pin:

1. **Posterior** ao último verde conhecido (maximizar cobertura de advisories recentes).
2. **Anterior** ao primeiro advisory com CVSS v4 (evitar o parse error).
3. **Commit determinístico** (não uma tag, que pode ser force-moved).
4. Texto do commit legível — dá contexto em `git log -1` nos logs de CI.

Pesquisa via `gh api repos/rustsec/advisory-db/commits` com filtros de data:

- Primeiro commit introduzindo `cvss = "CVSS:4.0/..."`: `410feb087a8f` / `dd4ff9f3ff4c` / `543c3046fe61` às `2026-03-24T08:19:00Z` (libcrux-ml-dsa placeholders).
- Último commit imediatamente anterior: **`1dc467507294adffdc8e0a5548d97a58f77d111f`** às `2026-03-24T08:16:07Z` (*"Assigned RUSTSEC-2026-0069 to hpke-rs, RUSTSEC-2026-0070 to hpke-rs, RUSTSEC-2026-0071 to hpke-rs, RUSTSEC-2026-0072 to hpke-rs-rust-crypto"*).

Verificação explícita: `gh api repos/rustsec/advisory-db/git/trees/1dc467507294?recursive=1` **não** lista `crates/libcrux-poly1305/*`. Fetch local do SHA (`git fetch --depth=1 origin 1dc4675...`) também confirma ausência do diretório offending. **SHA escolhido: `1dc467507294adffdc8e0a5548d97a58f77d111f`.**

### 5.4 Por que usar `git fetch --depth=1 <sha>` e não `git clone`

`git clone --depth=1 <url>` só funciona com branches/tags (HEAD rastreado). Para checkout de um SHA arbitrário antigo com shallow clone, o caminho oficial Git é:

```bash
git init
git remote add origin <url>
git fetch --depth=1 origin <full-sha>  # GitHub permite via uploadpack.allowReachableSHA1InWant
git checkout FETCH_HEAD
```

Testado localmente (2026-04-22) contra `github.com/rustsec/advisory-db` — funciona para o SHA completo (40 chars). Short SHAs (12 chars) **não funcionam** nesse fetch path (GitHub retorna `fatal: couldn't find remote ref`). Por isso a var `ADVISORY_DB_SHA` no workflow usa o SHA completo.

### 5.5 Flag `--no-fetch` para `cargo audit`

`cargo audit 0.21.2` expõe `-n / --no-fetch` ("do not perform a git fetch on the advisory DB") — verificado no source em `rustsec/rustsec:cargo-audit/v0.21.2/cargo-audit/src/commands/audit.rs`. Sem isso, `cargo audit` detecta o diretório já presente mas **tenta fazer pull** na base clonada, o que sobrescreveria o pin com o HEAD atual quebrado. A flag é o enforcement do pin.

### 5.6 Expiration date (`2026-05-20`)

Pin deve ser temporário, nunca permanente — senão o audit vira teatro ao longo do tempo. Escolhi **4 semanas** (prazo suficiente para rustsec ter tempo de shippar support a CVSS v4 ou para o advisory-db ter downgrade para v3 nos libcrux entries), documentado inline no comentário do workflow e no Linear `GAR-NEW-01`. Revisão obrigatória em 2026-05-20: ou bump do pin para SHA recente se ainda broken, ou remoção do pin se resolvido.

## 6. Testing strategy

### 6.1 Local (Windows + git bash)

- Executado `rm -rf /tmp/adb-test && mkdir -p /tmp/adb-test && cd /tmp/adb-test && git init --quiet && git remote add origin https://github.com/rustsec/advisory-db.git && git fetch --depth=1 --quiet origin 1dc467507294adffdc8e0a5548d97a58f77d111f && git checkout --quiet FETCH_HEAD` — SUCCESS.
- `ls crates/libcrux-poly1305/ || echo "ausente"` → **ausente** (confirma SHA safe).
- `git log -1 --oneline` → `1dc4675 Assigned RUSTSEC-2026-0069 to hpke-rs, ...` — commit message esperado.
- `cargo audit` local pulado (cargo-audit não está instalado no Windows, e instalá-lo exige compilar várias deps crypto — custo não justifica vs. `workflow_dispatch` imediato pós-PR open).

### 6.2 CI (GitHub Actions)

- Pré-merge: `gh workflow run cargo-audit.yml --ref feat/0043-cargo-audit-db-pin` assim que a branch subir, observar resultado via `gh run watch`.
- Pós-merge: próximo scheduled run automático às 07:00 UTC confirma estabilidade (primeira run "de verdade" após o fix).

### 6.3 Verificação que o fix é correto (e não só "verde")

Dois sinais no log do job:

- Step `Fetch pinned advisory database` imprime `Advisory DB pinned to: 1dc46750` e `Commit date: 2026-03-24T08:16:07+00:00`.
- Step `Run cargo audit` **não** exibe `Fetching advisory database from https://github.com/RustSec/advisory-db.git` (`--no-fetch` corta esse caminho).

Se algum dos dois falhar, o workflow pode ficar "verde por acidente" (ex: fallback silencioso a algum cache). Ambos são gates manuais no review.

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
