# Novidades da v0.4.4

> 🇬🇧 [English version](Whats-New-v0.4.4) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.4)

A release que faz o WhatsApp pessoal funcionar numa instalação **nova** e dá a
quem roda o Garra num pod um jeito explícito de entregar poder total ao agente.
Na v0.4.3, "instalar + `garra whatsapp link`" nunca chegava a responder: o canal
se recusava a subir porque toda instalação nova ganha um servidor MCP. Isso
acabou. E, para quem quer o agente com autonomia plena dentro de um container
descartável, chegou o perfil de execução `isolated-pod` — com a regra
**poder total dentro do pod isolado; nenhum acesso implícito fora do pod**.

---

## WhatsApp pessoal numa instalação nova

- **O canal sobe com MCP registrado** (#1327). A recusa
  `FerramentaMcpRegistrada` compensava uma brecha do `ToolGate` fechada na
  #1288 e tinha ficado órfã; como o servidor `filesystem` é provisionado no
  primeiro boot, ela desligava o canal em toda instalação padrão. O piso
  `search` continua negando ferramenta MCP por nome a cada turno.
- **`garra` e `garraia` existem os dois** (#1328). O `install.sh` cria `garra`
  como symlink relativo para `garraia`, o `install.ps1` grava o shim
  `garra.cmd`, e os pacotes `.deb`/`.rpm` trazem `/usr/bin/garra`. Um `garra`
  que não seja do instalador é preservado com aviso.
- **Toda instrução nomeia o executável que está rodando** (#1329). A mensagem
  pós-link diz `garraia start` quando você rodou `garraia` (e `garra start`
  quando rodou `garra`); o mesmo vale para o `garraia whatsapp status`, o
  próximo passo do `/api/diagnostics`, o log do gateway quando o canal não sobe
  e as falhas do `whatsapp link`.
- **O boot parou de avisar `unknown channel type: whatsapp_linked`**, e todo
  motivo de o canal não subir sai em `WARN` com a ação (`rode garraia whatsapp
  link`, `instale Node.js 20+`, `use search ou remova a chave`).

## Perfis de execução: `standard` e `isolated-pod` (ADR 0024)

| | `standard` (default) | `isolated-pod` |
| --- | --- | --- |
| Como liga | nada a fazer | `execution.profile: isolated-pod` ou `GARRAIA_EXECUTION_PROFILE=isolated-pod` (a env vence) |
| Dono do WhatsApp em conversa 1:1 | piso `search` | piso `code`: `bash`, `file_write`, subagentes e toda ferramenta MCP |
| Contato só pareado, admitido que não é dono, qualquer grupo | piso `search` | piso `search` — poder total nunca é herdado |
| Raiz do MCP `filesystem` autoprovisionado | `agent.file_roots`, senão `<data_dir>/workspace` | `execution.pod_root`, senão `<data_dir>/workspace` |
| Gate de comando arriscado, jail das file tools, sandbox | ligados | ligados |

```yaml
execution:
  profile: isolated-pod
  pod_root: /workspace          # opcional; caminho absoluto dentro do pod
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    owners: ["5511999998888"]   # só dono declarado, só em conversa 1:1
```

- **Explícito, nunca inferido.** Nenhum código lê `/.dockerenv` ou cgroups para
  decidir o perfil (dois testes varrem o fonte). Valor inválido recusa o boot e
  sai como `Error` no `garraia config check` (exit 2), no arquivo e na env.
- **Auditável.** No boot, um `WARN` único diz o que o perfil libera, o que ele
  **não** isola (filesystem do host, socket Docker/Podman, namespaces do host,
  mounts não declarados, segredos do host) e como reverter. O perfil e a origem
  (`default`/`file`/`env`) aparecem no `config check`, no check
  `execution.profile` do `/api/diagnostics`, na linha
  `security.execution_profile` do `/api/settings/effective` e no
  `garraia whatsapp status`. Cada turno do WhatsApp registra o perfil
  aplicado (`completo`/`padrao`) e o modo, identificando o remetente só pelos 4
  últimos dígitos.
- **O dono é identidade declarada.** Só quem está em `owners` recebe o perfil
  completo, e só em 1:1. Pareamento por código admite, mas nunca faz dono.
  Use dígitos (`5511999998888`) ou o JID `@lid`; o `config check` avisa quando
  uma entrada `…@s.whatsapp.net` nunca vai casar.

## MCP `filesystem` sem `$HOME`

O servidor provisionado no primeiro boot deixa de apontar para a sua home nos
**dois** perfis. Só o `<data_dir>/workspace` default é criado; uma raiz
declarada que não existe faz o provisionamento não acontecer (com aviso), nunca
cair numa raiz mais larga. Um `mcp.json` já gravado **nunca é reescrito** — se
a sua instalação é anterior à v0.4.4, o check `mcp.filesystem_root` do
`/api/diagnostics` avisa em `standard` e diz como corrigir.

## `tool_program`: vários passos num turno, com gate por passo

O modelo pode mandar um programa de até 16 passos (`tool_program`) e o runtime
executa sem voltar ao LLM entre eles (#1226 S-B). Cada passo passa pelo mesmo
despacho do loop normal: mesmo `ToolGate`, mesmo orçamento, mesma pausa de
confirmação. Os modos `auto`, `code` e `ask` expõem o `tool_program`; os modos
com whitelist (`search`, `architect`, `debug`, `orchestrator`, `review`,
`edit`) não. A revisão pós-merge fechou onze achados antes da release: variável
`$nome` não resolvida falha o passo, a pausa devolve ao modelo o que os passos
anteriores já fizeram, e um programa de um passo não escapa mais do detector de
loop.

## Outras correções

- **`config check` e o bind** (#1261): o check julga o bind pelo que o
  `garraia start` usa (env `HOST`/`PORT`, senão `127.0.0.1:3888`) e aponta
  `gateway.host`/`gateway.port` do arquivo como chaves que o start não lê. As
  decisões de comportamento (recusar bind exposto sem credencial, destino das
  chaves) seguem com o dono na #1261.
- **Aviso de atualização** (#1320): só anuncia release **maior** que o binário.
- **Imagem Docker** volta a construir com o bridge do WhatsApp, e o AppImage
  aarch64 da release volta a ser empacotado.
- **CI**: o Security Gate deixou de cair no timeout depois de os testes
  passarem (#1332).

## Atualizando

```bash
garraia update
```

- Nada muda sem configuração: sem a seção `execution`, o perfil é `standard`.
- Para ligar o perfil num pod, veja [`docs/execution-profiles.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/execution-profiles.md).
- Instalações anteriores com o `filesystem` em `$HOME`: edite o último
  argumento do `filesystem` em `<config_dir>/mcp.json` para um diretório dentro
  das raízes declaradas e reinicie.
- `garraia config check --strict` pode passar a sair 2 em configs geradas pelo
  wizard com `host: 0.0.0.0` (chave que o `start` não lê); veja a #1261.

### Limitações conhecidas

- `isolated-pod` confia na fronteira do pod que **você** montou: se o
  container tem o socket do Docker, `--privileged` ou `--pid=host`, o agente
  também tem.
- O perfil muda só o piso do WhatsApp pessoal; os outros canais seguem como
  antes.
- O jail das file tools nativas continua valendo em `isolated-pod`: declare a
  raiz do pod também em `agent.file_roots` se quiser `file_write` fora do
  workspace.
