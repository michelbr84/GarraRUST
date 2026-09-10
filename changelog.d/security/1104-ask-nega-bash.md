- **O modo `ask` passa a negar `bash` (#1104).** A negacao de `file_write`
  era decorativa: `bash` e execucao arbitraria e o modelo escrevia o arquivo
  pelo shell (`printf ... > arquivo`), contornando a promessa "apenas
  perguntas" do modo padrao de Telegram/Discord/WhatsApp. Leitura (`file_read`,
  `list_dir`, `repo_search`, `web_search`) segue livre. `run_tests` nao e
  `bash`: executa toolchains fixas com nome de programa fixo, e continua
  permitido.
