# Plan 0035 — GAR-379: `garraia config check` (slice 1 — validation + precedence report)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) (parcial — slice 1 de N)
**Branch:** `feat/0035-gar-379-cli-config-check`
**Unblocks:** nada (melhora DX; destrava triagem de misconfig em produção/dev)

---

## 1. Goal

Entregar o menor slice mergeável do GAR-379: um comando `garraia config check` que
carrega a configuração unificada (já produzida pelo `garraia-config`) e reporta:

1. **Precedência** — quais fontes foram lidas (defaults → arquivo → env vars),
   com *path* do arquivo e nome das env vars detectadas.
2. **Validação estruturada** — regras de negócio executadas sobre o
   `AppConfig` (ranges, campos mutuamente exclusivos, paths obrigatórios
   quando outra opção está ligada), agregando *todas* as violações antes de
   retornar, em vez de falhar no primeiro erro.
3. **Exit codes corretos** — `0` quando tudo ok, `2` quando validação falha,
   `65` quando o arquivo está corrompido/ilegível (seguindo sysexits
   `EX_DATAERR`).

Output humano por padrão, `--json` machine-readable para CI e scripts.

## 2. Non-goals

- **Não** refatora os leitores existentes de env/arquivo nos outros crates.
  O roteiro completo do GAR-379 (unificar *todas* as fontes) é escopo de
  slices posteriores — este slice só adiciona a ferramenta de diagnóstico
  que precisa existir antes do refactor maior.
- **Não** adiciona `ConfigBuilder` novo. `ConfigLoader` já faz load; o check
  reusa.
- **Não** toca em `AuthConfig` (GAR-391c já validou com `validator`).
- **Não** adiciona CLI flags para override (`--set key=value`) — fora do slice.
- **Não** altera nenhuma rota, nenhum schema Postgres, nenhum endpoint REST.

## 3. Scope

**Arquivos novos:**

- `crates/garraia-config/src/check.rs` — módulo com:
  - `pub struct ConfigCheck { pub source: SourceReport, pub findings: Vec<Finding>, pub summary: ConfigSummary }`
  - `pub struct SourceReport { pub config_dir: PathBuf, pub file_used: Option<PathBuf>, pub used_defaults: bool, pub env_vars_detected: Vec<String>, pub mcp_json_present: bool }`
  - `pub struct ConfigSummary { ... }` — snapshot redigido do `AppConfig` efetivo (só presença para segredos)
  - `pub struct Finding { pub severity: Severity, pub field: String, pub message: String }`
  - `pub enum Severity { Error, Warning }`
  - `pub fn run_check(loader: &ConfigLoader, config: &AppConfig) -> ConfigCheck`
- `crates/garraia-cli/src/config_cmd.rs` — handler `run_config_check(...)`
  que formata `ConfigCheck` como output humano (ansi-free; stderr friendly) ou JSON via `--json`.
- `plans/0035-gar-379-cli-config-check.md` — este arquivo.

**Arquivos modificados:**

- `crates/garraia-config/src/lib.rs` — re-export `check::{ConfigCheck, Finding, Severity, SourceReport, run_check}`.
- `crates/garraia-cli/src/main.rs` — novo `Commands::Config { action: ConfigCommands::Check { json, strict } }`.
- `plans/README.md` — entrada 0035.
- `CLAUDE.md` — menção breve de que `garraia config check` existe (seção skills/ferramentas).

**Testes novos:**

- `crates/garraia-config/src/check.rs` `#[cfg(test)]` — 5+ unit tests cobrindo:
  - precedence detecta YAML presente
  - precedence detecta TOML quando YAML ausente
  - precedence detecta "defaults only"
  - findings capturam `session_ttl_secs <= 0`
  - findings capturam TLS parcial (só cert *ou* key)
  - findings capturam `session_idle_secs > session_ttl_secs` (idle não faz sentido maior que absolute TTL)
  - findings capturam `rate_limit.burst_size == 0`
  - findings capturam `voice.enabled` sem endpoint (warning, não erro)
  - `Severity::Error` faz exit code 2; só `Severity::Warning` mantém exit 0 no modo default, 2 no modo `--strict`.

## 4. Acceptance criteria

1. `cargo check -p garraia-config -p garraia` verde.
2. `cargo clippy -p garraia-config -p garraia -- -D warnings` verde.
3. `cargo test -p garraia-config` verde (todos os testes novos passam).
4. `garraia config check` carrega a config default (sem arquivo) e sai com 0
   imprimindo o source report.
