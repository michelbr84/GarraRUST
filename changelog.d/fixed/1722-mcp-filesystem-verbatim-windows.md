- **No Windows, as tools do servidor MCP `filesystem` voltam a funcionar (#1722).**
  A ponte reescrevia cada caminho pelo canonico de `std::fs::canonicalize`, que
  no Windows vem com o prefixo verbatim `\?\C:\...`; o servidor Node normalizava
  isso para `C:\?\C:\...` e respondia `Access denied - path outside allowed
  directories` em toda operacao com caminho (`read_text_file`, `list_directory`,
  `write_file`, ...). E, mesmo sem o prefixo, o servidor desta versao recusa todo
  caminho-filho escrito com `\` e so aceita `/` (probe direto: `C:\ws\notas.txt`
  negado, `C:/ws/notas.txt` aceito). Agora a string entregue ao servidor sai sem
  o prefixo (UNC inclusive) e com `/` como separador, e `list_allowed_directories`
  mostra as raizes no mesmo formato, para o modelo copiar um caminho que o
  servidor aceita. A checagem do jail continua sobre o canonico nativo;
  Linux/macOS nao mudam.
