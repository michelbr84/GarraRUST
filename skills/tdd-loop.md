Implement a feature using TDD (Red-Green-Refactor) for GarraRUST.

Input: specification in plain English + file path to implement

Steps:
1. **Entender** — leia o arquivo alvo e os testes existentes no mesmo crate
2. **RED** — escreva o teste que falha:
   - Rust: adicione `#[test]` ou `#[tokio::test]` no módulo `tests` do arquivo
   - Flutter: crie/atualize `test/<feature>_test.dart`
3. **Verificar RED** — `cargo test -p <crate> <test_name> 2>&1 | tail -5` deve mostrar FAILED
4. **GREEN** — implemente o mínimo necessário para o teste passar
5. **Verificar GREEN** — rode o teste novamente, deve mostrar ok
6. **REFACTOR** — melhore sem quebrar: extraia funções, remova duplicação, melhore nomes
7. **Verificar REFACTOR** — `cargo test -p <crate>` todos os testes devem passar
8. **Flutter** — se arquivo .dart: `flutter test` no diretório apps/garraia-mobile/

Regras:
- Máximo 10 iterações Red→Green
- Não escreva mais código do que o necessário para o teste passar
- Se o teste não passar em 3 tentativas, pare e reporte o bloqueio

Usage: /tdd-loop <especificação> --file <caminho>
