# Perfis de execucao: `standard` e `isolated-pod`

> Decisao: [ADR 0024](adr/0024-perfis-de-execucao-isolated-pod.md) (#1329,
> aceita 2026-09-21). Threat model: [`security/threat-model.md` §5.15](security/threat-model.md#515-perfil-isolated-pod--poder-total-dentro-do-pod-nada-implícito-fora-1329).
> Regra em uma frase: **poder total dentro do pod isolado; nenhum acesso
> implicito fora do pod.**

O GarraIA tem uma unica postura de seguranca de fabrica, pensada para rodar
direto numa maquina compartilhada — laptop, servidor com outros usuarios,
Termux. Ela e o perfil **`standard`**. O perfil **`isolated-pod`** existe para
o outro cenario: o operador instala o Garra num pod ou container
**descartavel**, justamente para dar ao agente autonomia plena (ler e
escrever arquivos, rodar shell, instalar pacotes, usar servidores MCP e
subagentes) sem risco para nada fora do pod. Nesse caso o pod e a fronteira
de seguranca, nao o Garra.

O perfil so muda por **escolha explicita** do operador. Nenhuma linha do
codigo le `/.dockerenv`, `/proc/*/cgroup` nem variavel de runtime de
container para decidir: um container com o socket do Docker montado ou com
`--pid=host` e indistinguivel de um pod descartavel visto de dentro, e
"parece isolado" nao e isolamento. Dois testes varrem o fonte e reprovam
esses literais.

## O que cada perfil significa

| Superficie | `standard` (default) | `isolated-pod` |
|---|---|---|
| Piso do WhatsApp pessoal para remetente **dono** em conversa 1:1 | `channels.whatsapp_linked.default_mode` (default `search`) | `default_mode` se explicito; senao **`code`** (sem whitelist: filesystem, `bash`, MCP, subagentes) |
| Piso do WhatsApp para remetente admitido que **nao** e dono, ou qualquer mensagem de **grupo** | `default_mode` (default `search`) | **igual ao `standard`** — poder total nunca e herdado por grupo nem por contato so pareado |
| Remetente fora de `allow`/`owners` e sem codigo de pareamento | recusado em silencio | recusado em silencio (inalterado) |
| Raiz do MCP `filesystem` autoprovisionado | `agent.file_roots` se houver; senao `<data_dir>/workspace` | `execution.pod_root` se houver; senao `<data_dir>/workspace` |
| Jail das file tools nativas (`agent.file_roots`), gate de comando arriscado do `bash`, `agent.sandbox` | inalterados | inalterados — o perfil libera *ferramentas*, nao desliga *protecoes* |
| Log de boot | `INFO` com o perfil e a origem | um `WARN` unico: o que foi liberado, o que o perfil NAO isola, como reverter |

`$HOME` **nunca** e raiz implicita em nenhum perfil. Dentro de um pod
dedicado a fronteira default e o workspace do Garra (`<data_dir>/workspace`,
que por default e `<config_dir>/data/workspace`); um caminho pod-local mais
amplo (`/workspace`, `/`) e escolha explicita em `execution.pod_root`.

## Como ligar

```yaml
# config.yml
execution:
  profile: isolated-pod    # standard (default) | isolated-pod
  pod_root: /workspace     # opcional; so vale em isolated-pod
```

ou, sem tocar no arquivo — o caminho natural num manifest de pod:

```bash
GARRAIA_EXECUTION_PROFILE=isolated-pod garraia start
```

Regras de resolucao:

- **A env vence o arquivo.** `GARRAIA_EXECUTION_PROFILE` e lida uma vez pelo
  `ConfigLoader`, e a origem fica registrada (`default` | `file` | `env`) para
  as superficies de observacao dizerem "isolated-pod (fonte: env)" em vez de
  so o valor.
- **Secao ausente = `standard`** = o comportamento de hoje. Instalacao
  existente nao muda nada ao atualizar.
- **Valor invalido e erro de carga**, no arquivo ou na env: o gateway **nao
  sobe**, e `garra config check` reporta `Error` em `execution.profile`
  (exit 2). Nunca "cai em `standard` em silencio" — um typo no manifest do
  pod tem de aparecer, nao ser mascarado.
- **A env nunca vai para o disco.** `garra config set`, `garra whatsapp link`
  e outros caminhos load-modify-save rodam com a env presente; ela nao e
  promovida a `execution.profile` por um `save`. Tirar a env devolve o
  processo a `standard` no proximo start.
- `execution.pod_root` precisa ser **absoluto**; relativo resolveria contra o
  diretorio de quem iniciou o processo (o `config check` avisa). Declarado em
  `standard`, e ignorado com `Warning`.

Para reverter: `execution.profile: standard` no `config.yml` (ou remova a
secao) e remova a env `GARRAIA_EXECUTION_PROFILE`; reinicie o gateway.

## O que "poder total dentro do pod" libera

Para o **dono** do WhatsApp pessoal (ver abaixo) em conversa 1:1 — e so
para ele, numa sessao que nao escolheu `/mode` — o piso passa de `search` para
`code`. Todo o resto continua no piso de `standard`: remetente admitido que nao
e dono, contato so pareado, qualquer mensagem de grupo, e os outros canais (o
perfil nao muda o piso deles). No turno do dono:
o `ToolGate` do modo `code` nao tem whitelist, entao passa `file_read`,
`file_write`, `bash`, `run_tests`, toda ferramenta de servidor MCP registrado
(`filesystem__write_file` inclusa) e subagentes. E o mesmo `ToolGate` de
sempre, com outro piso — nenhuma superficie nova de politica.

O que **continua ligado** em `isolated-pod`, por desenho:

- O **jail das file tools nativas** (`file_read`, `file_write`, `list_dir`):
  `agent.file_roots` ∪ `working_dir` da sessao, exatamente como em `standard`
  (#1244). `execution.pod_root` muda so a raiz do MCP `filesystem`. Se quiser
  que as tools nativas alcancem o pod inteiro, declare isso tambem:
  `agent.file_roots: ["/workspace"]`.
- O **gate de comando arriscado** do `bash` (`rm -rf /`, `git reset --hard`,
  …). Sem canal de confirmacao ele e fail-closed; alargue com
  `agent.bash_allowlist` se o pod for descartavel de verdade.
- `agent.sandbox`, se configurado.
- Um `/mode` explicito da sessao continua vencendo o piso, para cima ou para
  baixo.

## Bash nas superficies sem humano (#1272)

O `garraia mcp-server` (tool `garra_agent`) e o runtime do gateway nao tem
humano para confirmar comando. Nelas o `bash`:

- em `standard`, so existe dentro de um sandbox `docker`/`podman` valido
  (`agent.sandbox.mode: all` ou `allowlist` com `bash`, binario instalado).
  Sem isso ele **nao e registrado** — `ssh` nao conta, porque e execucao
  remota, nao isolamento;
- em `isolated-pod` explicito, roda no host do pod (ou no sandbox, se houver
  um), com a denylist e o tier arriscado ligados.

O boot avisa uma vez quando o `bash` fica de fora, o `/api/diagnostics`
mostra o check `tools.bash` com o passo, e o modelo e avisado no system prompt
de que nao ha shell. `garraia chat` nao muda: la o humano confirma no
terminal.

## O que o perfil NAO isola

O perfil e uma **declaracao** do operador; ele nao cria a fronteira. Tudo
abaixo fica ao alcance do agente se o pod expuser:

- filesystem do host montado como volume;
- o socket do Docker/Podman (`/var/run/docker.sock`) — e o host inteiro;
- container `--privileged`;
- `--pid=host` (processos do host) e `--network=host` (servicos do host em
  loopback, metadata endpoints de nuvem);
- mounts nao declarados, bind mounts de `$HOME`, tmpfs compartilhados;
- segredos do host injetados por env (chaves de provider, tokens de deploy,
  credenciais de nuvem) — o agente pode le-los pelo `bash`.

Se qualquer item da lista vale para o seu container, ele **nao e um pod
isolado** e `isolated-pod` e a escolha errada. O aviso de boot e o `Warning`
permanente em `/api/diagnostics` existem para lembrar disso.

## Dono no WhatsApp pessoal (`channels.whatsapp_linked.owners`)

"WhatsApp conectado" nao torna todo remetente confiavel. O perfil completo e
por **identidade declarada**, em conversa **1:1**:

```yaml
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    owners: ["5511999998888"]   # mesma normalizacao de `allow`: digitos, ou JID @lid
    # default_mode: search      # se explicito, vale para o dono tambem
```

- Quem esta em `owners` e admitido como se estivesse em `allow`, e em
  `isolated-pod` recebe o piso `code` (ou o `default_mode` explicito).
- **Pareamento por codigo de 6 digitos nunca confere o perfil completo.** E
  credencial fraca (memoria do processo, um codigo); so identidade declarada
  na config e dono.
- **Grupo nunca herda.** O dono mandando de um grupo cai no piso `standard`
  (`default_mode`), como qualquer admitido.
- Mensagem `from_me` continua fora (`deve_responder`): "note to self" nao e
  caminho suportado nesta fatia.
- `owners` preenchido com perfil `standard` gera **Warning** no `config
  check` ("owners so tem efeito em isolated-pod"), nunca poder.
- Sem `owners`, `isolated-pod` **nao muda nada no WhatsApp** — o diagnostico
  diz "0 donos".

Cada turno loga `phone_last4` + `perfil` (`completo` | `padrao`) + o modo do
piso. Nunca JID, telefone, `push_name` nem texto (a varredura de fonte
`fonte_nao_loga_jid_cru_nem_material_de_sessao` continua valendo).

## Raiz do MCP `filesystem` autoprovisionado

No primeiro boot o gateway grava um `mcp.json` com o servidor `filesystem`
(`@modelcontextprotocol/server-filesystem`). Ate a v0.4.3 a raiz era `$HOME` —
dentro de um pod, o pod inteiro (aceitavel); numa maquina compartilhada, o
contorno do jail da #1244 (inaceitavel), e a mesma linha de codigo nao sabia
em qual dos dois estava. A partir da v0.4.4:

| Perfil | Raiz |
|---|---|
| `standard` | `agent.file_roots` da config, ou `<data_dir>/workspace` se vazio. A env `GARRAIA_FILE_ROOTS` e o `working_dir` da sessao, que o jail nativo tambem soma, **nao** entram aqui |
| `isolated-pod` | `execution.pod_root`, ou `<data_dir>/workspace` se ausente |

A raiz efetiva e logada no provisionamento e aparece no check
`mcp.filesystem_root` do `/api/diagnostics`. No primeiro boot so o
`<data_dir>/workspace` default e criado; uma raiz **declarada**
(`agent.file_roots`, `execution.pod_root`) tem de existir — se nao existe, o
autoprovisionamento nao acontece (um `warn!` diz qual raiz faltou) e nada e
criado em lugar dela, nunca um diretorio mais largo.

**Instalacao anterior a v0.4.4.** O `mcp.json` ja gravado **nunca e
reescrito** (a unica porta do autoprovisionamento e "arquivo ausente"), entao
o `$HOME` fica la. Em `standard`, o diagnostico avisa (`mcp.filesystem_root`
= `Warning`: o `filesystem` persistido aponta para fora das raizes declaradas) com o passo
para corrigir. Para corrigir: edite `<config_dir>/mcp.json` e troque o ultimo
argumento do `filesystem` por um diretorio dentro das raizes declaradas (uma
das `agent.file_roots`, ou `<data_dir>/workspace`); reinicie o gateway. Em
`isolated-pod` o check e `Ok` — o pod e a fronteira.

`GARRAIA_DISABLE_MCP_AUTOPROVISION=1` continua desligando o provisionamento
por completo.

## Onde o perfil aparece

| Superficie | O que mostra |
|---|---|
| Log de boot | `standard`: `INFO execution profile = standard (fonte: …)`. `isolated-pod`: um `WARN` unico com origem, `pod_root`, o que foi liberado, a lista do que nao e isolado e como reverter. |
| `garra config check` | `execution profile  : isolated-pod (source: env)` no sumario; `Error` em valor invalido; `Warning` para `execution.pod_root` fora de `isolated-pod` ou relativo, e para `channels.whatsapp_linked.owners` fora de `isolated-pod` (so a contagem, nunca as identidades). |
| `GET /api/diagnostics` | Check `execution.profile`: `Ok` em `standard`; em `isolated-pod` **`Warning`** com origem, piso do dono, numero de donos, raiz do MCP e `next_step` ("confirme que este processo roda num pod isolado; para reverter: `execution.profile = standard`"). Check `mcp.filesystem_root` (le o `mcp.json` e a secao `mcp:` do `config.yml`, que vence): `Warning` em `standard` quando o `filesystem` aponta para fora das raizes declaradas (`agent.file_roots` / `<data_dir>/workspace`); `Ok` em `isolated-pod`. Caminhos dentro do data dir aparecem como `<data_dir>/…`. |
| `GET /api/settings/effective` | Linha read-only `security.execution_profile` (valor + origem), no molde de `security.sandbox_mode`. |
| `garra whatsapp status` | Linha com o perfil, o piso do dono e a contagem de donos. |
| Log por turno do WhatsApp | `phone_last4` + `perfil` (`completo` \| `padrao`) + modo do piso. |

## Exemplo: pod RunPod / Docker descartavel

Um pod com o Garra e nada mais, sem volume do host, sem socket do Docker:

```yaml
# ~/.config/garraia/config.yml dentro do pod
execution:
  profile: isolated-pod
  pod_root: /workspace

agent:
  file_roots: ["/workspace"]        # as tools nativas tambem enxergam o pod
  # bash_allowlist: [...]           # so se o pod for descartavel de verdade

channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    owners: ["5511999998888"]
```

Ou, deixando o arquivo em paz e declarando pelo ambiente do container:

```bash
docker run --rm \
  -e GARRAIA_EXECUTION_PROFILE=isolated-pod \
  -e OPENROUTER_API_KEY=... \
  -p 3888:3888 garraia
```

Confira depois de subir: o log de boot tem o `WARN` do perfil,
`garra config check` mostra `execution profile  : isolated-pod (source: env)`
e `curl -s localhost:3888/api/diagnostics` traz `execution.profile` como
`Warning` com o numero de donos esperado.

## Troubleshooting

| Sintoma | Causa provavel / o que fazer |
|---|---|
| `isolated-pod` ativo, mas o WhatsApp continua em `search` | (a) `owners` vazio — o perfil completo e so para identidade declarada; o diagnostico diz "0 donos". (b) A mensagem veio de **grupo** — grupo nunca herda. (c) `default_mode` explicito na config — em `isolated-pod` ele vale para o dono tambem; remova a chave para o default `code`. (d) A sessao escolheu `/mode search` — escolha explicita vence o piso. |
| Gateway nao sobe | `GARRAIA_EXECUTION_PROFILE` fora de `standard` \| `isolated-pod` (a env ignora caixa; o log diz `perfil de execucao invalido "…"`), ou `execution.profile` no arquivo fora dessas duas grafias exatas (o arquivo diferencia caixa; o log traz o erro do parser, `unknown variant …`). Corrija ou remova; e fail-closed de proposito. `garra config check` reporta os dois casos como `Error` em `execution.profile` (exit 2). |
| `config check`: `channels.whatsapp_linked.owners lists N identities but the effective execution profile is standard` | `owners` sem `isolated-pod` nao confere poder nenhum. Ou ligue o perfil (se este processo roda num pod isolado), ou remova `owners`. |
| `config check`: `execution.pod_root (…) is set but the effective profile is standard` | `pod_root` so vale em `isolated-pod`; a raiz do MCP `filesystem` segue `agent.file_roots` / `<data_dir>/workspace`. |
| `config check`: `execution.pod_root (…) is not an absolute path` | Use um caminho absoluto pod-local (`/workspace`). |
| `/api/diagnostics`: `mcp.filesystem_root` = `Warning` em `standard` | `mcp.json` anterior a v0.4.4 com `$HOME` como raiz (ou uma entrada `filesystem` em `mcp:` do `config.yml`, que vence o `mcp.json`). Troque o ultimo argumento do `filesystem` por um diretorio dentro das raizes declaradas (`agent.file_roots` ou `<data_dir>/workspace`) e reinicie. |
| `/api/diagnostics`: `execution.profile` = `Warning` e eu nao queria `isolated-pod` | Veja a origem no proprio check: `env` → remova `GARRAIA_EXECUTION_PROFILE` do ambiente do processo (manifest do pod, unit do systemd); `file` → `execution.profile: standard`. Reinicie. |
| `file_write` do dono e negado mesmo em `isolated-pod` | Nao e o `ToolGate`: e o **jail** das file tools nativas (`agent.file_roots`), que continua valendo. Declare a raiz do pod la tambem. Para o `filesystem__write_file` (MCP) a raiz e `execution.pod_root`. |
| `bash` recusa `rm -rf …` no pod | Gate de comando arriscado, fail-closed sem canal de confirmacao. Deliberado: o perfil libera ferramentas, nao desliga protecoes. `agent.bash_allowlist` alarga. |
| A instrucao diz `garraia …` e eu uso `garra` | Toda instrucao do WhatsApp (pos-link, `whatsapp status`, `next_step` do `/api/diagnostics`, log do gateway) nomeia o executavel em execucao (`current_exe()`; so `garra` ou `garraia` sao aceitos, qualquer outro nome cai em `garraia`). Os dois nomes sao o mesmo binario (`garra` e symlink/shim). |

## Veja tambem

- [`whatsapp.md`](whatsapp.md) — o canal `whatsapp_linked`, `allow`, `owners` e o piso de ferramenta
- [`mcp.md`](mcp.md) — autoprovisionamento do `filesystem`
- [`configuration.md`](configuration.md) — a secao `execution` no exemplo completo
- [`deployment-runpod.md`](deployment-runpod.md) e [`deployment.md`](deployment.md) — como ligar num pod
- [`security/threat-model.md` §5.15](security/threat-model.md) — fronteira, premissas de confianca, o que o gate ainda impoe
