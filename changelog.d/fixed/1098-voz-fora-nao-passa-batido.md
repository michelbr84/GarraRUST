- **#1098 deixa de passar batido quando os servidores de voz estao fora.**
  Com `voice.enabled`, o gateway apenas logava um warning que ninguem lia e o
  `POST /api/tts` respondia 200 com fallback de texto — o dono nao via erro em
  lugar nenhum. O `GET /api/diagnostics` (e a pagina Diagnostics do console)
  agora tem as linhas `voice.tts` e `voice.stt`: cada uma sonda o endpoint
  configurado com budget de 1.5s e responde `ok`, `skipped` (modo voz
  desligado — nao e defeito) ou `error` com o comando exato de subida da
  `docs/voice.md` no `next_step`. A URL passa pelo `vet_url` do
  `garraia_common::ssrf` com `IpScope::AllowPrivate`, como o Ollama: voz e
  servico local, mas link-local e CGNAT continuam barrados. Falhar alto na
  propria requisicao continua disponivel em `/api/tts?fallback=false`.
