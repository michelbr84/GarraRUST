- **`gateway.host`/`gateway.port` do arquivo ficam deprecados, e `restart` le `HOST`/`PORT` (#1261).**
  As duas chaves nunca alimentaram o bind (`garraia start` liga em flag >
  `HOST`/`PORT` > `127.0.0.1:3888`), mas o wizard escrevia `0.0.0.0` nelas e
  os comandos cliente as liam como "onde esta o meu gateway". Agora: o
  `garraia init` para de escreve-las e as remove num re-run; os
  `docs/deployment/config.*.yml` nao as trazem mais; `status`, `stop`,
  `doctor` e `admin` usam o mesmo endereco do `start` (com `0.0.0.0`/`::`
  trocados por loopback); o Web Console mostra o bind como somente-leitura,
  com origem `runtime`; e o `config check` as reporta como Warning de
  deprecacao quando diferem do bind efetivo. `garraia restart` ganhou
  `env = "HOST"`/`"PORT"` como o `start` — antes, reiniciar um daemon de
  RunPod religava em loopback em silencio. As chaves seguem no schema (o
  arquivo antigo continua parseando); remove-las de vez fica para um release
  com quebra, porque hoje so trocaria um aviso util por um no-op silencioso.
