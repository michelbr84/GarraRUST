- **`mascarar_numeros_longos` agora cobre telefone com separador comum (#1514).**
  A funcao que o `RedactingWriter` usa para mascarar PII em log so contava
  sequencia **contigua** de 10+ digitos: um telefone com espaco, hifen ou
  parenteses (`"55 11 98765 4321"`, `"55-11-98765-4321"`,
  `"+55 (11) 98765-4321"`) atravessava inteiro. Achado por um `test-engineer`
  durante a revisao independente do PR de #1513. A ponte entre grupos exige que
  o grupo ancora nao esteja colado a letra (senao um id curto na frente de um
  telefone de verdade arrastaria o telefone para a rejeicao do id), rejeita
  grupo que fique colado a letra do outro lado, poe teto de 4 grupos no total
  (contra lista solta de numeros curtos) e para na hora se o trecho ja fechado
  for exatamente uma data `AAAA-MM-DD` (a forma que o `CLAUDE.md` manda usar em
  prosa narrativa). Nao cobre numero partido entre duas `MessagePart::Text`
  diferentes — isso fica para quem redige o conteudo agregado, fora do escopo
  desta funcao.
