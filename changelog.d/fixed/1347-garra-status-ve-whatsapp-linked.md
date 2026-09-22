- **`garra_status` enxerga o `whatsapp_linked` e os canais push, com o mesmo
  status do `/api/channels` (#1347, fatias 2 e 3).** O relatorio lia so o
  `ChannelRegistry`, onde o `whatsapp_linked` (supervisionado a parte) e os
  canais push (WhatsApp Cloud, Teams, LINE, Google Chat) nunca entram, entao
  o Garra conectado ao WhatsApp respondia que nao tinha acesso ao WhatsApp
  mesmo depois da fatia 1. A tabela de canais e a regra de status sairam do
  `router.rs` para um modulo compartilhado, e o `/api/channels` e o
  `garra_status` chamam a mesma funcao: cada item de `channels` agora traz
  `id`, `name` e `status` (`active` / `offline`). No relatorio so entram
  canais de mensagens que o operador ligou na config: numa instalacao nova a
  lista vem vazia (antes vinham oito `offline`, que o modelo repetia como
  "configurado e fora do ar"), e um canal ligado e caido sai `offline`
  mesmo quando nao precisa de segredo; o `/api/channels` mantem a regra
  dele. Web chat, API, CLI e MCP nunca entram na lista. O relatorio ganha
  `execution_profile` (`standard` / `isolated-pod`), `mcp_servers` (so nome,
  `connected` e contagem de ferramentas, nunca comando, env ou erro) e
  `session.channel`, e o `session.id` sai sempre mascarado nos 4 ultimos
  digitos (no WhatsApp ele e o numero de telefone inteiro). Num turno de
  portao restrito (o piso `search` do WhatsApp) ou numa sessao que nao e
  provadamente do operador, o relatorio retem o que e do operador
  (`working_dir`, `project_id`, a lista `providers`, os nomes dos servidores
  MCP e a versao exata, que cai para `major.minor`) e lista o que reteve
  em `withheld`, para o modelo nao confundir "retido" com "nao ha". Quem
  fala na sessao sai do que os pontos de entrada gravam (a superficie de
  cada turno, que so acumula, e as fontes de `chat_session_keys`), e nao do
  prefixo do id: a sessao do Telegram resolvida por UUID nao tem prefixo e
  passava por local. So web chat, API, VS Code e Desktop contam como do
  operador, e so com a porta fechada (loopback ou `gateway.api_key`); sessao
  desconhecida, app mobile (conta aberta em `/auth/register`), A2A e gateway
  exposto pelo opt-out sem chave ficam restritos. A tool passa a ser
  registrada depois de montar os canais push e antes do primeiro canal pull
  conectar, e guarda so as contagens dos push (guardar os canais fecharia um
  ciclo de `Arc` com o estado do gateway). A instrucao do runtime acompanha
  o formato novo: um canal de mensagens ausente da lista nao esta ligado, a
  ausencia de web chat, API, CLI e MCP nao diz nada, e a superficie da
  conversa esta em `session.channel`. Um teste ponta a ponta pergunta "voce
  tem acesso ao WhatsApp?" pela ponte de teste e pelo piso do canal e recebe
  "sim" com o canal conectado e "nao" sem ele ou com a ponte caida.
