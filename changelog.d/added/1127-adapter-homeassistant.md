- O gateway fala com o hub Home Assistant (#1127, epic #1124). Com a secao
  `hardware.home_assistant` no config (`url` do hub e `token_env` — o
  **nome** da env que guarda o long-lived access token, nunca o token), o
  boot sobe o adapter REST + WebSocket: descobre entidades por
  `GET /api/states`, le por `GET /api/states/{entity_id}`, executa por
  `POST /api/services/{domain}/{service}` e marca presenca pelos eventos
  `state_changed` do WebSocket, com reconexao automatica.
- Risco por dominio, pre-avaliado: sensor e binary_sensor so leem (R0);
  light, switch e climate executam em R1 (power, brightness, temperature);
  cover em R2 (open, close, set_position); lock em R3 (lock, unlock). Todo
  dominio fora dessa tabela nao vira dispositivo — fail-closed, sem R4/R5
  neste slice.
- Toda chamada ao hub passa pelo guard SSRF (`vet_url` + cliente pinado,
  regra 14) com escopo de IPs privados: o hub e alvo legitimo da LAN, mas
  link-local, CGNAT, multicast e addresses nao especificados continuam
  bloqueados; o WebSocket conecta no mesmo IP pinado, sem re-resolver DNS.
- `garra chat` sobe o mesmo adapter pela mesma funcao do gateway, e o store
  de presenca (`hardware.db`) passa a ser compartilhado entre os adapters
  configurados — uma fonte unica de online/offline.
- Feature `home-assistant` OFF por default (mesmo padrao do `mqtt` e do
  `storage-s3`): quem nao usa o hub nao paga a arvore de deps
  (reqwest + tokio-tungstenite). Sem a secao no config, nada muda.