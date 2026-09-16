- **`ConfigLoader::save` passou a ser atomica, e um `config.yml` vazio deixou de
  virar defaults (#1238).** A escrita era `std::fs::write` — truncate-then-write
  — com o aperto de permissao (`0600`) rodando **depois**: havia uma janela em
  que `llm.*.api_key` e `gateway.api_key` estavam no disco sob o modo do umask,
  e uma queda no meio deixava o arquivo pela metade ou vazio. Agora e tmp no
  mesmo diretorio, nascido `0600` via `OpenOptionsExt::mode`, `sync_all`,
  `rename` e `fsync` do diretorio — o mesmo padrao que o store de sessao do
  `whatsapp_linked` ja usava.
  O par disso: `ConfigLoader::load` **recusa** um `config.yml` em branco em vez
  de devolver `AppConfig::default()` com sucesso. Era o pior dos tres estados —
  o gate de credencial do gateway desaparecia em silencio e o `save()` seguinte
  gravava os defaults por cima da config do usuario.
  A janela deixou de ser teorica nesta fatia: alem da CLI (`garra whatsapp
  link`/`logout`), o gateway passou a chamar `set_channel_enabled` sozinho
  quando o servidor invalida a sessao — dois escritores, sem lock.
