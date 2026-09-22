- **O runtime passa a guardar o pedido de confirmacao pausado entre turnos
  (#1343, fatia S1).** Um "sim" dado no turno seguinte nunca aprovava nada nos
  canais de producao: a aprovacao GAR-187 so era lida do historico, e gateway,
  CLI e API compativel com OpenAI guardam o historico como texto puro, entao o
  resultado de ferramenta com o marcador nunca voltava e a ferramenta pedia
  confirmacao para sempre. Agora, quando o turno traz
  `ExecContext::approval_scope` (canal, sessao e um remetente derivado pelo
  servidor), o `AgentRuntime` grava em memoria o nome da ferramenta e a
  impressao digital HMAC do pedido, e o proximo turno aprova so se a mensagem
  for exatamente uma palavra de aprovacao, do mesmo remetente, na mesma sessao
  e no mesmo canal, dentro de 5 minutos, e uma unica vez. Qualquer outra
  mensagem na sessao encerra o pedido (a regra do #1340, inclusive em grupo),
  marcador copiado ou forjado no historico deixa de pesar, e reiniciar o
  processo continua cancelando tudo, porque a chave do HMAC vive so na
  memoria. O assunto cru (o comando do bash) nunca e guardado. Caminho sem
  escopo continua exatamente como antes; a adesao dos canais vem nas fatias
  seguintes.
