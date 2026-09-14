- **Codigo de pareamento ganha limite de tentativas, comparacao constant-time e avisos
  ao dono (#1191).** O codigo do `/pair` tem 6 digitos (~20 bits) e quem acerta entra na
  allowlist em disco; ate o #1190 o `claim()` nunca rodava em producao, e quando a
  fiacao foi consertada tres fragilidades antigas de `garraia_security::PairingManager`
  passaram a ser alcancaveis. Agora: (1) cada usuario que erra 5 vezes fica 15 minutos
  sem poder tentar (`LockedOut`, sem nem comparar e sem contar no global), e 20 erros
  comparados de qualquer usuario desde o ultimo `/pair` queimam todo codigo pendente
  (`Burned`) — 20 palpites em 10^6 da 2e-5 por ciclo; um `/pair` em outro canal nao zera
  o contador enquanto um codigo pendente sobrevive; (2) a comparacao usa
  `subtle::ConstantTimeEq` e percorre todos os codigos sem sair no primeiro; (3)
  `generate_with_status` devolve `GenerateStatus { code, replaced_pending, previous_burned }`
  e o `/pair` avisa tanto quando substituiu um codigo ainda nao resgatado quanto quando
  os codigos foram queimados desde o ultimo `/pair` (e com quantos erros) — sem isso a
  queima era invisivel: o canal descarta em silencio e o convidado parece ter errado.
  `claim()` mantem a assinatura `Option<String>` (os 11 canais nao mudam) e o usuario
  bloqueado ve o mesmo "unauthorized" de sempre — a resposta nao revela se o codigo
  existia. Limites em `ClaimLimits` (`PairingManager::with_limits`). Trade-off
  documentado no threat model 5.11: em canal de identidade gratis (IRC sem NickServ,
  alts de Discord) um atacante persistente consegue queimar cada codigo novo — DoS do
  pareamento, nao do bot — e o dono fica sabendo pelo `/pair`. Fica de fora, de
  proposito: escopar o codigo pelo canal de origem (`claim()` segue ignorando o
  `channel_id`) — a allowlist e global por instalacao e isso e decisao de produto.
