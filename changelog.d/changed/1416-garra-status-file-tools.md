- **`garra_status` diz se as file tools tem raiz nesta sessao (#1416, #1418).**
  O relatorio ganha o bloco `file_tools` — `ready` (bool), `source`
  (`session_workspace` | `session_working_dir` | `declared` | `none`), `roots`
  (contagem, so quando declaradas) e `means` (so quando `ready = false`: a
  frase que o modelo deve dizer em vez de prometer ler arquivo). Sem caminho
  nenhum, por isso sai inteiro tambem no turno restrito do WhatsApp, onde
  `session.working_dir` e retido. O resolvedor e o mesmo do boot e do
  `/api/diagnostics` (`raizes_das_file_tools`): so le metadado.
