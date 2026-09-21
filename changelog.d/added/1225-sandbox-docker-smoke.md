- **Teste de integracao do sandbox contra Docker real prova `--network none` e o mount do cwd (#1225 S4).**
  Ate aqui todo teste do sandbox por tool olhava a STRING que `wrap_command`
  monta — nenhum subia um container, entao um Docker que ignorasse a flag, ou
  um wrap que deixasse o comando rodar no host, manteria a suite verde.
  `crates/garraia-agents/tests/sandbox_docker.rs` dirige a `BashTool` com a
  policy Docker de verdade (imagem default `debian:bookworm-slim`, pull
  implicito, timeout de 120 s): o `echo` sai de dentro do container
  (`/.dockerenv` presente em todo comando, para um comando que rodasse no host
  nao passar), `getent hosts example.com` falha com `network_disabled = true` e
  resolve no controle com a rede ligada (pulado com aviso se o proprio runner
  nao tiver rede), e um arquivo criado no cwd do host e lido por `cat` por
  caminho relativo la dentro — e deixa de ser visivel com `mount_workdir =
  false`. Sem daemon o teste se pula com aviso; `GARRAIA_REQUIRE_DOCKER=1`,
  que o CI liga no runner Linux num step proprio ao lado do de `storage-s3`,
  transforma o skip em falha, no mesmo padrao do `s3_integration.rs`.
