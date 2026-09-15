- **Testes de PDF do `garraia-media` voltam a rodar (#1208).** Cinco testes
  estavam `#[ignore]`d desde 2026-04-15, atribuidos a "lopdf version drift".
  A causa era outra: a fixture `create_test_pdf` escrevia bytes de PDF na mao
  com offsets de `xref` errados, e esses bytes nao carregam em nenhuma versao
  do lopdf. Toda fixture passa a sair do writer do proprio lopdf, que ja era
  usado pelo teste de smoke. `test_extract_page_range_invalid` passava **pelo
  motivo errado** — como a fixture nao carregava, o erro vinha do
  `Document::load` e a validacao de faixa nunca era alcancada — e agora
  exercita a validacao de verdade. A assercao tautologica de
  `test_extract_metadata` (`title.is_none() || title.is_some()`) deu lugar a
  duas assercoes reais, incluindo a primeira cobertura de um PDF com `/Info`
  preenchido. De 16 passed / 6 ignored para 22 passed / 1 ignored.
