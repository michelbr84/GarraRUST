- **Desktop e `garraia start` com outro gateway na porta 3888 (#1816).** O
  Desktop conectava em silencio num gateway de outra instalacao (no teste da
  rc2, o servico 0.5.1 do systemd) e ainda subia o proprio sidecar, que rodava
  o boot inteiro (canais e a ponte do WhatsApp com a mesma sessao) so para
  morrer no bind, apagando o pidfile do outro. Agora o app detecta o gateway
  pelo `/api/health`, nao sobe outro, avisa a versao que esta atendendo e nao
  reinicia o que nao e dele; o `garraia start` recusa porta ocupada antes do
  boot e do pidfile (exit 69), dizendo se e um gateway GarraIA e qual. As notas
  da tela de modo do assistente, que ficavam escondidas, voltam a aparecer.
