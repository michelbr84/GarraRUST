- **`garra_status` enxerga o `whatsapp_linked` e os canais push, com o mesmo
  status do `/api/channels` (#1347, fatias 2 e 3).** O relatorio lia so o
  `ChannelRegistry`, onde o `whatsapp_linked` (supervisionado a parte) e os
  canais push (WhatsApp Cloud, Teams, LINE, Google Chat) nunca entram, entao
  o Garra conectado ao WhatsApp respondia que nao tinha acesso ao WhatsApp
  mesmo depois da fatia 1. A tabela de canais e a regra de status sairam do
  `router.rs` para um modulo compartilhado, e o `/api/channels` e o
  `garra_status` chamam a mesma funcao: cada item de `channels` agora traz
  `id`, `name` e `status` (`active` / `offline`), e canal que ninguem ligou
  fica de fora. O relatorio ganha `execution_profile` (`standard` /
  `isolated-pod`), `mcp_servers` (so nome, `connected` e contagem de
  ferramentas, nunca comando, env ou erro) e `session.channel`, e o
  `session.id` sai sempre mascarado nos 4 ultimos digitos (no WhatsApp ele e
  o numero de telefone inteiro). Num turno de portao restrito (o piso
  `search` do WhatsApp) ou numa sessao de canal remoto, o relatorio retem o
  que e do operador — `working_dir`, `project_id`, a lista `providers`, os
  nomes dos servidores MCP e a versao exata, que cai para `major.minor` — e
  lista o que reteve em `withheld`, para o modelo nao confundir "retido" com
  "nao ha". A tool passa a ser registrada depois dos canais push e guarda so
  as contagens deles (guardar os canais fecharia um ciclo de `Arc` com o
  estado do gateway). A instrucao do runtime acompanha o formato novo: um
  canal ausente da lista nao esta ligado. Um teste ponta a ponta pergunta
  "voce tem acesso ao WhatsApp?" pela ponte de teste e pelo piso do canal e
  recebe "sim" com o canal conectado e "nao" sem ele ou com a ponte caida.
