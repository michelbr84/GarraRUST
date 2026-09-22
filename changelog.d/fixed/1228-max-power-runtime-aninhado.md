- **`garraia max-power --goal` para de abortar com "Cannot start a runtime
  from within a runtime" (#1228).** O comando ja roda dentro do runtime tokio
  da CLI, mas `max_power.rs` e `AgentTeam::run` criavam um runtime proprio e
  chamavam `block_on` por dentro, o que o tokio recusa com panic antes da
  primeira etapa. Com provider configurado ou sem ele (execucao offline), o
  goal agora roda de ponta a ponta: a cadeia inteira ficou `async` e usa o
  runtime da CLI. Um teste varre `max_power.rs` e `team.rs` e falha se algum
  deles voltar a montar runtime ou a bloquear fora dos testes.
