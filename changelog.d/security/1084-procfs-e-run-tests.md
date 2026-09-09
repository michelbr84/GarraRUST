- O canal do procfs esta fechado: o processo `garra` passa a rodar
  `prctl(PR_SET_DUMPABLE, 0)` no inicio do `main`, e o kernel trata o
  `/proc/<pid>/environ` dele como root-only. Ate aqui um filho de tool de mesmo
  UID lia o ambiente do pai direto do procfs e alcancava `GARRAIA_JWT_SECRET`,
  `ANTHROPIC_API_KEY` e companhia — o scrub de ambiente do #1075 fechava a
  heranca, nao esse caminho. Medido com controle: sem o fix o filho le o
  segredo, com o fix recebe `EACCES`. Linux e Android; macOS e Windows nao tem
  esse canal. Nao e fail-closed de proposito — num kernel sem o knob o processo
  avisa e segue, porque nao subir o gateway seria trocar um vazamento estreito
  por indisponibilidade total (#1084, ADR 0019).
- O `test_name` do `run_tests` passa a ser validado antes de virar argumento do
  runner. Vinha do modelo e ia cru: `cargo test --config
  'target.<cfg>.runner=...'` aponta um executor de target, que e execucao
  arbitraria. A forma documentada `-p <crate>` segue aceita (#1084).
