- **`garra whatsapp` nao fica mais pendurado num bridge que sobe e nao fala
  (#1238).** O prazo de silencio, o contador do QR e o braco de Ctrl+C so
  comecavam DEPOIS do handshake com a ponte: `expect_started` e os dois
  comandos de abertura rodavam sem relogio e sem cancelamento. Um `node` da
  PATH que trava antes de rodar o bridge — shim de asdf, volta, nvm ou
  corepack baixando versao, stub de snap esperando confirmacao, wrapper que le
  stdin — deixava a tela parada nas instrucoes do QR para sempre, e nem o
  primeiro nem o segundo Ctrl+C faziam nada, porque o comando ja havia trocado
  o SIGINT default do sistema por um canal que ninguem estava lendo. A unica
  saida era `kill -9`, e ai o `Drop` que devolve a sessao arquivada nao rodava:
  num re-vinculo, a sessao boa ficava presa em `session.enc.prev`. Agora cada
  etapa anterior ao laco tem prazo e tem o cancelamento em primeiro lugar, o
  comando diz que o bridge nao respondeu e sugere conferir `node --version`, e
  uma linha de status sai antes do handshake para a tela nunca ficar muda. O
  mesmo tratamento vale para o modo `serve` do gateway, onde um handshake sem
  resposta pendurava o boot em vez de cair no backoff que existe para isso.
