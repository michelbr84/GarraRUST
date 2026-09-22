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
  `garra_status` antes de dizer que nao tem acesso a um canal, integracao ou
  ferramenta. A linha so entra quando a tool esta entre as oferecidas no
  turno, entao a CLI (`garraia chat`/`garraia ask`, que nao registram a tool)
  nunca a recebe. O relatorio do `garra_status` passar a enxergar o
  `whatsapp_linked` e os canais push fica para a fatia do gateway.
