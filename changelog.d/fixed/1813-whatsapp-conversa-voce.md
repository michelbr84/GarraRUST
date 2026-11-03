- **WhatsApp no modo "meu proprio numero": a conversa "Voce" agora conversa com
  o Garra (#1813).** O assistente do Desktop pedia para testar mandando um "oi"
  para o proprio numero, e essa mensagem (`from_me`) era descartada em silencio
  antes de qualquer politica: o primeiro teste nunca funcionava. A ponte marca
  `self_chat` quando o chat e a propria conta (por telefone ou LID), e o
  gateway aceita `from_me` so ali; o dono escrevendo a terceiros continua fora.
  O eco da resposta do proprio Garra nesse chat e barrado em duas camadas
  (ids enviados na ponte e no runner). A identidade e o numero da conta e passa
  pelo mesmo portao de admissao.
  Salvaguardas da revisao de seguranca: so o `remoteJid` decide a conversa
  "Voce" (o `remoteJidAlt` do Baileys 7 e o do remetente), encaminhada nao vira
  turno, as respostas ali levam uma marca invisivel que outro Garra na mesma
  conta reconhece, e um codigo de 6 digitos digitado ali nao vira pareamento.
