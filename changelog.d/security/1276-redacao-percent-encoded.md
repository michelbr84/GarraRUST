- **A redacao do stderr da ponte do WhatsApp deixava a chave inteira passar
  quando ela vinha percent-encoded ou com a barra escapada (#1276, item 2).**
  A regra corta qualquer segmento com 40+ caracteres de base64, e `%` e `\`
  nao sao caracteres de segmento — `100%` e o caminho do Windows dependem
  disso. So que uma chave na URL de um `FetchError` do undici
  (`https://mmg.whatsapp.net/v/t62.7118-24/<chave>?ccb=11-4`) chega com `+`,
  `/` e `=` percent-encoded (`%2B`/`%2F`/`%3D`), e uma chave que passou por
  `JSON.stringify` chega com `\/`: cada ocorrencia quebrava a sequencia em
  pedacos curtos demais para o teto, os pedacos saiam em claro e bastava um
  url-decode (ou um unescape) do texto ja redigido para remontar a chave.
  Medido sobre chaves aleatorias de 32 B: 62,9% das chaves percent-encoded e
  40,8% das chaves com `\/` chegavam INTEIRAS a tela. As saidas obvias foram
  medidas e descartadas — mexer no teto de 40 nao muda a fracao, porque o
  problema e onde o segmento quebra e nao o tamanho dele, e exigir mistura de
  classes de caractere deixa 0,07% das chaves em claro. O tokenizador de
  segmentos agora enxerga as duas codificacoes: `%2B`/`%2F`/`%3D` (qualquer
  caixa) e `\/` contam como UM caractere do segmento, o teto e o `N` do
  marcador `<redigido: N caracteres>` sao sobre o comprimento decodificado, e
  um segmento curto sai como entrou, sem decodificar. `%` seguido de qualquer
  outra coisa (`%20`, `%25`, `100%`) e `\` seguido de qualquer coisa que nao
  `/` continuam separadores, entao caminho do Windows e URL do registry npm
  (`@whiskeysockets%2Fbaileys`) saem inteiros. Um teste de corpus refaz a
  medicao da issue com 500 chaves de semente fixa — 61,0% e 37,6% no codigo
  antigo — e exige zero chaves recuperaveis nas duas codificacoes, com base64
  padrao e url-safe 100% redigidos como regressao. Fica, por escrito, o
  limite declarado: credencial com menos de 40 caracteres decodificados nao e
  redigida.
