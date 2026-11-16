- **Testes de dois processos provam que aprovacao nao sobrevive ao restart do gateway (#1912).**
  A regra ja existia (chave HMAC sorteada por processo, registros so em memoria), mas
  todo teste cunhava e conferia a impressao digital no mesmo processo: trocar a chave
  por uma constante passava verde. `runtime/tests/aprovacao_apos_restart.rs` reexecuta
  o binario de teste como "gateway A", recebe o marcador cunhado la e exige a negacao
  no "gateway B" pelas duas vias de retorno — historico reidratado e "sim" dentro do
  TTL —, nos tres caminhos do runtime, alem de `agent.approval: session` e de uma
  guarda de fonte contra disco/banco/ambiente nesses modulos. So testes; nenhum
  comportamento de producao muda. O percurso real por Telegram e MCP delegado segue
  aberto na issue.
