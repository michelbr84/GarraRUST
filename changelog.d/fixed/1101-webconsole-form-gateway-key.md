- **O form de gateway key volta a aparecer quando a autenticacao e exigida (#1100).**
  O CSS deixa `.gateway-key-form` escondida com `display: none`, e o boot apenas
  limpava o estilo inline (`authSection.style.display = ''`), o que nunca sobrepoe a
  folha de estilo. Com `gateway.api_key` configurada o console avisava "Gateway API Key
  required." sem nenhum campo visivel para digitar a chave, e o WebSocket reconectava
  em loop com 401. Passa a usar `display: 'block'` explicito em `webchat.html` e
  `assets/app.js`, que duplicam a mesma logica.
