- **`mascarar_numeros_longos` agora cobre telefone com separador comum (#1514).**
  A funcao que o `RedactingWriter` usa para mascarar PII em log so contava
  sequencia **contigua** de 10+ digitos: um telefone com espaco, hifen ou
  parenteses (`"55 11 98765 4321"`, `"55-11-98765-4321"`,
  `"+55 (11) 98765-4321"`, o celular BR com o nono digito separado
  (`"55 11 9 8765 4321"`) e o celular NANP com DDI separado
  (`"+1 555 123 4567"`) atravessava inteiro. Achado por um `test-engineer`
  durante a revisao independente do PR de #1513. A ponte entre grupos exige
  que o grupo ancora nao esteja colado a letra nem seja parte de um UUID,
  rejeita grupo colado a letra do outro lado, para de crescer assim que o
  total ja basta, exige que ao menos um grupo tenha 4+ digitos, e recusa —
  so depois de a ponte parar de crescer, avaliando os grupos completos —
  intervalo numerico (`"100000-200000"`) e numero com separador de milhar
  (`"1 234 567 890"`). Para na hora se o trecho ja fechado for uma data
  (`AAAA-MM-DD`, a forma que o `CLAUDE.md` manda usar em prosa narrativa, ou
  `DD-MM-AAAA`), a menos que a ponte, espiando adiante ate o que resta do
  teto de grupos, alcance um grupo de 4+ digitos que complete um telefone
  plausivel (`"55-11-98-76-5432"`).
  Revisado em tres rodadas por `code-reviewer` e `security-auditor`
  independentes: a primeira versao publicada tinha uma regressao na
  protecao de UUID e nao cobria os formatos de celular BR/NANP mais comuns;
  a segunda corrigiu isso mas criou uma regressao nova (apagava todo
  celular NANP) e nao fechava de vez o bypass por prefixo de data; a
  terceira corrigiu ambos e recebeu veredito MERGE_READY (risco R1), com
  duas limitacoes residuais aceitas e documentadas em codigo: um numero
  real cujos grupos apos o primeiro sejam todos de exatamente 3 digitos
  ainda escapa (indistinguivel de numero com separador de milhar sem
  contexto semantico), e uma combinacao especifica de data + numero curto +
  grupo de 4+ digitos pode ser sobre-mascarada (nunca vaza, so reduz
  legibilidade de log). Nao cobre numero partido entre duas
  `MessagePart::Text` diferentes — isso fica para quem redige o conteudo
  agregado, fora do escopo desta funcao.
