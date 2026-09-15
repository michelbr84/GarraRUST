- **`garra desktop` — a CLI abre o aplicativo desktop (#1181, milestone M1).**
  Ate aqui o controle so ia num sentido: o desktop lancava a CLI como sidecar
  Tauri. Agora existe o caminho inverso. `garra desktop` localiza o aplicativo
  instalado e o lanca; `--status` diz se esta instalado e onde; `--no-launch`
  so imprime o caminho resolvido, para script. Exit codes sysexits, no mesmo
  padrao do `config check`: 0 ok, 69 nao instalado, 70 encontrado mas nao
  abriu — um script consegue separar os dois casos.
  A resolucao tenta, nesta ordem, o diretorio do instalador da plataforma, a
  PATH e o diretorio da propria CLI. O diretorio do instalador vem primeiro
  porque e o unico dos tres que um terceiro nao ocupa por acidente. Ela mora
  em `garraia-desktop-core`, e nao na CLI, por dois motivos: e a crate que
  entra nos gates obrigatorios de CI, e a casca vai precisar da mesma lista de
  caminhos no M2 — uma segunda copia dela divergiria no primeiro instalador
  novo. A CLI continua sem nenhuma dependencia de Tauri, com um teste varrendo
  o proprio fonte para garantir isso, entao `cargo build --workspace --exclude
  garraia-desktop` e toda instalacao headless seguem intactos.
  O invariante que mais importa: a resolucao **nunca** devolve o proprio
  executavel. No `.deb` do desktop os dois binarios sao irmaos no mesmo
  diretorio (`/usr/bin/garraia` e `/usr/bin/garraia-desktop`, com
  `Provides/Conflicts/Replaces: garraia`), e e exatamente ali que um
  lancamento recursivo nasceria.
  Uma coisa o comando de proposito nao responde: se o aplicativo **ja esta
  rodando**. Responder isso exigiria enumerar processos nas tres plataformas
  por nome — heuristica fragil e dependencia nova — ou um canal de instancia
  unica na casca Tauri, que e onde a resposta pode ser confiavel. Esse canal e
  do M2; ate la o `--status` diz que nao sabe, em vez de imprimir um "nao" que
  seria falso toda vez que a janela estivesse aberta.