5. `garraia config check --json` imprime um JSON `{"source":{...},"findings":[...],"exit_code":N}`
   determinístico.
6. Config intencionalmente inválida (`session_ttl_secs = 0`) provoca exit 2 e
   lista a violação por *field path* humano.
7. Arquivo corrompido (`config.yml` com YAML inválido) provoca exit 65 com
   mensagem apontando arquivo + parse error.
8. `@code-reviewer` APPROVE.
9. `@security-auditor` APPROVE (slice não toca secret; verificar que
   o report *não* imprime valores de `api_key`, tokens ou DB URLs se
   houver).
10. PR mergeado em `main` com CI 9/9 green.

## 5. Design rationale

### 5.1 Precedência declarada, não inferida

O `ConfigLoader::load` já decide: YAML > TOML > defaults. O check reusa o
mesmo *loader* e só expõe o caminho que foi tomado. Não duplicamos a lógica
de precedência; apenas reportamos.

### 5.2 Agregar findings, não falhar no primeiro

Semanticamente, `config check` é diagnóstico. Quem roda quer ver *todos* os
problemas de uma vez, não um-por-vez-cada-re-run. A API `run_check` retorna
um `Vec<Finding>` para preservar essa propriedade.

### 5.3 Exit codes sysexits

- `0` — tudo ok.
- `2` — violação (regras de validação falharam).
- `65` (`EX_DATAERR`) — arquivo existe mas não parseia.

Isso alinha com o padrão já usado no plan 0034 (migração workspace) e
facilita uso em CI (`if garraia config check; then ...`).

### 5.4 Redaction

O JSON output *nunca* imprime:
- `gateway.api_key`
- `llm.*.api_key`
- `embeddings.*.api_key`
- conteúdo das env vars sensíveis (só o *nome* da env, não o valor)

A regra é: campos que podem conter segredos são reportados por presença
(`"api_key_set": true`), não por valor.

### 5.5 `--strict` flag

`--strict` promove `Warning` a `Error` para uso em CI. Sem a flag, warnings
não alteram exit code. Isso permite DX leve em desenvolvimento + rigor em CI.

## 6. Testing strategy

- **Unit tests** em `check.rs` para as 8 regras + precedence detection (in-process, usa `TempDir`).
- **Manual acceptance** — rodar `cargo run -p garraia -- config check` contra
  o próprio diretório do projeto para sanity-check; anexar output ao PR.
- **Integration CI** — o job `cargo test --workspace` já roda todos os crates;
  nenhum job novo necessário.

## 7. Security review triggers

- Verificar que API keys e DB URLs **nunca** aparecem no output humano ou no
  JSON.
- Verificar que `--json` não escapa errado o path do config (ex. se usuário
  aponta `GARRAIA_CONFIG_DIR` para um valor malicioso com `"` ou `\n`).

## 8. Rollback plan

Reversível via `git revert <merge-commit>`. O slice só adiciona código — não
altera schema, não altera comportamento de nenhum handler existente, não
mexe em config de produção. Revert é seguro em qualquer momento.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Imprimir API key acidentalmente | HIGH | Redaction explícita na serialização; unit test específico garante redaction. |
| Regra de validação false-positive | MEDIUM | Regras são conservadoras (ranges largos); warnings para heurísticas, errors só para invariantes violados. |
| Exit code diferente do sysexits | LOW | Documentado em `§5.3`; matches STRIDE T-CODE (CI relies on exit). |

## 10. Open questions

Nenhuma. Slice é cirúrgico.

## 11. Future work (fora deste slice)

- Slice 2: `garraia config diff --from file.yml --to env` (mostrar diff entre fontes).
- Slice 3: `garraia config explain <field>` (docs inline de cada campo).
- Slice 4: refactor dos leitores diretos de env em outros crates para passar
  pelo builder unificado.
- Slice 5: `garraia config schema --format json` (exportar JSON Schema).

Esses slices podem ser planejados depois que o `check` estiver em produção e
houver feedback real sobre o que falta.

## 12. Definition of done

- [x] Plan mergeado.
- [ ] Código implementado.
- [ ] Testes verdes locally + CI.
- [ ] Code review aprovado.
- [ ] Security audit aprovado (slice é baixo-risco mas redaction precisa ser verificada).
- [ ] PR mergeado em `main`.
- [ ] Linear GAR-379 **comentado** (NÃO fechado — issue continua aberta; slice 1 de N).
- [ ] `plans/README.md` atualizado.
- [ ] `.garra-estado.md` atualizado ao final da sessão.
