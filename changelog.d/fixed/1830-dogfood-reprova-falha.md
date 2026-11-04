- **O dogfood Linux volta a reprovar um passo que falhou (#1830).** O script
  `scripts/dogfood/linux-clean-install.sh` contava os passos que não passaram
  dentro de `{ … } | tee resumo.txt`, num subshell; o shell pai seguia com zero,
  e o job do gate de release ficava verde com `P9 FALHOU` no resumo (visto no
  dogfood da v0.5.2-rc4). Agora a contagem é feita no shell principal, o
  workflow confere o `resumo.json` por conta própria, e um job do CI roda o
  script de verdade sem Ollama e exige que ele reprove.
