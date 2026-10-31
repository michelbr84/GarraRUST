- **Gate de comando: falso-positivo de `environ` fechado e o PowerShell por
  extenso gated (#1781).** O substring solto `environ` no CONFIRM_LIST
  bloqueava o read-only `[Environment]::OSVersion` do PowerShell; o padrao
  agora e ancorado — caminho de arquivo (`/environ`, que segue pegando
  `/proc/<pid>/environ` e `/etc/environment`) e o par `os.environ` do Python.
  E o `SENSITIVE_PROGRAMS` listava `curl`/`wget` — que no PS 5.1 sao alias do
  Invoke-WebRequest — enquanto o comando escrito por extenso passava livre:
  entram `Invoke-WebRequest`/`iwr`, `Invoke-RestMethod`/`irm` e
  `Invoke-Expression`/`iex`, full name e alias, no tier de confirmacao. Codigo
  morto que parecia gate removido (`new_readonly`, `ALLOW_LIST_READONLY`,
  `is_allowed` — que devolvia `true` incondicional em producao).
