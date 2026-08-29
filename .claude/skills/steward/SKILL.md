---
name: steward
description: Guia específica do GarraRUST para dirigir um PR até o verde — quais falhas de CI são ambientais, quais são suas, e o que validar antes de empurrar. Lida ao reagir a eventos de CI ou de review num PR.
---

# Steward — dirigindo um PR do GarraRUST até o verde

Esta skill é lida **no momento em que um PR seu fica vermelho ou recebe
review**. Ela não repete o `CLAUDE.md` (convenções de código, regras absolutas,
estrutura de crates) — vá lá para isso. Aqui está só o que morde nesse momento
específico, e que já custou ciclos de CI reais.

Ela **não** autoriza aprovar nem mergear PR, e não sobrepõe nenhuma regra
"nunca" da política de PR do harness (pular/desabilitar teste, reescrever
histórico de branch alheia, commit vazio para chacoalhar CI).

---

## 1. Antes de qualquer push

O `Format Check` é **bloqueante** e é o erro mais barato e mais frequente:

```bash
cargo fmt --all && cargo fmt --all -- --check
```

Rode isso **sempre**, mesmo em mudança que "só mexe em comentário". `cargo
check`, `clippy` e `test` passando **não** implicam fmt limpo — o rustfmt
reflui `params!`, `assert!`, `match` longos e reordena blocos `use`, e nada
disso aparece nas outras ferramentas.

> Caso real: PR #861 foi para o CI com 21 de 22 checks verdes e o único
> vermelho era fmt — 19 blocos em 8 arquivos, todos cosméticos. Um ciclo
> inteiro de CI (~25 min) perdido.

Depois, o mais próximo possível do que o CI roda:

```bash
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo test  --workspace --exclude garraia-desktop
```

`garraia-desktop` é excluído em **todos** os jobs (clippy, build, test, msrv) —
faltam libs GTK e o sidecar Windows nos runners. Não tente "consertar" isso.

---

## 2. Falhas que NÃO são suas

Antes de investigar como defeito, descarte estas. Todas aparecem como falha
legítima e não são.

### `migration_smoke` — "failed to initialize a docker client"

```
Error: failed to initialize a docker client: Socket not found: /var/run/docker.sock
```

Os testes de `garraia-workspace` (e os de integração de `garraia-auth` /
`garraia-gateway`) usam testcontainers. **O container de dev não tem Docker.**
Nos runners Linux do GitHub tem, e lá passam — inclusive aplicando todas as
migrations via `sqlx::migrate!` em Postgres real. Se falhar só localmente com
essa mensagem, é ambiente. Diga isso explicitamente em vez de alegar que
validou.

### `utoipa-swagger-ui` — build script falha localmente

O `build.rs` baixa um zip do GitHub, e o proxy de egresso desta sessão bloqueia
o domínio; o "zip" gravado é uma página de erro JSON de 378 bytes. O `ci.yml`
já contorna pré-baixando com curl.

Para destravar localmente, **não apague o cache cegamente** (`find target -name
'v5.17.14.zip' -delete` remove também cópias válidas de builds anteriores —
erro já cometido). Gere o arquivo a partir do clone da tag:

```bash
git clone --depth 1 --branch v5.17.14 https://github.com/swagger-api/swagger-ui /tmp/swui
git -C /tmp/swui archive --format=zip --prefix=swagger-ui-5.17.14/ HEAD -o /tmp/v5.17.14.zip
export SWAGGER_UI_DOWNLOAD_URL="file:///tmp/v5.17.14.zip"
```

### "No space left on device" disfarçado de erro de compilação

`target/` chega a ~26 GB e o disco da sessão é uma cota fixa — `df` engana
(mostra "Avail" baixo com "Used" baixo). Sintoma típico: vários `could not
compile X` sem erro de tipo algum, e no meio um `error 28`. Cure com
`target/debug/incremental` primeiro (metade do peso), `cargo clean` se
necessário. **Commite e empurre antes de limpar**, para que um estouro não leve
o trabalho junto.

