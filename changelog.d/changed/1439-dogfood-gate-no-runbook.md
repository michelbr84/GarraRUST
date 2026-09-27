- **O runbook de release ganha um gate de dogfood manual antes do tag
  (#1439).** `docs/releasing.md` §1.5 fixa a matriz que alguem com maquina e
  telefone executa contra o candidato — instalacao limpa nos dois
  instaladores, WhatsApp de ponta a ponta, restart sem QR, `update` e
  `rollback`, os bundles do desktop, isolamento do workspace por sessao e
  diagnostico sem aviso espurio — com data, sistema e executor registrados no
  corpo da PR de release. Linha sem data, release nao sai. E a resposta ao
  padrao das v0.4.4/v0.4.5, verdes no CI e quebradas no caminho real; a
  automacao do que da para automatizar segue na #1426.
