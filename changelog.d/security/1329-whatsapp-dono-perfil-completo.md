- **WhatsApp pessoal: o dono declarado ganha o perfil completo dentro de um
  pod isolado, e mais ninguem (ADR 0024, #1329).** Nova chave
  `channels.whatsapp_linked.owners` (mesma normalizacao do `allow`; quem esta
  la e admitido como se estivesse no `allow`). O perfil do turno e uma funcao
  pura avaliada depois da admissao e antes do `ExecContext`: `completo` so
  quando `execution.profile = isolated-pod` **e** a conversa e 1:1 **e** o
  remetente esta em `owners` — e ai o piso e `code` (sem whitelist:
  filesystem, `bash`, servidores MCP, subagentes), salvo `default_mode`
  declarado. Todo o resto — remetente admitido que nao e dono, dono falando
  por grupo, contato so pareado por codigo, `owners` num processo em
  `standard` — fica exatamente onde esta hoje (`search`). `default_mode`
  passa a ser opcional na struct: ausente, o default depende do perfil
  (`search` em `standard`, `code` para o dono em `isolated-pod`), e a
  validacao na subida (`ModoPadraoInvalido`) continua identica nos dois
  perfis. Cada turno loga `phone_last4` + `perfil` (`completo` | `padrao`)
  + o modo do piso, nunca JID, telefone ou texto. O aviso de drift de MCP na
  subida passa a falar por perfil: em `isolated-pod` com dono ele diz que a
  liberacao e decisao de perfil, nao erro; sem dono, avisa que o perfil nao
  muda nada neste canal. Provado com a fiacao real (ponte falsa, sink,
  runtime, dispatch de ferramenta): o dono no pod invoca
  `filesystem__write_file` e a ferramenta executa; tirar o dono de `owners`,
  voltar o perfil para `standard` ou mandar do grupo deixa a ferramenta sem
  rodar e a resposta ainda sai. Uma varredura de fonte garante que a recusa
  antiga por servidor MCP nao volta e que o canal nao le marcador de
  container para decidir nada.
