- Sem canal de confirmacao, o `run_tests` deixa de ser bloqueado
  incondicionalmente e passa a seguir a **mesma regra do `bash`**, aplicada a
  linha de comando que vai rodar de verdade. O bloqueio antigo nao protegia
  nada: no mesmo runtime `bash("cargo test")` roda, porque `cargo test` nao e
  comando sensivel no gate — era a mesma capacidade por outra porta, com o
  custo de deixar a ferramenta inutil no caminho full-auto. Agora `cargo test`
  e `npm test` rodam, e `pytest` continua bloqueado, porque roda por um
  interpretador Python que o gate trata como codigo arbitrario. Nenhuma
  capacidade nova e concedida. Com canal de confirmacao nada muda: toda suite
  continua pedindo aprovacao vinculada ao diretorio (#1084).
