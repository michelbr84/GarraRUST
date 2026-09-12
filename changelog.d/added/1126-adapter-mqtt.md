- O gateway fala com dispositivos fisicos via MQTT (#1126, epic #1124). Com a
  secao `hardware.mqtt` no config (broker `host:porta`, `username` e
  `password_env` — o **nome** da env que guarda a senha, nunca a senha), o
  boot sobe o adapter rumqttc: descobre dispositivos pelo manifesto que cada
  um publica retained em `garra/devices/{id}/capabilities`, marca presenca
  pelos status/LWT em `garra/devices/{id}/status` e guarda o estado em
  `<data_dir>/hardware.db` (mesma resolucao de `memory.db`, via
  `AppConfig::hardware_db_path`).
- A leitura correlaciona `request_id`: publica em `garra/devices/{id}/get/{cap}`
  e espera a resposta em `state/{cap}` pelo id — sem resposta no timeout
  (5s), erro claro para o modelo. A execucao publica `set/{cap}` com os
  args validados contra o schema truncado do manifesto (tipos primitivos,
  `required`) e confirma **o que o broker aceitou** — nao o que o
  dispositivo aplicou; conferir o efeito e uma leitura depois.
- Fail-closed nos dois sentidos: manifesto que viola as invariantes
  (`validar()` de Capability, id que nao bate com o topico, segmentos
  perigosos) recusa o dispositivo inteiro, e classe de risco vem do
  manifesto declarado — nunca inferida de payload em tempo de execucao.
- `garra chat` sobe o mesmo transporte pela mesma funcao do gateway, entao
  o registry e o store de presenca (`hardware.db`) sao os mesmos nos dois.
- Sem `hardware.mqtt` no config, nada muda: registry vazio e
  `device_list` sem dispositivos, como antes.