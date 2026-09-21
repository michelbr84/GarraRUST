- **A doc do sandbox por tool nao insinua mais que OpenShell/Crabbox estao
  chegando (#1225).** O comentario de modulo de `garraia-agents::sandbox`
  remetia a fatia S6 a issue de tracking como assunto em aberto, o que lia
  como entrega pendente. O texto agora nomeia os tres backends que existem
  de fato (`Docker`, `Podman`, `Ssh`) e deixa claro que fechar OpenShell e
  Crabbox como won't-do e a recomendacao registrada em #1225 — a decisao
  final continua sendo do dono. Tambem corrige a descricao de
  `backend: None`: com sandbox obrigatorio, o comando e recusado
  fail-closed, nao roda no host como um quarto modo implicito. Mudanca so
  de documentacao, sem efeito em comportamento.
