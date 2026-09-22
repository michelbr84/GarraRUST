- **Sessao REST sobrevive ao restart do gateway: `GET /api/sessions/{id}/history`, `POST .../messages` e `DELETE /api/sessions/{id}` deixam de responder 404 para sessao que esta no `sessions.db` (#1372).**
  Os tres handlers so olhavam o mapa em memoria, que nasce vazio a cada
  subida, e davam `{"error":"session not found"}` antes da hidratacao que
  carregaria as mensagens do disco (achado do smoke de instalacao limpa da
  v0.4.4, em todos os cenarios). Agora a sessao fora da memoria volta do
  banco, o historico e servido e o turno novo grava ao lado dos antigos; o
  `DELETE` volta a revogar os tokens depois de um restart. So volta a sessao
  que so a superficie REST gravou: canal da linha `api` no tenant `default`,
  nenhuma chave do Chat Sync, tokens so de `api` e o canal de cada mensagem
  so `api`. Sessao de Telegram, WhatsApp, web, mobile ou do `garraia chat
  --persist` segue 404 por esta rota ate a propria superficie a trazer de
  volta, e nada dela e reescrito (o `--resume latest` do CLI continua achando
  a sua). Id que nao existe segue 404 sem criar linha, e banco ilegivel da
  500 sem readotar nada. O gate de `api_key` e o `origin_guard` rodam antes
  do handler e nao mudam. O `working_dir` da sessao continua so em memoria:
  depois do restart a sessao REST volta sem ele.
  Sessao encerrada pelo `DELETE` nao volta: revogar os tokens so esvaziava
  `session_tokens`, e a linha de uma sessao encerrada ficava igual a de uma
  viva. O `DELETE` agora grava `api_logout` no metadado da linha (so o
  metadado; tenant, canal e usuario ficam) e a readocao recusa quem a
  carrega com o mesmo 404 de id desconhecido, como era antes desta mudanca
  depois do TTL ou do restart. Se a marca nao pode ser gravada, o `DELETE`
  responde 500 `failed to record logout` em vez de `ok`, com os tokens ja
  revogados. E `POST /api/mode/select` deixa de reescrever tenant, canal e
  usuario da linha que ja existe: o upsert com o `X-Session-Id` de qualquer
  id reetiquetava a sessao de outra superficie (uma `whatsapp-<numero>` ainda
  sem mensagem, por exemplo) como da API, e a readocao passava a aceita-la.
  Agora ele so cria a linha que falta e grava apenas o modo.
  Limite conhecido: a marca so existe para `DELETE` feito a partir desta
  versao. Uma sessao REST encerrada numa versao anterior ficou sem ela, e
  depois da atualizacao volta a ser lida por esta rota como uma sessao viva.
  A rota e do proprio operador (loopback, ou o gate de `api_key` num bind
  exposto) e o historico ja esta no `sessions.db` dele; quem quiser apagar
  de vez faz um novo `DELETE`, que agora grava a marca.
