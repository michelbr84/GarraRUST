# Novidades da v0.4.5

> 🇬🇧 [English version](Whats-New-v0.4.5) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.5)

A release que tira o `bash` irrestrito de onde não há humano para confirmar e
faz funcionar o que a v0.4.4 deixava pela metade. No perfil `standard`, o
`garraia mcp-server` e o gateway só oferecem shell ao modelo dentro de um
sandbox `docker`/`podman`. O "sim" a um pedido de confirmação volta a aprovar
em todo canal com uma pessoa do outro lado. O `garraia whatsapp link` pergunta
quem pode falar com o GarraIA. E o boot passa a **recusar** duas
configurações que antes subiam abertas: bind exposto sem credencial e TLS pela
metade.

---

## `bash` só com sandbox onde não há humano no laço (#1272)

Antes, em `standard`, o `garraia mcp-server` e o runtime do gateway
registravam um `bash` que rodava no host. O tier de comando arriscado só pega o
que *parece* perigoso: um `cat /etc/shadow` ou um `echo x > /qualquer/lugar`
pedido pelo modelo passava. Agora a decisão é uma função pura do perfil e do
`agent.sandbox`, e nunca vem de detectar container:

| `agent.sandbox` para o `bash` | `standard` (default) | `isolated-pod` (explícito) |
| --- | --- | --- |
| Exigido e utilizável: `docker`/`podman` com `mode: all` (sem `bash` em `elevated`) ou `allowlist` com `bash`, binário presente | registrado; todo comando no container, sem fallback para o host | registrado, no sandbox |
| Não exigido: `mode: off` (o default), `bash` em `elevated`, `allowlist` sem `bash` | **não existe** | registrado no host do pod, com denylist e tier arriscado ligados |
| Exigido mas inutilizável: `backend: ssh`, sem backend, binário ausente | **não existe** | **não existe**, com o motivo real no aviso |

- **A mesma regra vale para o `run_tests` do gateway**, que roda o
  `scripts.test` do `package.json`, o `build.rs` e o `conftest.py`, todos
  arquivos que o `file_write` consegue escrever.
- **Sem sandbox não há shell.** O boot avisa uma vez com `warn!` dizendo por
  que e como religar, o `/api/diagnostics` ganha o check `tools.bash` e o
  system prompt do `garra_agent` diz ao modelo que não há shell.
- **O container virou fronteira de verdade.** Ele recebe `--cap-drop ALL`,
  `--pids-limit 512` e `--user <uid>:<gid>` do operador (`--userns=keep-id` no
  podman). Só o diretório de trabalho canônico da sessão é montado: `/`, o
  `$HOME`, um ancestral dele ou uma sessão sem `working_dir` são recusa. No
  timeout o container é removido.
- **`git_diff` e `code_review` não executam programa plantado no repositório.**
  O git roda sem fsmonitor, hooks, textconv, filtros nem submódulos, e o
  `file_write` recusa qualquer caminho com componente `.git`.
- O `garraia chat` não muda: ali há um humano no terminal que confirma.

Decisão registrada no Amendment 2026-09-21 do
[ADR 0024](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0024-perfis-de-execucao-isolated-pod.md).

## O "sim" volta a aprovar, em todo canal com humano (#1343)

Quando uma ferramenta pedia confirmação (um `bash` arriscado com
`agent.tool_confirmation_enabled`, um `device_execute` R3/R4), o turno pausava
e o "sim" da mensagem seguinte não aprovava nada. Os canais guardam o
histórico como texto, então o pedido pausado não voltava e a ferramenta
perguntava de novo para sempre. Agora o pedido fica em memória. A mensagem
seguinte, se for **inteira** uma palavra de aprovação (`sim`, `yes`, `ok`,
`confirma`, `confirmar`, `proceed`, `approve`), roda o pedido **uma vez**,
dentro de **5 minutos**, e só se vier do mesmo remetente, na mesma sessão e no
mesmo canal:

| Onde | Quem pode aprovar |
| --- | --- |
| Web Console (`/ws`) e desktop (`/ws/parrot`) | a mesma conexão; reconectou, pergunta de novo |
| `/v1/chat/completions` (com e sem `"stream": true`) | o dono, com o **mesmo** `Authorization` e a **mesma** `X-Session-Id` nos dois requests |
| App mobile (`POST /chat`) | o `sub` do JWT |
| Telegram, Discord, Slack, WhatsApp, Matrix, IRC, Signal, LINE, Teams, Google Chat, iMessage, WhatsApp pessoal | o id do usuário na plataforma |
| `garraia chat` | o próprio terminal |

