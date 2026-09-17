- **Ponte WhatsApp em Node/Baileys, stateless, falando NDJSON v1 por stdio
  (#1238, ADR 0023 slice S4a).** `bridge/whatsapp/` e um diretorio, nao uma
  crate: fica fora do workspace Cargo e o Rust o spawna como processo filho.
  A divisao de trabalho e o ponto — a ponte cuida do protocolo do WhatsApp
  (Baileys pinado exato em `7.0.0-rc14`) e o Rust cuida de tudo que e do
  GarraIA: desenhar o QR, cifrar e gravar a sessao, aplicar allowlist, rotear
  para o agente. A ponte **nunca escreve no disco**; o estado de autenticacao
  vive em memoria e volta por `session_update` como snapshot completo, para o
  `CredentialVault` gravar cifrado. stdout e exclusivamente NDJSON (logger do
  Baileys em `silent` apontado para o fd 2), e todo diagnostico sai por evento
  `log` ja redigido para os 4 ultimos digitos. Acompanha a fixture
  `crates/garraia-channels/tests/fixtures/fake_whatsapp_bridge.py`, que fala o
  mesmo protocolo em milissegundos, para que os testes Rust rodem sem Node e
  sem telefone, e um `protocol-lint` que arbitra as duas contra o contrato.
  Nada no workspace Rust consome a ponte ainda: o wiring da CLI e do gateway
  vem no PR companheiro.
