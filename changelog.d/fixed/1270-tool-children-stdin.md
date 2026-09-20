- **Filhos de `run_tests` e `bash` nao herdam mais o stdin do gateway
  (#1270).** A varredura sistematica de argv injection que a #1270 pedia
  (sign-off do PR #1268) inventariou as 14 chamadas de `std::process::Command`
  nas tools e achou um achado novo da mesma familia que o #1269 fechou no
  `git_diff`/`code_review`: o filho de `repo_search`, `git_diff` e
  `code_review` ja rodava com stdin fechado, mas o de `run_tests` (nos cinco
  frameworks) e o do `bash` herdavam o stdin do processo do gateway — em
  terminal, um `cat` sem argumento no comando rouba o que o operador digitou,
  e o comportamento dependia de como o processo subiu (`garra start`, systemd,
  sidecar, container). Agora os dois fecham o stdin (`Stdio::null()`), com
  guard que varre o fonte (padrao do `spinner.rs`) e o inventario completo
  das tools no threat-model §5.72. Zero achados de argv injection: todas as
  tools que montam linha de comando ja seguem o padrao do #1268 (terminador
  `--` ou valor colado na opcao).
