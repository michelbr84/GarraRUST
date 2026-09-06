- **Erro no `garra chat` passa a dizer o que fazer (#941).** O #933 tirou o
  tracing do console, e isso criou uma divida: silenciar o log de rotina nao
  pode significar esconder a falha. Ate aqui saia `Erro: {mensagem crua do
  provedor}` e, para dois casos, uma dica escolhida por `err_str.contains(...)`
  solto no meio do laco do chat. Agora sai um cartao com o componente que
  falhou no titulo, a mensagem redigida no corpo e o proximo passo embaixo —
  oito classes (credencial, timeout, inalcancavel, modelo indisponivel, limite
  de taxa, permissao, provedor local fora do ar, desconhecida) em vez de duas.
- **Nao reconhecer nunca perde informacao.** Classe desconhecida cai num cartao
  generico que mostra a mensagem original **inteira**, e sem acao inventada.
  Trocar um erro feio por um erro invisivel seria pior, e ha teste afirmando
  isso.
- **Provedor local tem conserto proprio.** "Verifique a rede" para quem
  esqueceu de subir o Ollama e mandar investigar a coisa errada; o cartao
  sugere `ollama serve`. Rodar o binario contra um Ollama desligado mostrou que
  a frase real do `reqwest` e "error sending request for url" — que nao contem
  "connect" nem "refused", entao a lista de padroes escrita de cabeca perdia
  justamente o caso mais comum do projeto. O codigo antigo tambem o perdia.
- **Segredo nunca aparece no cartao.** Criterio de aceite da issue, e nao
  teorico: mensagem de erro de provedor e corpo de resposta HTTP, e ha provedor
  que ecoa o pedido com o `Authorization` dentro. O texto passa por
  `redact_secrets` e pelo filtro de controle do #996 — na origem **e** no
  renderer, porque confiar so na origem deixaria um jeito de errar para quem
  montar um cartao a mao.
- **A variante `UiEvent::Error` de uma linha saiu.** Depois do cartao ela nao
  tinha mais caso proprio: um erro sem proximo passo e um cartao de acoes
  vazias, e ganha do texto solto por nomear o componente. O aviso do `/model`
  migrou do `println!` com cor incondicional para o renderer, entao passou a
  respeitar `NO_COLOR` e pipe — uma linha a menos da divida que o plano de
  migracao da ADR 0017 registra.
