- **Access Policy v2 do WhatsApp pessoal: `channels.whatsapp_linked.access`
  (#1388, #1390, #1391, #1392, #1399, #1412, #1421, #1423; ADR 0025).** Quem
  fala com o agente e ate onde cada um vai, por principal: `admission`
  (`restricted` | `open`), `default` (o que um desconhecido recebe em `open`,
  nunca `full`), `users` (por numero ou JID: `level: chat|read|full`,
  `write: on|off`, `role: owner`, `blocked: true`) e `groups` (`enabled`,
  `default` e politica por JID de grupo). O nivel e o `write` compilam para
  um teto de capacidades composto por E com o modo da sessao: o teto so
  tira, nunca poe, e `write` mexe so em escrita de arquivo (nativa e MCP) —
  shell, dispositivo, mensagem e agenda sao controles independentes. Grupo
  nunca herda o papel do dono; bloqueado vence `open`, `allow` e pareamento;
  nivel desconhecido e `chat`; a secao inteira e relida por turno (um
  bloqueio vale na mensagem seguinte) e cada normalizacao deixa um aviso
  sem numero. Compatibilidade: `allow`, `owners` e `reply_in_groups`
  continuam valendo e, sem `access:`, nada muda — nivel e `write` so
  existem onde foram declarados; a excecao e quem entrou por codigo do
  `/pair`, que passa a ter teto `read` (credencial fraca). O log do turno
  ganha `principal` e `alcance` (etiquetas fixas, nunca identidade).
