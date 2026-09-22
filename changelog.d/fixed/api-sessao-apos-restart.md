- **Sessao REST sobrevive ao restart do gateway: `GET /api/sessions/{id}/history`, `POST .../messages` e `DELETE /api/sessions/{id}` deixam de responder 404 para sessao que esta no `sessions.db`.**
  Os tres handlers so olhavam o mapa em memoria, que nasce vazio a cada
  subida, e davam `{"error":"session not found"}` antes da hidratacao que
  carregaria as mensagens do disco (achado do smoke de instalacao limpa da
  v0.4.4, em todos os cenarios). Agora a sessao fora da memoria volta do
  banco, o historico e servido e o turno novo grava ao lado dos antigos; o
  `DELETE` volta a revogar os tokens depois de um restart. So volta a sessao
  que so a superficie REST gravou: canal da linha `api` no tenant `default`,
  nenhuma chave do Chat Sync, tokens so de `api` e o canal de cada mensagem
  so `api`. Sessao de Telegram, WhatsApp, web, mobile ou do `garra chat
  --persist` segue 404 por esta rota ate a propria superficie a trazer de
  volta, e nada dela e reescrito (o `--resume latest` do CLI continua achando
  a sua). Id que nao existe segue 404 sem criar linha, e banco ilegivel da
  500 sem readotar nada. O gate de `api_key` e o `origin_guard` rodam antes
  do handler e nao mudam. O `working_dir` da sessao continua so em memoria:
  depois do restart a sessao REST volta sem ele.
