- **O wizard `garra whatsapp link` oferece a politica de acesso depois do QR
  (#1429; ADR 0025).** Depois de perguntar o primeiro numero (e o dono, em
  `isolated-pod`), o wizard passa a perguntar, com defaults seguros: o nivel
  de quem acabou de entrar (`chat` | `read` | `full`, default `read`) e, fora
  de `chat`, a escrita de arquivo (default nao); e a admissao — so quem voce
  autorizar (`restricted`, default) ou qualquer numero (`open`). Escolher
  `open` mostra o default que um desconhecido recebe NESTA config e pede a
  mesma confirmacao do `access open`, com default nao. Cada resposta e
  gravada pelo mesmo motor do `access` (`whatsapp_linked_politica::mutacao`:
  validacao, escrita atomica `0600`, audit com origem `cli`) — o wizard nao
  tem uma segunda forma de escrever politica, e Enter em tudo deixa o numero
  com `read` sem escrita e o canal fechado. Dono nao recebe pergunta de nivel
  (dono nao tem teto). `link --allow <numero> [--owner]` continua identico e
  scriptavel: nada novo e perguntado, o numero entra sem teto como o `allow`
  gravaria. O resumo final passa a ser o da politica efetiva (as linhas do
  `access`), no lugar do resumo do `users`.
