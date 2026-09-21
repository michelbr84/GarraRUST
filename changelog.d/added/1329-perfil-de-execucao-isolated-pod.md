- **Perfil de execucao isolated-pod: poder total dentro do pod, nada implicito
  fora (#1329, ADR 0024).** O Garra passa a ter dois perfis de execucao.
  `standard` (default, secao ausente) e a postura de hoje para maquina
  compartilhada. `isolated-pod` e a declaracao explicita do operador de que o
  processo roda num pod/container descartavel — e o pod, nao o Garra, e a
  fronteira de seguranca. Liga-se por `execution.profile: isolated-pod` no
  `config.yml` ou pela env `GARRAIA_EXECUTION_PROFILE`, que vence o arquivo e
  nunca e gravada nele; valor invalido recusa o boot em vez de cair em
  `standard` em silencio. Nenhum codigo le `/.dockerenv`, cgroup ou env de
  runtime de container para decidir o perfil — testes varrem o fonte.
  No WhatsApp pessoal, a nova chave `channels.whatsapp_linked.owners`
  (mesma normalizacao de `allow`) declara o dono: em `isolated-pod`, e so em
  conversa 1:1, ele recebe o piso `code` (`bash`, `file_write`, subagentes e
  toda tool MCP) em vez de `search`; grupo, contato so pareado e identidade
  desconhecida continuam no piso `standard`, e `owners` fora de
  `isolated-pod` e apenas um `Warning` no `config check`. O MCP `filesystem`
  autoprovisionado deixa de nascer em `$HOME` em qualquer perfil: em
  `standard` usa `agent.file_roots` ou `<data_dir>/workspace`; em
  `isolated-pod` usa `execution.pod_root` ou o mesmo workspace; um `mcp.json`
  anterior nunca e reescrito, e o diagnostico avisa quando ele ainda aponta
  para fora do jail. O jail das file tools nativas, o gate de comando
  arriscado do `bash` e `agent.sandbox` continuam ligados: o perfil libera
  ferramentas, nao desliga protecoes. Observabilidade: `WARN` unico no boot
  em `isolated-pod` (o que foi liberado, o que o perfil NAO isola, como
  reverter); checks `execution.profile` e `mcp.filesystem_root` em
  `/api/diagnostics`; linha read-only `security.execution_profile` em
  `/api/settings/effective`; perfil, piso do dono e contagem de donos em
  `garra whatsapp status`; perfil e origem no sumario do `config check`;
  cada turno do WhatsApp loga `phone_last4` + perfil + piso. A instrucao
  pos-link do `garra whatsapp` usa o nome do executavel em execucao
  (`garraia` ou `garra`). Guia: `docs/execution-profiles.md`.
