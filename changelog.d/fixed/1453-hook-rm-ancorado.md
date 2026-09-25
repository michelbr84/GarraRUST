- **Hook `pre-tool-use` deixa de bloquear toda remocao por caminho (#1453).**
  Os padroes `rm -rf /`, `rm -rf ~` e `rm -rf ./` eram casados como substring
  literal, entao `rm -rf /tmp/scratchpad/x` e `rm -rf ./pintest` — limpezas
  legitimas de uma rodada autonoma — caiam como "comando perigoso". O `rm`
  passa a ser julgado por uma regex ancorada no ALVO: raiz, home (`~`,
  `$HOME`), diretorio atual e pai, sozinhos ou com glob, precedidos de
  qualquer flag (`-rf`, `-r -f`, `--`) e de `sudo`/`xargs`/`&&`. Um caminho
  DENTRO desses diretorios nao casa. Os demais padroes literais (fork bomb,
  `DROP TABLE`, force push em `main`, `dd if=`, `mkfs.`) continuam iguais, e
  `scripts/test-hooks.sh` ganha os casos dos dois lados.
