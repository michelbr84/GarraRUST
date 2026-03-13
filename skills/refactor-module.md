Safely refactor a Rust crate or Flutter module in GarraRUST.

Steps:
1. Leia o arquivo alvo e entenda a responsabilidade atual
2. Rode os testes existentes como baseline:
   - Rust: `cargo test -p <crate> 2>&1 | tail -10`
   - Flutter: `flutter test apps/garraia-mobile/ 2>&1 | tail -10`
3. Planeje as mudanças em 3-5 bullet points (objetivo declarado)
4. Execute as mudanças — uma de cada vez, verificando que não quebra
5. Após cada mudança significativa: rode os testes novamente
6. Se algum teste quebrar: reverta a última mudança e reporte
7. Atualize doc comments se a API pública mudou
8. Reporte: o que mudou, testes antes/depois, surface pública alterada (se houver)

Regras:
- Não mude comportamento observável — apenas estrutura interna
- Mantenha compatibilidade com AppState, SessionStore e AgentRuntime
- Se refatorando handler Axum: verifique que as rotas continuam iguais em router.rs

Usage: /refactor-module --file <caminho> [--goal <descrição>]
