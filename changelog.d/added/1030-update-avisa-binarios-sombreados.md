- **`garraia update` avisa de binarios antigos no PATH (#1030).** O update
  trocava so o proprio binario; um `/usr/bin/garraia` de outra instalacao
  seguia la, intocado e sem aviso — e era ele que scripts, cron e terminais
  com PATH diferente passavam a rodar. Depois de atualizar, o comando varre
  os diretorios do PATH e os de sistema (`/usr/local/bin`, `/usr/bin`) por
  outros `garraia`/`garra`, pergunta a versao de cada um (com timeout) e
  avisa os que diferem, com caminho, versao, se ele vem antes ou depois do
  atualizado no PATH e o comando para remover. `garraia update
  --check-binaries` roda so a varredura, sem baixar nada.
