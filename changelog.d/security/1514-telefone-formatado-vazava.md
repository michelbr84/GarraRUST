- **`mascarar_numeros_longos` agora cobre telefone com separador comum (#1514).**
  A funcao que o `RedactingWriter` usa para mascarar PII em log so contava
  sequencia **contigua** de 10+ digitos: um telefone com espaco, hifen ou
  parenteses (`"55 11 98765 4321"`, `"55-11-98765-4321"`,
  `"+55 (11) 98765-4321"`, e o celular BR com o nono digito separado,
  `"55 11 9 8765 4321"`) atravessava inteiro. Achado por um `test-engineer`
  durante a revisao independente do PR de #1513. A ponte entre grupos exige
  que o grupo ancora nao esteja colado a letra nem seja parte de um UUID
  (senao um id curto na frente, ou o ultimo grupo so-digitos de um UUID,
  arrastaria consigo um telefone de verdade ou a propria protecao de UUID),
  rejeita grupo que fique colado a letra do outro lado, para de crescer assim
  que o total ja basta (senao "os 4 ultimos digitos" vira "os 4 ultimos
  bytes"), exige que ao menos um grupo tenha 4+ digitos, e recusa intervalo
  numerico (`"100000-200000"`) e numero com separador de milhar
  (`"1 234 567 890"`). Para na hora se o trecho ja fechado for uma data
  (`AAAA-MM-DD`, a forma que o `CLAUDE.md` manda usar em prosa narrativa, ou
  `DD-MM-AAAA`) — a menos que o proximo grupo, espiado antes de decidir,
  completasse um telefone plausivel, para nao deixar um numero real que por
  acaso comeca com essa forma (`"5511-98-76-5432"`) escapar so por isso.
  Revisado por `code-reviewer` e `security-auditor` independentes; a segunda
  rodada achou e corrigiu uma regressao na protecao de UUID e o formato de
  celular BR mais comum, que a primeira versao deste fragmento nao cobria.
  Nao cobre numero partido entre duas `MessagePart::Text` diferentes — isso
  fica para quem redige o conteudo agregado, fora do escopo desta funcao.