- Em grupo, o "sim" de outro membro não aprova e **encerra** o pedido.
- Qualquer outra mensagem no meio, "não" inclusive, encerra o pedido (#1340).
  Um segundo "sim" pausa de novo, e reiniciar o gateway cancela os pendentes.
- Continuam sem retomada, de propósito:
  - A2A e OpenClaw;
  - `POST /api/sessions/{id}/messages`;
  - a resposta do agente no chat do workspace e as tarefas agendadas;
  - `garraia ask` e o `garra_agent` do `garraia mcp-server`.
- **O pedido chega sem o marcador interno** (#1373): o usuário não vê mais
  `[CONFIRM_REQUIRED:…]` na mensagem nem na linha da ferramenta do
  `garraia chat`. A frase continua dizendo o comando e "Responda **sim** para
  executar".
- **O telefone sai mascarado no log.** O id de sessão do WhatsApp e do Signal
  embute o número de quem fala, e os spans do runtime o gravavam inteiro. O
  `RedactingWriter` do stderr e do `garraia.log` agora troca toda sequência de
  10 ou mais dígitos que não esteja colada a letra por `…` e os 4 últimos.

Matriz completa de recusas:
[`docs/security/threat-model.md` §5.16](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md).

## WhatsApp pessoal: quem pode falar, e a ponte que se atualiza

- **O `link` pergunta quem pode falar com o GarraIA** (#1345). Depois do QR, o
  comando pede o número com o código do país e só diz "pronto" quando há alguém
  autorizado. Resposta vazia deixa o portão fechado, com o aviso e o comando que
  resolve depois.
- **`garraia whatsapp allow <número> [--owner] [--yes]`** autoriza sem
  terminal. O número exige `+` e código do país, e `<id>@lid` também é aceito.
  Os códigos de saída são 65 para número inválido e 64 para `--owner` fora de
  `isolated-pod` ou num pipe sem `--yes`. O dono só é oferecido em
  `isolated-pod`, com default não.
- **Autorizar e revogar valem sem reiniciar.** O gateway relê `allow`, `owners`
  e `enabled` do `config.yml` a cada mensagem. Para revogar, apague o número
  da lista no arquivo. `enabled: false` recusa todo mundo na mensagem seguinte.
  Celular brasileiro com e sem o nono dígito casa como o mesmo número.
- **Status honesto.** `garraia whatsapp status` mostra
  `Autorizados: N · Donos: M` (contagens, nunca números). O `/api/diagnostics`
  e o log de boot avisam quando ninguém está autorizado.
- **A ponte se atualiza no boot** (#1373). Depois de um `garraia update`, o
  gateway regrava a ponte embutida no binário antes de lançá-la. Antes, ele
  seguia lançando o `bridge.mjs` da versão anterior até alguém vincular de
  novo. O `npm ci` só roda quando falta `node_modules` ou quando nada prova que
  a árvore instalada é a do lock embutido.
- **O log mostra o final certo do número conectado** (#1373). Antes, o sufixo
  de aparelho do JID entrava na conta.
- **O erro de ponte morta volta a trazer a cauda do stderr do Node** (#1368).

## MCP `filesystem` sem cache do npx corrompido (#1346)

- A provisão de instalação nova fixa
  `@modelcontextprotocol/server-filesystem@2026.8.31`, em vez de baixar o build
  mais novo a cada cache frio. Um `mcp.json` existente nunca é reescrito: o
  check `mcp.filesystem_pinned` do `/api/diagnostics` avisa quem ficou sem
  versão e traz os `args` exatos para colar.
- Um `ERR_MODULE_NOT_FOUND` dentro de `<cache npm>/_npx/<hash>/` apaga só aquela
  entrada, validada, e tenta de novo uma vez.
- Esgotados os `max_restarts`, o erro sai uma vez só, e não a cada 30 s.
- `/api/mcp/health` passa a listar servidores que falharam no boot, com
  `status`, `cause` e `last_error`. O check `mcp.servers` traz o próximo passo
  de cada causa.

## O Garra sabe o que tem: `garra_status` (#1347)

Com o WhatsApp vinculado conectado, o Garra respondia "não tenho acesso ao
WhatsApp". Três coisas mudaram:

- **`garra_status` entra nos modos com whitelist** (`search`, `architect`,
  `debug`, `orchestrator`, `review`, `edit`). O runtime acrescenta ao prompt a
  instrução de consultá-lo antes de negar acesso a um canal.
- **O relatório vê o que o `/api/channels` vê.** A lista inclui o
  `whatsapp_linked` e os canais push, com `status` (`active`/`offline`), e sai
  da mesma função. Numa instalação nova ela vem vazia, em vez de oito canais
  `offline`. O relatório ganha `execution_profile`, `mcp_servers` (só nome,
  estado e contagem de ferramentas) e `session.channel`, e o `session.id` sai
  mascarado.
- **O que é do operador fica com o operador.** O relatório retém
  `working_dir`, `project_id`, a lista de provedores, os nomes dos servidores
  MCP e a versão exata, e lista o que reteve em `withheld`, em dois casos:
  - num turno de portão restrito, como o piso `search` do WhatsApp;
  - numa sessão que não é provadamente do operador: app mobile, A2A, canais de
    mensagem, sessão desconhecida, e gateway exposto pelo opt-out sem chave.
- O turno em streaming passa a aplicar o prompt e o `max_tokens` do modo, como
  o caminho em lote.

## Um boot que recusa o que falharia aberto (#1261, #1247)

- **Bind exposto sem credencial não sobe.** `garraia start`, `start -d` e
  `restart` saem com exit 78 quando algum endereço do bind não é loopback e não
  há `gateway.api_key` nem `GARRAIA_GATEWAY_API_KEY`. A recusa vem antes do
  bind, antes do fork em `-d` (a mensagem chega ao terminal) e antes de o
  `restart` derrubar o daemon atual. TLS não isenta, e um nome que não resolve
  também é recusado.
- **Nova env `GARRAIA_GATEWAY_API_KEY`.** Ela vence o arquivo, vazia conta como
  ausente e nunca é gravada no `config.yml`. O `config check` a reporta só por
  presença.
- **Opt-out consciente**: `gateway.allow_unauthenticated_network_bind: true`,
  só no arquivo, sem env nem flag, e com aviso alto em todo boot.
- **`gateway.host`/`gateway.port` do arquivo ficam deprecados.** As chaves
  nunca alimentaram o bind. O `garraia init` para de escrevê-las e as remove
  num re-run, e `status`, `stop`, `doctor` e `admin` usam o mesmo endereço do
  `start`. O `garraia restart` passa a ler `HOST`/`PORT`, como o `start`.
- **O `config check` roda em todo boot.** `start`, `restart` e `start -d` rodam
  o mesmo check uma vez: `Error` vai para o log como erro, `Warning` como aviso.
  No `start -d` as linhas vão para o stderr antes do fork.
- **Só uma lista fechada recusa o boot**: hoje, o TLS configurado pela metade
  (só `tls_cert_path` ou só `tls_key_path`), que antes servia HTTP puro em
  silêncio. Todo outro `Error` é dito e não derruba nada.

## Sandbox, runs e runtime

- **Sandbox cobre as tools de repositório** (#1225). Com
  `agent.sandbox.mode: all`, `run_tests`, `git_diff`, `code_review` e
  `repo_search` rodam no container, com argv montado sem shell. O `backend: ssh`
  é recusado para elas.
- **Ledger de runs** (#1227):
  - `garraia runs list` lê o `sessions.db` sem falar com o gateway, com
    `--status`, `--limit` (padrão 50) e `--json`;
  - `GET /api/runs` é só leitura, com prévias de até 120 caracteres e acesso
    mais estrito que o resto de `/api/*`;
  - `runs.retention_days` liga uma varredura (no boot e a cada 24 h) que apaga
    runs terminais antigos. O default `0` nunca apaga, e um run `running`
    nunca é apagado.
- **Duas crates mortas saem** (#1226): `garraia-tools` e `garraia-runtime`. O
  workspace fica com 22 crates, e o snapshot do `garraia max-power` anuncia o
  `tool_program`.
- **O detector de loop avisa antes de abortar** (#1295). Na primeira repetição
  (mesma ferramenta, mesmos argumentos, três vezes seguidas), o modelo recebe
  uma observação corretiva em vez de o turno morrer. Qualquer detecção seguinte
  na mesma tarefa aborta como antes, e a chamada barrada nunca roda.
- **`garraia max-power`** (#1228). O `--goal` não aborta mais com "Cannot start
  a runtime from within a runtime", e sem provider utilizável roda offline,
  como a ajuda promete. O comando grava no `garraia.log`, e o erro de corpo de
  um provider OpenAI-compatível diz a causa real.

## Correções da instalação limpa

O smoke de instalação limpa da v0.4.4 achou estas:

- **`install.sh` em container e CI** (#1369). Sem terminal de verdade, o
  instalador segue o caminho não interativo sem linhas de erro. Um `CI` não
  vazio também conta como sem terminal, como no `install.ps1`.
- **`garraia.log` preservado** (#1371). O `garraia start -d` abre o log em
  append, em vez de apagar a execução anterior e escrever por cima da cabeça
  do arquivo.
- **`garraia status | head` sai em silêncio** (#1371), sem panic de broken
  pipe, em todos os comandos que só leem e imprimem. O
  `GET /admin/api/logs` lê só os últimos 512 KiB e não responde mais 500 com
  byte que não é UTF-8.
- **Sessão REST sobrevive ao restart** (#1372). `GET .../history`,
  `POST .../messages` e `DELETE /api/sessions/{id}` param de dar 404 para uma
  sessão que está no `sessions.db`. O `DELETE` grava a marca `api_logout`, e
  uma sessão encerrada não volta.
- **A chave de um provider só vai para o endpoint da própria entrada `llm:`**
  (#1370). `garraia ask -p openai` mandava `llm.openai.api_key` para
  `https://api.openai.com` mesmo com `base_url` própria, e a auditoria achou a
  mesma classe de defeito em mais quatro caminhos da CLI. A variável de
  ambiente do tipo (`OPENAI_API_KEY`, …) só vai para o host padrão do tipo.

## Infra e processo

- **Quality Ratchet** (#1254): `freeze-baseline.py` ganha
  `--adopt-current-file-metrics --reason '#NNN'`. A flag adota só as métricas
  de tamanho de arquivo e registra a origem. Audit, cobertura e clippy seguem no
  ratchet estrito. O re-baseline em si (#1376) rodou no `main` logo antes da
  tag: o ratchet deixa de repetir a deriva de meses em todo PR, o teto de 3500
  linhas não subiu, e os 16 arquivos acima de 2500 linhas ficam registrados
  como dívida aceita no `.quality/README.md`.
- **PR sem fragmento em `changelog.d/` fica vermelho**, com isenção pela label
  `no-changelog`.
- **Cobertura e ratchet editam um comentário só por PR**, em vez de postar um
  novo a cada push.
- **O Swagger UI vem vendored**, sem download no build.
- **A release volta a subir o AppImage aarch64 da CLI** (#1344), e o CI confere
  a lista de upload.

## Atualizando da v0.4.4

```bash
garraia update
```

O que uma instalação existente encontra ao subir a v0.4.5:

- **Bind exposto sem credencial não sobe mais** (exit 78, com a mensagem de
  como corrigir). Instalações do `install.sh` e a unit systemd ligam em
  loopback e não mudam. Para quem expõe a porta, faça **antes** de atualizar:
  - defina `gateway.api_key` no arquivo, ou exporte
    `GARRAIA_GATEWAY_API_KEY="$(openssl rand -hex 32)"`. O `garraia init`
    também grava a chave quando a máquina é servidor (root ou pod RunPod) ou
    quando o `HOST` não é loopback;
  - ou volte a ligar só em loopback, com `garraia start --host 127.0.0.1`, e
    tire `HOST` do ambiente;
  - a **imagem Docker** (o `CMD` liga em `0.0.0.0`), os `docker-compose*.yml`
    e os **pods RunPod** precisam de `GARRAIA_GATEWAY_API_KEY` no ambiente.
    Sem ela o container sai com 78 e o `restart: unless-stopped` entra em loop:
    `docker compose ps` mostra `Restarting`. O `.env.example` traz a linha
    vazia de propósito, para você preencher;
  - **Helm**: bloco `gatewayApiKey` (gerado no install e preservado no upgrade,
    ou `existingSecret`/`value`). `helm template`/ArgoCD precisam de um dos
    dois;
  - **Terraform/ECS**: variável obrigatória `gateway_api_key_secret_arn`, sem a
    qual o `terraform plan` falha antes de trocar a imagem;
  - atrás de um proxy que autentica, com a porta aberta de propósito, use
    `gateway.allow_unauthenticated_network_bind: true` no arquivo;
  - `/ping`, `/health` e `/api/health` seguem abertos para healthcheck, e os
    clientes mandam a chave como `Authorization: Bearer <chave>`.
- **`garraia restart` passa a ler `HOST`/`PORT`.** Um daemon reiniciado num pod
  com `HOST=0.0.0.0` agora liga no endereço exposto, em vez de cair em loopback
  em silêncio, e portanto também precisa da credencial.
- **Erros de config que bloqueiam o boot.** Com só `gateway.tls_cert_path` ou só
  `gateway.tls_key_path`, `start`, `restart` e `start -d` saem com exit 78
  nomeando o campo que falta. Para subir mesmo assim, use
  `GARRAIA_ALLOW_INVALID_CONFIG=1` (exatamente `1`); o achado segue logado como
  erro. A escotilha **não** desliga a recusa do bind exposto. Todo outro `Error`
  do `config check` agora aparece no log de cada boot, mas não impede a subida.
  `memory.retention` com `interval_hours` ou `max_age_days` fora da faixa não
  derruba mais o worker com panic: a varredura não sobe e nada é apagado.
- **`garraia mcp-server` e o gateway, em `standard` e sem sandbox, não têm
  `bash`** (nem o `run_tests` do gateway). O boot avisa uma vez e o
  `/api/diagnostics` mostra `tools.bash`. Para religar, contenha o shell:
  ```yaml
  agent:
    sandbox:
      mode: all
      backend: docker   # ou podman; o binário precisa estar instalado
  ```
  Ou, **só** se o processo roda num pod descartável, declare
  `execution.profile: isolated-pod`. O `file_write` passa a recusar caminhos
  com `.git`.
- **`agent.sandbox.mode: all` agora também contém `run_tests`, `git_diff`,
  `code_review` e `repo_search`.** A imagem default (`debian:bookworm-slim`) só
  tem `grep`, então aponte `agent.sandbox.image` para uma imagem com
  `git`/`cargo`/`rg` ou liste a tool em `agent.sandbox.elevated`. Com
  `network_disabled: true` o `cargo` não baixa crates.
- **O primeiro boot do gateway regrava a ponte do WhatsApp.** O gateway troca
  pelos embutidos os arquivos que diferem (entre a 0.4.4 e a 0.4.5 só o
  `bridge.mjs` mudou) e roda `npm ci` só se faltar `node_modules` ou se nada
  provar que a árvore é a do lock. Uma instalação da 0.4.4 cujo `npm ci`
  terminou é adotada sem `npm`, pelo `node_modules/.package-lock.json`. Se o
  `npm ci` for necessário, o `npm` precisa estar na `PATH` do processo do
  gateway. Sem ele, ou se ele falhar, a ponte não sobe. O
  `garraia whatsapp status` mostra `Ponte:    dependências faltando`, e o
  `/api/diagnostics` traz o passo `rode npm ci em <dir> e reinicie o gateway`.
  Rode `npm ci` naquele diretório, num shell com `npm`, e reinicie o gateway,
  que adota a árvore no boot. A sessão vinculada não é tocada.
- **WhatsApp com `allow` vazio.** O gateway sempre descartou essas mensagens,
  mas agora avisa no boot, no `status` e no `/api/diagnostics`. Rode
  `garraia whatsapp allow +<país><número>`. Edições em `allow`/`owners`/`enabled`
  passam a valer na mensagem seguinte, e um `config.yml` que não parseia
  mantém a lista anterior. O `garraia whatsapp allow` reescreve o arquivo, e
  comentários não ficam.
- **Sessões do app mobile contam como restritas no `garra_status`.**
  `/auth/register` é aberto, e ter conta no gateway não prova ser o operador:
  nessas sessões o relatório retém diretório, `project_id`, provedores, nomes
  dos servidores MCP e versão exata. O mesmo vale para web, API, VS Code e
  desktop num gateway exposto pelo opt-out sem chave.
- **Retomar uma aprovação em `/v1/chat/completions` exige `X-Session-Id`
  estável.** Sem ela, cada request é uma sessão nova e o "sim" não aprova nada.
  Mande a mesma `X-Session-Id` e o mesmo `Authorization` no pedido e no "sim".
  Sem dono reivindicado na allowlist, a pausa segue terminal.
- **Entrada `llm:` com `base_url` própria e sem `api_key` deixa de receber a
  variável de ambiente do tipo**, inclusive pelo `agent.default_provider`. O
  endpoint OpenAI-compatível recebe o marcador `not-needed`, e
  `anthropic`/`openrouter` recusam com erro. Ponha o `api_key` na própria
  entrada. O `https://openrouter.ai/api/v1` que o `garraia init` grava continua
  funcionando com a chave no ambiente. O `--url` avulso só usa `LLM_API_KEY` ou
  a chave da entrada com a mesma `base_url`.
- **Limites documentados nos fragmentos desta versão:**
  - o `garraia.log` agora cresce entre starts, sem rotação, como já crescia
    no `garraia start` em primeiro plano;
  - `runs.retention_days` fica em `0` (nunca apaga) até você mudar; enquanto
    isso o boot diz quantos runs existem;
  - um `mcp.json` já gravado não é reescrito: siga o `mcp.filesystem_pinned`
    para fixar a versão, que vale só para o pacote de topo (as dependências dele
    seguem os ranges semver);
  - a sessão REST volta do disco sem o `working_dir`;
  - quem pareou por código `/pair` e nunca esteve no `allow` segue admitido até
    o restart;
  - `gateway.host`/`gateway.port` continuam no schema, e o `config check` os
    aponta como deprecados quando diferem do bind efetivo;
  - o Web Console mostra o bind como somente-leitura;
  - pedidos de confirmação pendentes somem num restart do gateway;
  - o aviso de loop custa no máximo uma volta extra de LLM, e não há chave para
    desligá-lo;
  - a action `swagger-ui-cache` do CI fica uma release como vestígio.

## Limites conhecidos

- **A `temperature` de um modo ainda não chega ao provider.** O prompt e o
  `max_tokens` do modo chegam em todo turno. Já os turnos de chat, canais e API
  vão sem temperatura, e o provider usa o default dele. Mandá-la mudaria o
  pedido de todo turno com modo (os embutidos declaram de 0.3 a 0.7), e isso
  fica para quando o provider souber omitir o parâmetro nos modelos que o
  recusam. Ver
  [`docs/src/modes.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/modes.md).
- **Sessões REST encerradas antes da 0.4.5 voltam a ser lidas.** A marca
  `api_logout` só existe para `DELETE` feito a partir desta versão. Uma sessão
  REST encerrada numa versão anterior ficou sem ela e, depois da atualização,
  `GET /api/sessions/{id}/history` volta a servi-la como sessão viva. A rota é
  do próprio operador (loopback, ou o gate de `api_key` num bind exposto) e o
  histórico já está no `sessions.db` dele. Para encerrar de vez, faça um novo
  `DELETE`, que agora grava a marca.
- **O `bash` no sandbox ainda é uma linha de shell.** `run_tests`, `git_diff`,
  `code_review` e `repo_search` rodam o container por argv, sem shell (#1225).
  O `bash` ainda monta o `docker run`/`podman run` como uma linha interpretada
  por shell no host, com o comando entre aspas simples (`sh_quote`). A conversão
  estrutural do `bash` para argv não está nesta release. As flags de contenção
  são as mesmas nos dois caminhos (`--cap-drop ALL`, `--pids-limit 512`,
  `--user`/`--userns=keep-id`, `no-new-privileges`), e nenhum dos dois tem
  `--read-only` nem limite de memória
  ([threat model](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md),
  prioridade 9). Em `standard` sem sandbox o `bash` nem é registrado, então
  este limite só vale com sandbox `docker`/`podman` configurado ou em
  `isolated-pod`.
- **O gateway ainda resolve a chave de provider por config > env, sem olhar a
  `base_url`.** A regra da #1370 vale na CLI (`ask`, `chat`, `max-power`,
  `garra_ask`/`garra_agent`). No gateway, uma entrada sem chave ainda pode
  receber a variável de ambiente do tipo. Ver
  [`docs/configuration.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/configuration.md).
- **Contato WhatsApp identificado só por `@lid`, sem número, não casa com um
  número do `allow`.** Ele é recusado em silêncio, e o `status` conta essas
  recusas. Para autorizá-lo, use um código `/pair` ou
  `garraia whatsapp allow <id>@lid`.
- **A recusa do "sim" de outro membro não tem teste ponta a ponta em cada um dos
  11 canais de `bootstrap/`.** Há um e2e do caminho comum e um guarda estático
  que prende, por arquivo, qual identificador vai como remetente. O guarda
  prova o identificador, mas não que a plataforma o entrega autenticado. No
  IRC, o nick é a única identidade e pode ser tomado sem NickServ.
