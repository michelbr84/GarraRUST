- O gate de comandos passou a cobrir os fake-negativos que o #1075 deixou
  documentados como residuo. Script-file num shell ou interpretador
  (`bash payload.sh`, `python3 script.py`, `python3 -m modulo`) e codigo que
  o gate nao consegue ler e agora exige confirmacao, como o `-c` inline ja
  exigia. `xargs -a arquivo curl` deixou de esconder o programa atras do
  caminho do arquivo, e o mesmo defeito atingia `sudo -u root curl`. `awk`
  com `system()` ou pipe para comando, e o `e`/`w` do GNU sed, entram pelo
  script em vez de passarem por ferramenta de texto. `find` com
  `-exec`/`-ok`/`-delete`, `nc`/`ncat` sem flag, e os verbos de exfiltracao
  de `aws`/`az`/`gcloud`/`gsutil` tambem entraram. Nada disso e substring
  novo no DENY_LIST: sao regras por programa, com o script tokenizado com
  aspas respeitadas, e cada uma tem teste do falso positivo que ela nao pode
  causar (#1078).
