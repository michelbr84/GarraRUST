- **Re-auditoria de merito do ledger de supressoes CodeQL, 43 entradas (#1991).**
  O `audit_expiration` vencia em 2026-11-28 e passa a 2027-02-23. Ficam 29
  entradas e saem 14, todas de alertas que a analise do `main` deixou de
  detectar: um alerta dispensado nunca passa a `fixed`, entao nenhum script
  percebia (#40-#42, #44, #45, #115, #177, #178, #186-#188, #195-#197). A
  justificativa do #165 afirmava que nenhuma chave nova e derivada do salt
  constante do admin, o que e falso em dois ramos de fallback de
  `derive_with_passphrase`; o texto foi corrigido e o comportamento virou a
  #1992. As do #144, #145 e #146 respondiam a outra regra
  (`rust/non-https-url`) e foram reescritas para a pergunta que
  `rust/cleartext-transmission` faz. Dez citacoes de linha foram corrigidas, e
  o `codeql-reapply-dismissals.sh --dry-run`, que saia com `exit 2` na `main`
  por `line` desatualizada, volta a passar. As lacunas de ferramenta que
  deixaram isso acumular estao na #1993. Nenhum codigo de producao muda.
