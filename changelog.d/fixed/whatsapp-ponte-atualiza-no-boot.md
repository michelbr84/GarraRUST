- **O gateway sobe a ponte do WhatsApp vinculado deste binario, e nao a da
  versao anterior.** So o `garraia whatsapp link` materializava o
  `bridge.mjs`, o `package.json` e o `package-lock.json` embutidos; depois de
  um `garraia update` o gateway seguia lancando o `bridge.mjs` que estava no
  disco (o da 0.4.4 nao tem o conserto do `@lid`) ate alguem vincular de novo.
  Agora o supervisor do canal regrava, antes de lancar a ponte, so os arquivos
  que diferem do embutido (diretorio `0700` e allowlist de nome mantidos), e
  roda o `npm ci` apenas quando o `package.json`/`package-lock.json` mudou,
  quando falta `node_modules` ou quando um `npm ci` anterior nao terminou: um
  carimbo `.garraia-deps-sha256` diz para quais manifestos o `node_modules`
  foi instalado, e uma instalacao da 0.4.4 com os mesmos manifestos e adotada
  sem `npm`. Se o `npm` falhar ou nao estiver na PATH, a ponte nao sobe (nunca
  contra dependencias de outra versao), o `node_modules` que sobrou sai do
  disco e o `/api/diagnostics` e o `garraia whatsapp status` mostram "sem
  dependencias" com o passo `rode npm ci em <dir>`; a sessao vinculada nao e
  tocada. O `garraia whatsapp link` usa a mesma regra, e ja nao roda `npm` so
  porque o `bridge.mjs` mudou.
