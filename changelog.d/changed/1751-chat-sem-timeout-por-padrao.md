- **`garra chat` deixa de ter teto de parede por padrao; `--timeout-secs` vira opt-in (#1751).**
  Sem a flag o turno roda ate o modelo terminar (Ctrl+C cancela; o teto de 30 s por
  ferramenta e o connect timeout dos providers continuam valendo). Com a flag estourada,
  ou no Ctrl+C, o texto ja transmitido e a evidencia das ferramentas entram no historico
  (em memoria e, com `--persist`, no SQLite) com o marcador de interrupcao, em vez de
  serem descartados. Antes o default de 120 s cortava o turno inteiro e perdia tudo.
  `garra ask` (60 s, exit 124) e o timeout do `garra_agent` MCP (300 s) nao mudam.
