- **O gateway sobe a ponte do WhatsApp vinculado deste binario, e nao a da
  versao anterior (#1373).** So o `garraia whatsapp link` materializava o
  `bridge.mjs`, o `package.json` e o `package-lock.json` embutidos; depois de
  um `garraia update` o gateway seguia lancando o `bridge.mjs` que estava no
  disco (o da 0.4.4 nao tem o conserto do `@lid`) ate alguem vincular de novo.
  Agora o supervisor do canal regrava, antes de lancar a ponte, so os arquivos
  que diferem do embutido (diretorio `0700` e allowlist de nome mantidos, I/O
  em `spawn_blocking`), e roda o `npm ci` so quando falta `node_modules` ou
  quando nada prova que a arvore e a do lock embutido. Provam o carimbo
  `.garraia-deps-sha256` (gravado quando o `npm ci` do GarraIA sai 0, e posto
  em `pending` antes de um manifesto ser reescrito e antes de cada `npm ci`) ou
  o registro do proprio npm, `node_modules/.package-lock.json`, quando lista
  exatamente os pacotes do lock embutido e nao e mais velho que a arvore - o
  que adota sem `npm` uma instalacao da 0.4.4 cujo `npm ci` terminou (e nao
  mais uma pela metade) e o `npm ci` que o usuario roda a mao. Se o `npm` do
  gateway falhar, a ponte nao sobe e a arvore que ele deixou pela metade sai do
  disco; sem `npm` na PATH do gateway, a ponte nao sobe e nada e apagado. Nos
  dois casos o `/api/diagnostics` e o `garraia whatsapp status` mostram "sem
  dependencias" com o passo `rode npm ci em <dir> e reinicie o gateway`, que
  agora funciona: o boot seguinte adota a arvore. A sessao vinculada nao e
  tocada. O `garraia whatsapp link` usa a mesma regra, e ja nao roda `npm` so
  porque o `bridge.mjs` mudou.
