- **`garra_status` passa a chegar ao modelo nos modos restritos, com a
  instrucao de consulta-lo antes de negar uma integracao (#1347, fatia 1).**
  Com o `whatsapp_linked` conectado, o Garra respondia "nao tenho acesso ao
  WhatsApp": o piso do canal e o modo `search`, cuja allowlist nao tinha
  `garra_status`, e o runtime tira da lista tudo que o modo nao permite; alem
  disso o prompt do modo substitui a persona, o unico lugar que mandava usar
  a tool. Agora `garra_status` (leitura R0 do proprio runtime, sem I/O nem
  rede) entra na allowlist de `search`, `architect`, `debug`,
  `orchestrator`, `review` e `edit` — `denied` num modo customizado continua
  tirando-a —, e o `AgentRuntime` acrescenta ao prompt de sistema que venceu,
  depois dele e sem substitui-lo, uma linha pedindo que o modelo chame
  `garra_status` antes de dizer que nao tem acesso a um canal ou integracao:
  um canal presente na lista `channels` do relatorio esta conectado, um
  campo `status` por canal (quando existir) so conta como conectado em
  `active`, e um canal ausente da lista nao basta para negar o acesso. A
  linha nao fala de ferramentas, porque a lista oferecida no turno e a
  fonte de verdade delas, e o `tools` do relatorio do `garra_status` passa
  a trazer so as ferramentas que o portao do turno libera, e nao mais toda
  tool registrada: no piso `search` o relatorio nao lista mais `bash` nem
  `file_write`, que o turno nega. A linha so entra quando a tool esta entre
  as oferecidas no turno, entao a CLI (`garraia chat`/`garraia ask`, que nao
  registram a tool) nunca a recebe. O sintoma do WhatsApp nao some so com
  esta fatia: o relatorio ainda nao enxerga o `whatsapp_linked` nem os
  canais push, e isso fica para a fatia do gateway.
