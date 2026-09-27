- **O `garra init` oferece WhatsApp ao lado do Telegram (#1430).** O passo de canal
  do wizard so sabia perguntar por Telegram, e quem instalava o GarraIA para usar
  no WhatsApp tinha de adivinhar que existe um `garra whatsapp link` separado. Ele
  virou uma lista: nenhum canal (o default — Enter sem ler continua nao conectando
  nada), Telegram, WhatsApp pelo numero pessoal, ou os dois. O rotulo do WhatsApp
  ja diz na lista que o caminho e o de aparelho conectado por um cliente NAO
  oficial, e escolher a opcao entrega ao mesmo fluxo de sempre — tela de
  consentimento com default nao, QR e a pergunta de quem pode falar, nada
  reimplementado no wizard. A entrega roda DEPOIS de o `config.yml` ser escrito,
  porque o vinculo grava na mesma secao `channels.whatsapp_linked`; um vinculo que
  nao complete (sem Node, QR nao lido, desistencia) nao derruba o `init`, que so
  diz como tentar de novo. Instalacao sem terminal nao chega ao passo.
