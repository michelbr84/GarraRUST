# 24. Perfis de execução: `standard` e `isolated-pod`

- **Status:** Accepted (2026-09-21, decisão de produto do dono na #1329 — R5
  assinada no comentário de 2026-09-21; implementação pelo coordenador autônomo)
- **Deciders:** @michelbr84 (decisão de produto e de segurança) + coordenador
  autônomo (desenho, implementação e revisão com `security-auditor`)
- **Date:** 2026-09-21 (America/New_York)
- **Tags:** seguranca, toolgate, whatsapp, mcp, config, deploy, fase-4
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: #1329 (raiz `$HOME` do MCP `filesystem` autoprovisionado contorna o
    jail da #1244) — decisão registrada no comentário de 2026-09-21
  - #1327 / #1330 (a recusa `FerramentaMcpRegistrada` saiu; piso `search` no
    canal), #1244 (jail das file tools), #1264 e #1288 (whitelist do `ToolGate`
    fail-closed), #1225 (sandbox)
  - [ADR 0019](0019-process-hardening-and-sandbox.md) — confinamento das tools
  - [ADR 0023](0023-whatsapp-dispositivo-vinculado.md) — canal
    `whatsapp_linked`
  - `docs/security/threat-model.md` §5.15 (esta decisão), `docs/execution-profiles.md`
  - `CLAUDE.md` regras 4, 6, 8 e 14

---

## Context and Problem Statement