### Quality Ratchet reclamando de arquivos grandes

O baseline está congelado em 2026-05-05 e a `main` andou muito desde então.
As regressões de `max_file_lines` / `files_over_*` quase sempre são **deriva do
baseline, não suas**. Confirme antes de agir:

```bash
git diff --name-only origin/main..HEAD          # o arquivo apontado está aqui?
git show origin/main:<arquivo> | wc -l          # já tinha esse tamanho na main?
```

O modo é `report-only` (exit 0) e o check fica verde. **Nunca** edite
`.quality/baseline.json` à mão — é fraude; use `freeze-baseline.py`.

### CodeQL agregado em `neutral`

O check-run agregado "CodeQL" às vezes fecha como `neutral` enquanto os três
`Analyze (rust|actions|javascript-typescript)` fecham `success`. `neutral` não
é falha e não bloqueia. Olhe os três individuais.

---

## 3. Bump do wasmtime: o piso de MSRV vem junto

Já aconteceu **quatro vezes** (1.92, 1.93, 1.94, 1.95). Um PR do Dependabot que
sobe `wasmtime` costuma trazer **dois** bloqueios independentes, e o segundo
não aparece se você ler só o erro de compilação:

1. quebra de API em `crates/garraia-plugins/src/runtime.rs`;
2. `MSRV check` falhando **antes de compilar**, porque a árvore
   `wasmtime`/`cranelift-*`/`pulley-*`/`wiggle-*` declara um `rust-version`
   novo.

Corrigir só o código deixa o job de MSRV vermelho. Subir o piso toca
`Cargo.toml` (`rust-version`) **e** o `ci.yml` (nome do job, `dtolnay/rust-
toolchain@X.YZ`, e as duas invocações `cargo +X.YZ`). Prove nos dois sentidos:
o novo piso passa `--locked`, o antigo ainda recusa.

Não confie na sugestão do compilador em migração de API do `wasmtime-wasi`: no
bump 47→48 ele mandava trocar só `FilePerms` por `FsPerms` e **manter**
`DirPerms`, o que não compilaria.

---

## 4. Auto-merge está armado neste repositório

Ele dispara assim que os checks **obrigatórios** passam — o que pode ser
**antes** de todos os checks terminarem, e antes de você chamar merge.

Consequências práticas:

- **Nunca afirme que você mergeou** sem conferir `merged_at` e `merged_by`.
  Uma chamada de merge sobre PR já mergeado retorna "successfully merged" de
  forma idempotente, e o `commit_title` que você passou é ignorado.
- Por isso, **arrume o título do PR antes de ele ficar verde**. O squash herda
  o título do PR, e o auto-merge usa o valor em cache. Um título autogerado
  (`Claude/foo-bar-abc123`) vira assunto de commit na `main`, fora de
  Conventional Commits — e corrigir depois exigiria reescrever a `main`, o que
  é proibido.

---

## 5. O CI não roda em branch solta

`ci.yml` dispara em `push` para `main`/`develop`/`master` e em `pull_request`
para esses alvos. Empurrar uma branch **não** roda nada: sem PR aberto, não há
validação de CI. Planeje isso — e não peça para o usuário "esperar o CI" de uma
branch que não tem PR.

---

## 6. Ao reagir a um review

Achado de bot é relatório de defeito: verifique e corrija se for pequeno e
local. Se os achados pararem de convergir (cada correção gera um novo), pare de
empurrar e levante o assunto uma vez, dizendo o que continua sinalizado.

Pedido grande de revisor humano em PR que não é seu → responda com a proposta,
não empurre nem resolva a thread.

Mudança que toca segurança, auth, storage, RLS, secrets ou CI crítico: o
`CLAUDE.md` manda acionar os agents `security-auditor` e `code-reviewer` antes
de seguir.
