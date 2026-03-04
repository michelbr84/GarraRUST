# Glob & File Matching — GarraIA

O GarraIA usa o crate `garraia-glob` para correspondência de caminhos, varredura de diretórios e regras de ignorar. Este documento descreve a semântica completa dos padrões e apresenta exemplos prontos para uso.

---

## Modos de Correspondência

| Modo | Flag config | Comportamento |
|------|------------|---------------|
| `picomatch` *(padrão)* | `fs.glob.mode: picomatch` | Seguro, sem backtracking, POSIX-compatível |
| `bash` | `fs.glob.mode: bash` | Extglob completo (`!(...)`, `@(...)`, etc.) |

Configure em `config.yml`:

```yaml
fs:
  glob:
    mode: picomatch   # ou "bash"
    dot: false        # true = * e ? correspondem a dotfiles
  ignore:
    use_gitignore: true
```

---

## Referência de Padrões

### Wildcards básicos

| Padrão | Corresponde | Não corresponde |
|--------|-------------|-----------------|
| `*` | `main.rs`, `lib.rs` | `src/main.rs` *(barra não atravessa)* |
| `**` | tudo, inclusive `/` | — |
| `**/*.rs` | `src/main.rs`, `crates/foo/src/lib.rs` | `README.md` |
| `?` | qualquer 1 caractere *(exceto `/`)* | — |
| `[abc]` | `a.rs`, `b.rs`, `c.rs` | `d.rs` |
| `[!abc]` | `d.rs`, `x.rs` | `a.rs` |

### Diferença entre `*` e `**`

```
src/
  main.rs
  utils/
    parser.rs
```

| Padrão | Resultado |
|--------|-----------|
| `*.rs` | `main.rs` apenas |
| `**/*.rs` | `main.rs` + `utils/parser.rs` |
| `src/*.rs` | `src/main.rs` apenas |
| `src/**/*.rs` | `src/main.rs` + `src/utils/parser.rs` |

### Expansão de chaves

| Padrão | Corresponde |
|--------|-------------|
| `**/*.{ts,tsx}` | qualquer `.ts` ou `.tsx` em qualquer nível |
| `src/**/*.{js,mjs,cjs}` | todo JS no diretório `src/` |
| `**/{test,spec}/**` | diretórios `test/` ou `spec/` em qualquer lugar |

### Extglob (modo `bash` ou `.garraignore`)

| Padrão | Significado |
|--------|-------------|
| `!(pattern)` | Qualquer coisa que **não** corresponde ao padrão |
| `@(a\|b)` | Exatamente `a` ou `b` |
| `*(pattern)` | Zero ou mais ocorrências |
| `+(pattern)` | Uma ou mais ocorrências |
| `?(pattern)` | Zero ou uma ocorrência |

Exemplo:
```
# .garraignore — ignorar todo JS exceto index.js
!(index).js
```

---

## Exemplos por Linguagem

### Projeto Rust

```
# Ignorar artefatos de build
target/
**/*.rs.bk
Cargo.lock.bak

# Manter apenas fontes
include:
  - src/**/*.rs
  - crates/**/*.rs
  - tests/**/*.rs

exclude:
  - target/
  - **/*.rs.bk
```

### Projeto TypeScript/Node

```
# Ignorar
node_modules/
dist/
build/
**/*.d.ts.map

# Incluir
src/**/*.{ts,tsx}
tests/**/*.spec.ts
```

### Excluir `target/` e `node_modules/` globalmente

```yaml
# .garraignore
target/
node_modules/
.git/
**/__pycache__/
**/*.pyc
```

---

## Regras de Prioridade

1. **Padrões de negação** (`!pattern`) têm prioridade mais alta
2. **Padrões mais específicos** ganham de menos específicos
3. **Última linha** de mesmo nível de especificidade vence
4. **`.garraignore`** pode sobrescrever `.gitignore` (não o contrário)

```
# Exemplo de precedência
*.log           # ignora tudo .log
!error.log      # mas mantém error.log
build/*.log     # ignora .log dentro de build/
!build/keep.log # mas mantém build/keep.log
```

---

## CLI — Testar Padrões

```bash
# Testar um padrão contra um diretório
garraia glob test "**/*.rs" --dir ./src

# Modo bash (extglob)
garraia glob test "!(target)/**/*.rs" --mode bash

# Incluir dotfiles
garraia glob test "**/.env*" --dot

# Saída JSON para scripts
garraia glob test "src/**/*.{ts,tsx}" --json

# Mostrar regex gerado (debug)
garraia glob test "!(*.min).js" --debug-regex
```

---

## Dotfiles

Por padrão `*` e `?` **não** correspondem a arquivos ou diretórios cujo nome começa com `.`:

| Padrão | `dot: false` | `dot: true` |
|--------|-------------|------------|
| `*` | ignora `.env` | inclui `.env` |
| `**/*` | ignora `src/.hidden` | inclui `src/.hidden` |
| `.*` | sempre corresponde | sempre corresponde |

Configure globalmente com `fs.glob.dot: true` ou por chamada no `garraia glob test --dot`.

---

## API HTTP — Live Tester

```http
POST /admin/api/config/glob/test
Content-Type: application/json

{
  "pattern": "**/*.rs",
  "paths": ["src/main.rs", "README.md", "crates/foo/lib.rs"],
  "mode": "picomatch",
  "dot": false
}
```

Resposta:
```json
{
  "matches": ["src/main.rs", "crates/foo/lib.rs"],
  "total": 3,
  "matched": 2
}
```