O GarraIA tem hoje **uma** postura de segurança, pensada para instalação direta
em máquina compartilhada: `ToolGate` fail-closed por modo (`search` como piso do
WhatsApp pessoal), jail de filesystem (`agent.file_roots` ∪ `working_dir`,
#1244), gate de comando arriscado no `bash`, sandbox opcional (#1225). Essa
postura está certa para um laptop e para um servidor com outros inquilinos.

Ela está errada para o cenário que a #1329 expôs: o operador instala o Garra
**num pod/container descartável**, exatamente para dar ao agente autonomia
plena (ler e escrever arquivos, rodar shell, instalar pacotes, usar MCP e
subagentes) sem risco para nada fora do pod. Nesse cenário:

- o piso `search` do WhatsApp nega o `filesystem__write_file` que o operador
  quer usar;
- o servidor MCP `filesystem` autoprovisionado no primeiro boot aponta para
  `$HOME` — dentro de um pod isso é o pod inteiro (aceitável), numa máquina
  compartilhada é o contorno do jail da #1244 (inaceitável) — e a mesma linha
  de código não sabe em qual dos dois está;
- não existe nenhum lugar para o operador **declarar** "este processo roda num
  pod isolado", então toda decisão de política tem de assumir o pior caso.

O dono decidiu (#1329, 2026-09-21): *full power inside the isolated pod; no
implicit access outside the pod*. Este ADR fixa como isso vira código.

## Decision Drivers

- **Explícito, nunca inferido.** Detecção de container (`/.dockerenv`,
  `/proc/1/cgroup`) não é prova de isolamento: um container com o socket do
  Docker montado ou com `--pid=host` parece igual a um pod descartável. O perfil
  só muda por escolha do operador, persistida e auditável.
- **Um conceito, não dois sistemas.** Reusar `ToolGate`/`AgentMode`,
  `piso_somente_leitura`, `FileJail`, `provision_filesystem_if_missing` e as
  superfícies `config check` / `/api/diagnostics` / `/api/settings/effective`.
- **Remetente ≠ canal.** "WhatsApp conectado" não torna todo remetente
  confiável; o perfil completo é por identidade declarada, em conversa 1:1.
- **Fail-closed em toda dúvida**: valor inválido, identidade desconhecida,
  grupo, lock envenenado — tudo cai no piso `standard` ou na recusa.
- **Zero regressão em instalação existente**: seção ausente = comportamento de
  hoje; `mcp.json` já gravado nunca é reescrito.

## Considered Options

1. **Perfil de execução explícito (`execution.profile`)** com dois valores e
   política derivada por superfície — *escolhida*.
2. Só mudar a raiz do MCP autoprovisionado (opção 1 original da #1329) e deixar
   o operador subir o `default_mode` do WhatsApp para `code` — resolve o `$HOME`
   mas continua sem um lugar para declarar o pod, e `code` para **todo**
   remetente admitido é mais largo do que a decisão pede.
3. Autodetectar container e liberar automaticamente — rejeitada pelo driver 1.
4. Não provisionar o `filesystem` (opção 3 original) — perde a conveniência
   que motivou o autoprovisionamento e não resolve o piso do canal.

## Decision Outcome

### Config e ativação

```yaml
execution:
  profile: standard        # default; ou isolated-pod
  pod_root: /workspace     # opcional; so vale em isolated-pod
```

- Nova seção `execution` em `AppConfig` (`garraia-config`), enum
  `ExecutionProfile { Standard (default), IsolatedPod }` no molde de
  `SandboxMode` (`#[serde(rename_all = "kebab-case")]`, valor `isolated-pod`).
  Seção ausente = `standard` = comportamento de hoje.
- Variável de ambiente `GARRAIA_EXECUTION_PROFILE` (`standard` |
  `isolated-pod`) **vence** o arquivo, resolvida uma vez no `ConfigLoader`,
  com a origem registrada (`default` | `file` | `env`) para as superfícies de
  observação. Valor inválido (env ou arquivo) é **erro de carga**: o gateway
  não sobe e o `config check` reporta `Error` — fail-closed, nunca "cai em
  standard em silêncio".
- `GARRAIA_EXECUTION_PROFILE` entra em `KNOWN_GARRAIA_ENV_VARS`.
- Nenhum código lê `/.dockerenv`, `/proc/*/cgroup` nem variável de ambiente
  de runtime de container para decidir o perfil. Um teste varre o módulo do
  perfil e proíbe esses literais (molde: `detect.rs::o_modulo_de_deteccao_nunca_executa_binario`).

### O que cada perfil significa

| Superfície | `standard` (hoje) | `isolated-pod` |
| --- | --- | --- |
| Piso do WhatsApp para remetente **dono** em conversa 1:1 | `channels.whatsapp_linked.default_mode` (default `search`) | `default_mode` se explícito; default **`code`** (sem whitelist: filesystem, `bash`, MCP, subagentes) |
| Piso do WhatsApp para remetente admitido que **não** é dono, ou qualquer mensagem de **grupo** | `default_mode` (default `search`) | **igual ao standard** — poder total nunca é herdado por grupo nem por contato só pareado |
| Remetente fora de `allow` e sem código de pareamento | recusado em silêncio | recusado em silêncio (inalterado) |
| Raiz do MCP `filesystem` autoprovisionado | `agent.file_roots` se houver; senão `<data_dir>/workspace` | `execution.pod_root` se houver; senão `<data_dir>/workspace` |
| Jail das file tools nativas, gate de comando arriscado do `bash`, sandbox | inalterados | inalterados (o pod é a fronteira; o gate de comando arriscado continua sendo a proteção contra `rm -rf /` **dentro** do pod e pode ser alargado por `agent.bash_allowlist`) |
| Boot | `info!` com o perfil e a origem | `warn!` único: poder total dentro do pod, fronteira = pod, como reverter |

`$HOME` **nunca** é raiz implícita em nenhum perfil. Dentro de um pod dedicado
a fronteira default é o workspace do Garra (`<data_dir>/workspace`); um caminho
pod-local mais amplo (`/workspace`, `/`) é escolha explícita em
`execution.pod_root`. A raiz efetiva é logada no provisionamento e aparece no
diagnóstico.

### Dono no WhatsApp (`whatsapp_linked`)

- Nova chave `channels.whatsapp_linked.owners: [<identidade>]` — mesma
  normalização de `allow` (`normalizar_identidade`: dígitos, ou JID `@lid`).
  Quem está em `owners` é admitido como se estivesse em `allow`.
- Pareamento por código de 6 dígitos **nunca** confere o perfil completo: é
  credencial fraca (memória do processo, um código). Só identidade declarada
  na config é dono.
- Mensagem `from_me` continua fora (`deve_responder`); "note to self" não é
  caminho suportado nesta fatia — documentado.
- O perfil efetivo do turno é uma função pura
  `perfil_do_turno(perfil_de_execucao, settings, remetente, is_group) ->
  PerfilDoTurno { Completo | Padrao }`, avaliada **depois** de `admitir` e
  **antes** de montar o `ExecContext`. Ele alimenta `piso_somente_leitura`, que
  continua respeitando um `/mode` explícito da sessão.
- Cada turno loga `phone_last4` + `perfil` (`completo` | `padrao`) + o modo do
  piso. Nunca JID, telefone, `push_name` nem texto (a varredura
  `fonte_nao_loga_jid_cru_nem_material_de_sessao` continua valendo).
- `owners` preenchido com perfil `standard` gera **Warning** no `config check`
  ("só tem efeito em isolated-pod"), nunca poder.

### Observabilidade

- `/api/diagnostics`: check `execution.profile` — `Ok` em `standard`; em
  `isolated-pod` **`Warning`** com origem, piso do dono, número de donos, raiz
  do MCP e `next_step` ("confirme que este processo roda num pod isolado;
  para reverter: `execution.profile = standard`"). Check `mcp.filesystem_root`
  — em `standard`, `Warning` quando o `filesystem` persistido aponta para fora
  do jail (o `$HOME` gravado por instalações anteriores à v0.4.4), com o
  passo para corrigir; `Ok` em `isolated-pod`.
- `/api/settings/effective`: linha read-only `security.execution_profile`
  (valor + origem), no molde de `security.sandbox_mode`.
- `garraia whatsapp status`: linha com o perfil, o piso do dono e a contagem de
  donos.
- `config check`: perfil e origem no sumário; `Error` em valor inválido;
  `Warning` para `owners` fora de `isolated-pod`.

### CLI

- A instrução pós-link e as demais instruções do `garraia whatsapp` passam a
  usar o nome do executável em execução (`garraia` ou `garra`, derivado de
  `current_exe()`, fallback `garraia`) — nunca um binário que pode não existir.

## Consequences

**Boas:** o operador de pod declara uma vez e ganha o agente com autonomia
plena, com aviso claro e diagnóstico que diz exatamente o que foi liberado; a
instalação padrão fica **mais** segura do que hoje (o `filesystem` nasce no
workspace, não em `$HOME`); nenhuma superfície nova de política — só o
`ToolGate` de sempre com outro piso.

**Ruins / custo:** mais uma chave para documentar em dois idiomas; instalação
`standard` anterior à v0.4.4 mantém o `$HOME` no `mcp.json` (nunca reescrito) e
só recebe um aviso no diagnóstico com o passo para corrigir; o dono precisa
listar a própria identidade em `owners` (sem isso, `isolated-pod` não muda
nada no WhatsApp — e o diagnóstico diz "0 donos").

## Riscos e mitigações

- **Operador liga `isolated-pod` fora de um pod.** Mitigação: aviso no boot,
  `Warning` permanente no diagnóstico, nome do perfil autoexplicativo, docs
  com a lista do que NÃO é isolado por padrão (socket Docker, `--pid=host`,
  mounts, segredos do host).
- **Identidade forjada.** O remetente vem do JID que o Baileys autentica;
  `@lid` sem número entra como JID cru; comparação byte a byte após
  normalização. Grupo nunca herda.
- **Comando destrutivo dentro do pod.** O gate de comando arriscado do `bash`
  continua ligado em ambos os perfis; sem canal de confirmação ele é
  fail-closed. Isso é deliberado: o perfil libera *ferramentas*, não desliga
  *proteções*.

## Amendment 2026-09-21 — `bash` sem humano no laço (#1272)

A linha "Jail das file tools nativas, gate de comando arriscado do `bash`,
sandbox: inalterados" da tabela "O que cada perfil significa" valia para o
jail e o gate, mas **não** para a existência do `bash`: em `standard` o
`garraia mcp-server` e o gateway registravam um `bash` irrestrito, e o gate
só pega comando que parece perigoso (`cat /etc/shadow` e `echo x > /fora`
passavam). Decisão do dono, aplicada às superfícies sem humano no laço
(`garraia mcp-server` e o runtime do gateway):

| | `standard` | `isolated-pod` |
|---|---|---|
| `bash` registrado | só com `agent.sandbox` `docker`/`podman` válido e o binário presente; todo comando via `wrap_command`, sem fallback para o host | sim, no host do pod (ou no sandbox, se configurado) |
| `bash` com sandbox desligado, `ssh`, `elevated`, `allowlist` sem `bash`, sem backend ou binário ausente | **não registrado** (warn! no boot, check `tools.bash`, system prompt diz que não há shell) | registrado no host do pod |
| Jail das file tools, denylist e tier arriscado | inalterados | inalterados |

- A decisão é a função pura `garraia_gateway::bootstrap::exposicao_do_bash`
  (perfil + policy + disponibilidade do binário), com a mesma regra de
  "nunca inferir de container" deste ADR (teste varre o fonte).
- Nenhuma lista negra textual de comando é a fronteira: a contenção é o
  isolamento de processo. O container ganha `--cap-drop ALL`,
  `--pids-limit`, `--user <uid>:<gid>` (`--userns=keep-id` no podman) e
  monta só o diretório de trabalho canônico.
- Como o `bash` sandboxado escreve no diretório montado, as tools de git
  (`git_diff`, `code_review`) rodam endurecidas (fsmonitor, textconv,
  filtros, bare implícito, hooks) e `file_write` recusa caminho com `.git`.
- `garraia chat` fica como está: o humano no terminal confirma e é o
  principal.

## Testes de regressão exigidos pela decisão

1. instalação padrão segura (perfil `standard`, piso `search`, MCP write
   negado pelo runtime com a fiação real);
2. `isolated-pod` explícito por arquivo e por env (env vence; inválido = erro);
3. canal sobe com servidor MCP registrado, nos dois perfis;
4. dono em `isolated-pod` invoca `filesystem__write_file` com a fiação real
   (ponte falsa + provider stub que pede a tool) e a tool executa;
5. remetente desconhecido não recebe resposta; admitido-não-dono e dono em
   grupo caem no piso `standard` e a tool é negada;
6. nenhuma liberação por detecção de container (varredura de fonte + teste de
   resolução sem config/env = `standard`);
7. o literal `FerramentaMcpRegistrada` não existe mais no fonte do canal;
8. instrução pós-link = `garraia start` quando o executável se chama
   `garraia` (e `garra start` quando `garra`; fallback `garraia`).

Cada teste positivo tem o gêmeo negativo (tirar o dono de `owners`, voltar o
perfil para `standard`, mandar do grupo) para que remover a autorização ou a
permissão efetiva **quebre** a suíte.
