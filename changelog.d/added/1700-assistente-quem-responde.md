- **Assistente do Desktop configura quem responde, sem terminal (#1700).** Uma
  instalacao limpa pareava o WhatsApp e nunca respondia: nada configurava o
  provedor de LLM, e o aviso mandava rodar `garraia init`, que nem existe no
  PATH do AppImage. O passo novo "Quem responde" oferece o Ollama local
  (detectado em `127.0.0.1:11434`, com os modelos instalados) ou o OpenRouter
  (chave + modelo), grava pelo `garraia config set-routing` com a chave no
  stdin (nunca em argv, nunca de volta na webview) e testa com uma chamada real
  (`garraia ask --json`), com motivo e dica quando falha. A tela Pronto so diz
  "pronto para conversar" com o teste ok; senao diz que o WhatsApp conectou mas
  o Garra nao conversa, e por que. O `/api/health` passa a avisar quando nao ha
  provedor registrado, e a instrucao do primeiro "oi" segue o modo (conversa
  "Voce" no proprio numero, #1813).
