# Guia de Migração — File Matching

Este guia descreve as mudanças introduzidas pelo sistema `garraia-glob` (Ciclo 5-6) e como migrar configurações existentes.

---

## O que mudou

Antes do `garraia-glob`, o GarraIA usava o crate `glob` do ecossistema Rust padrão, com:
- Suporte limitado a wildcards (`*`, `**`, `?`)
- Sem suporte a extglob (`!(...)`, `@(...)`, etc.)
- Sem suporte a expansão de chaves `{a,b}`
- Sem integração com `.gitignore` e `.garraignore`
- Sem CLI para testar padrões

Com o `garraia-glob`:
- Motor próprio compilando glob → regex (modo picomatch ou bash)
- Suporte completo a extglob em `.garraignore`
- Scanner unificado com ignore files, dotfiles e limites configuráveis
- CLI `garraia glob test` para testar padrões interativamente
- Watcher integrado com filtro de padrões

---

## Configuração

### Antes (sem seção `fs:`)

```yaml
# config.yml — sem configuração explícita de glob
agent:
  system_prompt: "..."
```

### Depois

```yaml
# config.yml — adicione a seção fs: (opcional, valores padrão são seguros)
fs:
  glob:
    mode: picomatch   # "picomatch" (padrão) ou "bash"
    dot: false        # incluir dotfiles em * e ? (padrão: false)
  ignore:
    use_gitignore: true  # respeitar .gitignore (padrão: true)
```

**A seção `fs:` é completamente opcional.** Se omitida, os padrões são:
- `mode: picomatch`
- `dot: false`
- `use_gitignore: true`

---

## Padrões de glob

### Padrões básicos — sem mudança

```
# Estes funcionam igual ao comportamento anterior
*.rs
**/*.rs
src/*.ts
```

### Expansão de chaves — nova funcionalidade

Antes você precisava de múltiplas entradas:
```
# Antes
**/*.ts
**/*.tsx
**/*.js
```

Agora pode usar:
```
# Depois (equivalente)
**/*.{ts,tsx,js}
```

### Extglob — nova funcionalidade (modo bash ou .garraignore)

```
# Ignorar todo JS exceto arquivos de entrada
!(index|main).js

# Corresponder qualquer arquivo .test. ou .spec.
@(*test*|*spec*).ts
```

---

## Arquivo .garraignore

### Novo arquivo (introduzido no Ciclo 6)

Crie `.garraignore` na raiz do projeto. Ele tem precedência mais alta que `.gitignore` no scanner do GarraIA.

```
# .garraignore — regras específicas do GarraIA
# (não interfere com o git)

# Ignorar arquivos de sessão e vault
*.db
*.db-journal
credentials/
vault.json

# Ignorar scripts temporários de automação
close_*.ps1
list_linear.ps1

# Suporte a extglob — ignorar tudo exceto fontes
# !(src|docs|tests)/    # (requer mode: bash no config)
```

### Diferença entre .gitignore e .garraignore

| Aspecto | `.gitignore` | `.garraignore` |
|---------|-------------|----------------|
| Usado por | git | scanner GarraIA |
| Extglob | não | sim |
| Precedência | menor | maior |
| Afeta git | sim | não |

---

## Comportamento de dotfiles

### Comportamento anterior

O matcher anterior não tinha política explícita para dotfiles — o comportamento dependia do padrão.

### Comportamento novo (padrão)

Com `dot: false` (padrão), `*` e `**` **não** correspondem a caminhos com componente oculto:

```bash
# dot: false (padrão)
garraia glob test "**/*.yml" --dir .
# Não inclui: .github/workflows/ci.yml  ← começa com .

# dot: true (opt-in)
garraia glob test "**/*.yml" --dot --dir .
# Inclui: .github/workflows/ci.yml
```

**Ação necessária:** Se você dependia que `**` correspondesse a diretórios ocultos como `.github/`, adicione `dot: true` no config ou use padrões explícitos:

```
# Alternativa sem dot: true
.github/**/*.yml
.vscode/*.json
```

---

## CLI de teste

Nova ferramenta para validar padrões antes de colocar em produção:

```bash
# Verificar quais arquivos serão incluídos
garraia glob test "src/**/*.rs" --dir .

# Verificar com as regras de ignore do projeto
garraia glob test "**/*.ts" --dir . --no-gitignore

# Saída JSON para scripts CI
garraia glob test "**/*.rs" --json | jq '.matches | length'
```

---

## Checklist de migração

- [ ] Revisar `config.yml` — adicionar seção `fs:` se necessário
- [ ] Verificar se algum padrão `**` precisava de dotfiles (`dot: true`)
- [ ] Criar `.garraignore` na raiz para regras específicas do GarraIA
- [ ] Testar padrões com `garraia glob test` antes de usar em produção
- [ ] Atualizar CI para usar `--no-gitignore` se o scanner do GarraIA rodar em CI
