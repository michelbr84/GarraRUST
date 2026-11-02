- **Gate de comandos reconhece o executavel do Windows (`curl.exe`, `C:\...\git.exe`) e cobre `kubectl exec` (#1791).**
  O nome do programa so era cortado em `/` e comparado cru, entao `curl.exe`,
  `C:\Windows\System32\curl.exe`, `CURL.EXE` e `git.exe push --force` passavam
  livres no Windows, e `git.exe push --force origin main` e `format.com c:` nem
  chegavam ao bloqueio duro que tem no Linux. Agora o basename corta em `/` e `\`,
  tira o sufixo do PATHEXT (`.exe`, `.com`, `.cmd`, `.bat`) sem diferenciar
  maiusculas e aceita caminho entre aspas (`& "C:\Program Files\Git\cmd\git.exe"`),
  e os substrings do DENY_LIST/CONFIRM_LIST tambem sao checados com o programa
  nessa forma canonica. A mesma resolucao passou a valer no wrapper
  (`/usr/bin/sudo curl`), no pipe para shell (`| /bin/sh`) e no shell com `-c`
  (`/bin/bash -c '...'`), que tambem escapavam no Linux. `kubectl exec pod -- env`
  passa a exigir confirmacao, como o `docker exec`.
