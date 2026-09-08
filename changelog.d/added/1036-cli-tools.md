- **`garra chat` registra as mesmas ferramentas do gateway (#1036).** A CLI
  registrava quatro tools (`file_read`, `file_write`, `bash`, `git_diff`) e o
  prompt do sistema listava-as a mao; `list_dir`, `repo_search`, `run_tests`,
  `web_fetch`, `web_search` e `code_review` ficavam de fora. Agora o conjunto e
  o do gateway mais `git_diff` — `run_tests` com confirmacao como o `bash`,
  `web_search` so com a chave do Brave, `code_review` com o provider e modelo
  da sessao — e a secao de ferramentas do prompt e gerada de `tool_names()`,
  para nunca mais anunciar o que nao existe. `schedule_*` seguem so no
  gateway, porque o chat nao abre `SessionStore`.
