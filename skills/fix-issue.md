Fix a GitHub issue using TDD for GarraRUST.

Steps:
1. `gh issue view $ARGUMENTS --json number,title,body,labels` — leia o issue
2. Identifique o crate Rust ou módulo Flutter afetado pelo título/body
3. Encontre o código relevante com Grep/Glob
4. Escreva um teste que reproduz o bug (deve falhar — RED)
5. Corrija o código para o teste passar (GREEN)
6. Rode `cargo test -p <crate>` ou `flutter test` — todos devem passar
7. Crie um PR: `gh pr create --title "fix: <titulo>" --body "Fixes #<numero>"`
8. Reporte: issue, arquivo corrigido, teste adicionado, PR criado

Usage: /fix-issue --issue <number> [--repo <owner/repo>]
