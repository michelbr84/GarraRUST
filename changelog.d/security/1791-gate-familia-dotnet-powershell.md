- **Gate de comandos cobre a familia .NET/PowerShell de leitura de ambiente, download e spawn (#1791).**
  `[Environment]::GetEnvironmentVariable(s)` (que com `'User'`/`'Machine'` le o
  ambiente persistente do registro, fora do scrub R3), `reg query HKCU\Environment`,
  `Get-ChildItem Env:` (o `printenv` do PowerShell), as chamadas `Download*`/`Upload*`
  do `WebClient`, `Start-BitsTransfer`/`bitsadmin` e `[Diagnostics.Process]::Start`
  passam a exigir confirmacao, no mesmo nivel de `os.environ`, `printenv` e `curl`.
  `Start-Process` e avaliado como wrapper: decide o programa que ele dispara, com os
  argumentos do `-ArgumentList` sem as aspas (`Start-Process notepad.exe` segue livre;
  `Start-Process git -ArgumentList 'push --force'` pede confirmacao). `$env:PATH` segue
  livre, como `echo $HOME`.
- **Atribuicao na frente do comando deixa de esconder o programa do gate (#1791).**
  `$r = iwr https://...` (PowerShell) e `FOO=1 curl ...` (POSIX) passavam livres em
  toda regra por programa, porque o primeiro token era o alvo da atribuicao. Agora
  recebem o tratamento do comando sem a atribuicao; `RUST_LOG=debug cargo run` segue
  livre, e `DEBUG=1 node server.js` passa a pedir confirmacao como `node server.js`.
- **Codigo pelo stdin e interpretador com versao no nome passam pelo gate (#1791).**
  `echo '...' | python3` e `python3.12 -c '...'` (e `py -3 -c` no Windows, `pypy3`,
  `perl5.36.0`, `nodejs`) passavam livres. Com isso o alias `import os as o; o.environ`
  fica como residuo aceito, sem ancora textual: o codigo so executa passando por um
  interpretador, e o interpretador e gated pelo programa, qualquer que seja o texto
  (decisao em `docs/security/hardening-decisions.md` secao 7). `kubectl exec` tem o
  mesmo tratamento do `docker exec` em todas as formas, fixado em teste.
