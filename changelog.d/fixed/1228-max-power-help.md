- **`garraia max-power` para de anunciar como futura a execucao que ja existe
  (#1228).** A ajuda dizia que a execucao do pipeline "lands in
  GAR-495..GAR-501", mas ela existe desde o PR #1218: com provider padrao
  resolvido cada etapa e uma chamada ao LLM, e sem ele a execucao e
  deterministica (offline) — a linha `execution:` da saida diz qual rodou. O
  exemplo do menu e a dica do `garraia runs list` sem ledger passam a nomear o
  binario em execucao em vez do alias fixo `garra`, que nao existe numa
  maquina com so o `garraia` (imagem Docker, `cargo install`).
