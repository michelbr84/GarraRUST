- **O runtime passa a suportar a retomada de um pedido de confirmacao entre
  turnos (#1343, fatia S1); nenhum canal adere ainda.** Hoje um "sim" dado no
  turno seguinte nunca aprova nada nos canais de producao: a aprovacao
  GAR-187 so e lida do historico, e gateway, CLI e API compativel com OpenAI
  guardam o historico como texto puro, entao o resultado de ferramenta com o
  marcador nunca volta e a ferramenta pede confirmacao para sempre. Esta
  fatia so traz o mecanismo: quando o turno traz
  `ExecContext::approval_scope` (canal, sessao e um remetente derivado pelo
  servidor), o `AgentRuntime` grava em memoria o nome da ferramenta e a
  impressao digital HMAC do pedido, e o proximo turno aprova so se a mensagem
  for exatamente uma palavra de aprovacao, do mesmo remetente, na mesma sessao
  e no mesmo canal, dentro de 5 minutos. A aprovacao vale para um unico turno
  (o registro e consumido na leitura), e nao para uma unica execucao: dentro
  desse turno, chamadas identicas ao pedido aprovado rodam, como no caminho
  pelo historico. Qualquer outra mensagem na sessao encerra o pedido (a regra
  do #1340, inclusive em grupo), marcador copiado ou forjado no historico
  deixa de pesar, e reiniciar o processo continua cancelando tudo, porque a
  chave do HMAC vive so na memoria. O assunto cru (o comando do bash) nunca e
  guardado. Caminho sem escopo continua exatamente como antes. Nenhum
  chamador de producao preenche `approval_scope` ainda, entao o sintoma
  visivel so muda quando os canais aderirem, nas fatias seguintes.
