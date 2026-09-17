- **`garra whatsapp link` deixa de exibir uma linha congelada enquanto a ponte
  esta muda (#1238).** O `→ conectando ao WhatsApp…` era impresso uma vez, na
  troca de fase, e o contador regressivo so roda nas fases que ja tem QR na
  tela. Medido com o Baileys real (7.0.0-rc14) num container que nao alcanca
  os servidores do WhatsApp: `status connecting` aos 2 s e **nada** por 85 s.
  Ou seja, ate dois minutos de linha estatica antes de o teto de
  `no_progress_after_secs` encerrar com a mensagem certa. Nao era infinito,
  mas nao era feedback. Agora um pulso a cada 5 s diz ha quanto tempo esta
  tentando e em quantos segundos desiste — e o prazo anunciado e o MESMO
  relogio que de fato encerra, com teste afirmando a soma. O
  `PairUi::connecting` entrou **sem implementacao default**: uma UI nova tem
  de decidir o que mostrar, porque herdar silencio em silencio foi exatamente
  o defeito.
