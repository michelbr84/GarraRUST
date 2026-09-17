- **`garra whatsapp link` e `garra whatsapp cloud` num pipe deixam de devolver
  o comando que voce acabou de rodar (#1238).** Sem terminal, os tres comandos
  imprimiam o MESMO texto, byte a byte — e esse texto termina mandando rodar
  `garra whatsapp link`. Medido no binario: o `diff` entre as saidas de
  `whatsapp`, `whatsapp link` e `whatsapp cloud` era vazio, e os tres saiam 0.
  Quem conecta um GarraIA headless com `ssh servidor 'garra whatsapp link'`
  caia num ciclo fechado: sem QR, sem erro, e com um exit code afirmando que
  tinha dado certo. Agora quem JA escolheu o fluxo recebe o motivo (o QR se le
  deste terminal e o consentimento se da nele), a saida (`ssh -t <usuario>@
  <maquina> garra whatsapp link`) e exit 69 `EX_UNAVAILABLE` em vez de 0. O
  `garra whatsapp` sem escolha continua saindo 0 com as duas opcoes: ali o
  hint E a resposta, nao uma repeticao.
