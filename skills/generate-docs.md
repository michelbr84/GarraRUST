---
name: generate-docs
description: Gera documentação automática para crates Rust e módulos Flutter do GarraRUST. Produz doc comments, README de crate e índice de API.
---

# Generate Docs

Gera documentação automática para o GarraRUST.

## Steps

1. **Descobrir fontes** — Identifique arquivos `.rs` ou `.dart` no diretório alvo
   - Rust: `Glob("crates/<crate>/src/**/*.rs")`
   - Flutter: `Glob("apps/garraia-mobile/lib/**/*.dart")`

2. **Extrair API pública** — Para cada arquivo:
   - Rust: funções `pub fn`, `pub struct`, `pub enum`, `pub trait`, `impl` blocks
   - Flutter: classes, métodos públicos, providers
   - Ignorar: `#[cfg(test)]` modules, `_private` items

3. **Gerar doc comments** — Adicionar documentação onde faltam:
   - Rust: `///` para items públicos, `//!` para módulos
   - Flutter: `///` dartdoc para classes e métodos
   - Estilo: frase inicial descritiva, parâmetros, retorno, erros possíveis
   - **Não alterar** doc comments existentes (apenas adicionar onde faltam)

4. **Gerar README de crate** — Se não existir `crates/<crate>/README.md`:
   ```
   # <crate-name>

   <descrição extraída de Cargo.toml>

   ## Dependências
   ...

   ## API Pública
   ...

   ## Exemplos
   ...
   ```

5. **Atualizar índice** — Criar/atualizar `docs/api/README.md` com links para cada crate

6. **Verificar** — `cargo doc --no-deps -p <crate>` deve compilar sem warnings

## Regras

- Linguagem: EN para doc comments Rust, PT-BR para READMEs internos
- Nunca usar placeholders (`<TODO>`, `...`)
- Nunca documentar internals privados
- Manter doc comments concisos (1-3 linhas para funções simples)
- Usar o agent `doc-writer` para documentação mais complexa

Usage: /generate-docs [--dir <path>] [--crate <name>]
