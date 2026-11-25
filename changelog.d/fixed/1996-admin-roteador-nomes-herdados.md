- **Web Console: o roteador nao despacha mais para nomes herdados de `Object.prototype` (#1996).**
  `navigate()` em `admin.html` filtrava a pagina com `if (!pages[page])`, e `pages` e um
  objeto literal: `#__proto__`, `#constructor` e `#toString` passavam. Abrir o painel em
  `/admin#__proto__` lancava `TypeError` e deixava a tela presa no "loading". O filtro
  agora e por posse propria (`Object.prototype.hasOwnProperty.call`), e qualquer nome que
  o mapa nao declare cai no dashboard. Sem execucao de codigo no caminho antigo — so o
  travamento da tela de quem abria o link —, mas o alerta CodeQL #17
  (`js/unvalidated-dynamic-method-call`) estava certo e a dispensa como falso-positivo
  nao. Spec nova `tests/playwright/admin-router-hash.spec.ts`.
