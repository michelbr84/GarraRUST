Run pre-commit validation before pushing GarraRUST changes.

Steps:
1. `git diff --cached --name-only` — lista arquivos staged
2. **Segredos** — busca por padrões perigosos nos arquivos staged:
   - `GARRAIA_JWT_SECRET`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `.env` commitado
   - Se encontrar: BLOQUEIO — desstagie antes de continuar
3. **Debug artifacts** — busca por `dbg!`, `println!`, `console.log`, `print(` em código Rust/Dart staged
4. **Arquivos grandes** — avisa se algum arquivo staged > 1MB
5. **Cargo check** — `cargo check --workspace --quiet` — deve passar sem erros
6. **Flutter analyze** — se há .dart staged: `flutter analyze apps/garraia-mobile/ --no-pub`
7. **Mensagem de commit** — gera sugestão no formato Conventional Commits:
   `<type>(<scope>): <descrição>`
   tipos: feat, fix, chore, refactor, test, docs
8. Reporte: ✓ passou / ✗ bloqueado + motivo

Usage: /pre-commit
