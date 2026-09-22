- **O "sim" a um pedido de confirmacao volta a aprovar em todo canal com um
  humano do outro lado (#1343).** Quando uma ferramenta pedia confirmacao (um
  comando arriscado do `bash` com `agent.tool_confirmation_enabled`, um
  `device_execute` R3/R4), o turno pausava e o "sim" da mensagem seguinte nao
  aprovava nada: o gateway, a CLI e a API compativel com OpenAI guardam o
  historico como texto, o pedido pausado nao voltava, e a ferramenta
  perguntava de novo para sempre. Agora o pedido fica guardado em memoria e o
  "sim" (a mensagem inteira) aprova o pedido para o turno seguinte, uma vez,
  dentro de 5 minutos, so se
  vier do mesmo remetente, na mesma sessao e no mesmo canal: a mesma conexao
  no Web Console e no desktop, o dono com o mesmo `Authorization` em
  `/v1/chat/completions`, o `sub` do JWT no app mobile, o id do usuario na
  plataforma no Telegram, Discord, Slack, WhatsApp, Matrix, IRC, Signal, LINE,
  Teams, Google Chat, iMessage e no WhatsApp pessoal, e o terminal no
  `garraia chat`. Em grupo, o "sim" de outro membro nao aprova e encerra o
  pedido. Qualquer mensagem no meio encerra o pedido, um segundo "sim" pausa
  de novo, e reiniciar o gateway cancela os pendentes. A2A, OpenClaw,
  `POST /api/sessions/{id}/messages`, a resposta do agente no chat do
  workspace, `garraia ask` e o `garra_agent` do `garraia mcp-server` continuam
  sem retomada, de proposito: ali a pausa segue terminal.
