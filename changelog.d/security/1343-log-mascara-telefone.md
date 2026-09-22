- **O log deixa de gravar o telefone de quem fala com o GarraIA pelo WhatsApp
  ou pelo Signal (#1343).** O id de sessao desses canais embute o remetente
  (`whatsapp-<telefone>`, `signal-<telefone>`, `whatsapp-linked-<jid>`), e os
  spans do `AgentRuntime` gravam `session_id` em todo evento do turno: o
  canal `whatsapp_linked` ja so logava os 4 ultimos digitos por conta
  propria, mas o span passava o numero inteiro por baixo. O `RedactingWriter`
  do stderr e do `garraia.log` agora troca toda sequencia de 10 ou mais
  digitos que nao esteja colada a letra por `…` e os 4 ultimos; hash
  hexadecimal e UUID passam intactos. O `Debug` do escopo de aprovacao
  mascara a sessao do mesmo jeito, e o do registro de pedidos pausados mostra
  so quantos ha. O `user_id` do LINE, que nao e numerico, continua legivel.
