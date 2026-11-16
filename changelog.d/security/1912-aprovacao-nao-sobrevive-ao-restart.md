- **Testes de dois processos provam que aprovacao nao sobrevive ao restart do gateway (#1912).**
  A regra ja existia (chave HMAC sorteada por processo, registros so em memoria), mas
  todo teste cunhava e conferia a impressao digital no mesmo processo: trocar a chave
  por uma constante passava verde. `runtime/tests/aprovacao_apos_restart.rs` reexecuta
  o binario de teste como "gateway A", recebe o marcador cunhado la e exige que o
  "gateway B" negue o "sim" sobre o historico reidratado, nos tres caminhos do runtime.
  O "sim" dentro do TTL e o `agent.approval: session` sao provados com um runtime novo
  (registros vazios), e uma guarda de fonte impede que chave ou registros passem a ler
  disco, banco ou ambiente. So testes; nenhum
  comportamento de producao muda. O percurso real por Telegram e MCP delegado segue
  aberto na issue.
