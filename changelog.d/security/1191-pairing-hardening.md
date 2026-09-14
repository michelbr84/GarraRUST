- **Codigo de pareamento ganha limite de tentativas, comparacao constant-time e aviso
  de substituicao (#1191).** O codigo do `/pair` tem 6 digitos (~20 bits) e quem
  acerta entra na allowlist em disco; ate o #1190 o `claim()` nunca rodava em
  producao, e quando a fiacao foi consertada tres fragilidades antigas de
  `garraia_security::PairingManager` passaram a ser alcancaveis. Agora: (1) cada
  usuario que erra 5 vezes fica 15 minutos sem poder tentar (`LockedOut`, sem nem
  comparar), e 20 erros de qualquer usuario desde o ultimo `/pair` queimam todo
  codigo pendente (`Burned`) — o dono gera outro; (2) a comparacao usa
  `subtle::ConstantTimeEq` e percorre todos os codigos sem sair no primeiro; (3)
  `generate_with_status` diz se substituiu um codigo ainda nao resgatado e o `/pair`
  avisa o dono. `claim()` mantem a assinatura `Option<String>` (os 11 canais nao
  mudam) e o usuario bloqueado ve o mesmo "unauthorized" de sempre — a resposta nao
  revela se o codigo existia. Limites em `ClaimLimits` (`PairingManager::with_limits`).
  Fica de fora, de proposito: escopar o codigo pelo canal de origem (`claim()` segue
  ignorando o `channel_id`) — a allowlist e global por instalacao e isso e decisao
  de produto, nao hardening.
