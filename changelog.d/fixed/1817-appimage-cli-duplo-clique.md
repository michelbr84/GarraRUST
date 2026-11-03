- **AppImage da CLI: duplo clique deixa de "nao fazer nada" (#1817).** O
  `garraia-linux-x86_64.AppImage` e o programa de terminal, mas o gerenciador
  de arquivos o executa sem terminal: o chat abria, lia fim de arquivo e saia
  sem mostrar nada. O `AppRun` virou um script: com argumentos ou num terminal
  continua o `garraia` direto; no duplo clique abre um emulador de terminal com
  ele, ou, sem nenhum, avisa por notificacao que e a versao de linha de comando
  e aponta o AppImage do desktop. `docs/installation.md` explica o que e cada
  AppImage, como por no PATH e o requisito de FUSE.
