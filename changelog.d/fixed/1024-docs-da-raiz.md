- **Os documentos da raiz contradiziam o produto em cinco pontos, conferidos
  contra o binario e o codigo.** Os dois READMEs diziam que um subcomando
  `garra memory` "esta no roadmap" — ele existe desde o #950/#953, com dez
  subcomandos, e ganhou o `add` no #958. O `README.pt-BR` ainda atribuia a API
  do gateway operacoes de "adicionar" e "exportar" que ela nao tem: as rotas
  reais sao `GET /api/memory/recent`, `GET /api/memory/search` e
  `DELETE /api/memory`, tres e nao cinco.
- **O `AGENTS.md` declarava `rust-version = "1.94"`;** o `Cargo.toml` diz
  `1.95`. E mandava rodar `cargo test --workspace` sem excluir o
  `garraia-desktop`, cujo `build.rs` precisa de GTK/glib e de um sidecar do
  Windows que uma maquina limpa nao tem — seguir o arquivo ao pe da letra
  falhava em algo que nao era a mudanca de quem seguia. O `CONTRIBUTING.md`
  repetia o mesmo comando.
- **O `CONTRIBUTING.md` nao mencionava o `changelog.d/`,** que e obrigatorio em
  todo PR desde o #973 e existe justamente para dois PRs paralelos nao
  colidirem na secao `[Unreleased]`. Ganhou passo proprio, com a razao junto.
- A tabela de "Politicas de Ferramentas" dos modos era **aspiracional** ate a
  v0.3.9: a `ToolPolicy` era declarada e nunca verificada no executor (#988).
  Os dois READMEs agora dizem desde quando ela vale, e o que valia antes.
- `ROADMAP.md` e `TODO.md` reancorados na v0.3.9 e em zero issues abertas; o
  `CLAUDE.md` registra o invariante novo do corpo de release e a armadilha de
  `.gitignore` sem ancora que engoliu o `scripts/release/notes.py`.
