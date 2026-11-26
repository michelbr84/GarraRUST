# Novidades da v0.5.2

> 🇬🇧 [English version](Whats-New-v0.5.2) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.2)

Release estável de 25 de novembro de 2026 — substitui a v0.5.1 como release `latest`.

A v0.5.2 é a versão estável de uma série que passou por **sete candidatos**
(rc1 a rc7, de 3 a 22 de novembro). Ela traz conversa por voz contínua no
navegador, um app Desktop que se configura sem terminal, um WhatsApp
vinculado que não perde mensagem em silêncio, modos e nível de aprovação na
CLI, e diagnóstico que diz de onde veio cada afirmação.

## Voice Chat contínuo no Web Console (#1754)

- **Uma chamada, não um botão por turno.** A página `#/voice` inicia a
  chamada uma vez, ouve sem apertar nada a cada fala e aceita ser
  interrompida: fale por cima e o GarraIA para.
- **Troca no meio da chamada.** STT, modelo de conversa, TTS, voz e idioma
  mudam sem perder a sessão.
- **Local ou OpenRouter.** Usa os servidores locais do `--with-voice` e as
  instâncias OpenRouter já configuradas, com o preço como o provedor informa.
  O navegador só escolhe IDs: URL, chave e política de roteamento ficam no
  servidor, e STT/TTS não têm fallback.
- **Base de voz com limites.** Respostas com teto de tamanho, timeouts e
  clientes que não seguem redirect; o áudio do Chatterbox só é baixado da
  origem configurada, e um timeout não gera uma segunda síntese paga.

Guia: [docs/voice.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/voice.md) · decisão: [ADR 0027](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0027-natural-voice-conversation.md).

## Desktop: quem responde, sem terminal (#1700)

- **Passo "Quem responde".** O assistente oferece o Ollama local (detectado,
  com os modelos instalados) ou o OpenRouter (chave + modelo) e testa com
  uma chamada real. A tela Pronto só diz "pronto para conversar" quando o
  modelo respondeu; senão, diz por que não.
- **Outra instalação na máquina** (#1816, #1824, #1826). Com outro Garra na
  porta 3888 ou segurando a pasta de configuração, o app avisa a versão que
  está atendendo, não sobe um segundo gateway, pede confirmação explícita
  antes de gravar na configuração do outro e guarda uma cópia do
  `config.yml` em `backups/`. Parear o WhatsApp por cima é recusado.
- **`garraia start` recusa antes do boot** porta ocupada e pasta de
  configuração já em uso (exit 69).
- **AppImage da CLI no duplo clique** (#1817) abre um terminal em vez de
  "não fazer nada", e o release passa a executar os AppImages antes de
  publicar (#1818).

## WhatsApp vinculado

- **A conversa "Você" conversa** (#1813, #1822). No modo "meu próprio
  número", a mensagem para si mesmo vira turno — sem o Garra responder a si
  próprio e sem nunca responder a terceiros no lugar do dono. O opt-in é
  explícito: `garra whatsapp access self-chat on|off` (`access.self_chat`).
  Sem a chave no config, nada muda no update.
- **Mensagem enviada durante o restart é respondida** (#1814): o que chegou
  "offline" há até 10 minutos vira turno; mais velho é descartado com motivo.
- **Nenhum descarte é mudo** (#1815): falha de decriptação, mensagem vencida
  e conteúdo sem texto aparecem no log, no Web Console e no
  `/api/diagnostics`.
- **Sem resposta em dobro** (#1822): reentrega com o mesmo id não vira um
  segundo turno, cada chat roda um turno por vez, em ordem, e o acumulado da
  reconexão vira um lote e uma resposta por chat.

## CLI: modos, aprovação e memória de ferramentas

- **`garra chat --mode` e `/mode`** (#1858) aplicam os mesmos modos do
  gateway. `plan` é apelido do `architect`: somente leitura, toda ferramenta
  que muta é negada.
- **Nível de aprovação** (`agent.approval`): `always` (padrão, como antes),
  `session` (o mesmo pedido aprovado não pergunta de novo até o fim da
  sessão) e `never-confirm`, com `--yolo` como atalho que nunca persiste. O
  bloqueio definitivo (`rm -rf /`, fork bomb, exfiltração) e o fail-closed
  sem canal de confirmação não mudam em nenhum nível, e o `never-confirm`
  deixa trilha de auditoria. Detalhes em
  [docs/hardening-gateway.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md).
- **Histórico de ferramentas entre turnos** (#1708): o turno seguinte recebe
  as chamadas e os resultados pareados por id — inclusive a chamada
  interrompida por Ctrl+C.
- **`cli_inspect`, `/status` e `/tools` leem a mesma fonte** (#1709, #1712) e
  reportam as permissões efetivas do turno e os efeitos que o executor
  registrou. Guia: [docs/cli-runtime-inspection.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-runtime-inspection.md).
- **`garra skills mine|candidates|list`** (#1901): o pipeline de skills pela
  CLI, sem gateway.
- **Janelas de timeout** (#1790, #1793): `garra ask` cortado por
  `timeouts.llm` sai com exit 124; as sondas usam `timeouts.health` e o
  `garra mcp` usa `timeouts.mcp`.

## Diagnóstico e revisão com evidência

- **`garra_diagnostics`** (#1914) diz o SHA-256 do executável, o commit e a
  tag do build, qual processo responde e quem o supervisiona, e o instante
  da observação. `garra_status` não confunde RC com release estável.
- **Revisão remota** (#1909): `repo_search` diz onde olhou quando não acha
  nada; `git_diff` e `code_review` nomeiam o intervalo exato, sem entregar
  caminho absoluto do host.
- **Desfecho por chamada** (#1915): cada chamada de ferramenta termina com
  um desfecho classificado, a recusa nomeia a camada que barrou e chamada
  cancelada não conta como falha do modelo.

## Segurança

- **Gate de comandos** (#1791, #1861): leitura do ambiente pelo
  .NET/PowerShell, download por `WebClient`/BITS e
  `[Diagnostics.Process]::Start` pedem confirmação; atribuição na frente do
  comando (`DEBUG=1 node server.js`), código pelo stdin e interpretador com
  versão no nome deixam de passar livres. E um comando que só **cita** um
  literal perigoso (`echo 'nunca rode rm -rf /'`) deixa de ser bloqueado.
- **Fronteira de autorização dos turnos** (#1985): os caminhos do runtime
  que não recebem contexto explícito passam a ser fail-closed (teto de
  leitura, sem prova de operador); o teto da superfície e o teto do
  principal se compõem, e nenhum amplia o outro; a tarefa agendada roda com
  a autoridade de quem a criou.
- **Aprovações vinculadas ao workspace** (#1912): trocar de workspace exige
  nova aprovação; um "sim" é consumido uma vez e não sobrevive ao restart.
- **Nenhum cliente HTTP do runtime segue redirect** (#1792), e credencial em
  query string é redigida.
- **HSTS** quando o transporte é https (#1895); **teto declarado no corpo
  dos webhooks** (#1987); **encerramento limpo** — servidor MCP stdio
  encerra a árvore inteira de processos e o desligamento do gateway tem teto
  (#1913).
- **Dependências:** `rmcp` 3.9.0, wasmtime 50.0.3, SQLite 3.54,
  `jsonwebtoken` 12, `rustix`, `quinn`, `url` 2.5.9 e `cookie` 0.18.3,
  fechando advisories RUSTSEC.

## Atenção ao atualizar (#1985)

- **A2A sem `gateway.api_key`:** as tarefas de `/a2a/*` passam a rodar com
  teto de leitura (sem `bash`, escrita ou envio de mensagem), porque nesse
  caso a rota não exige credencial. Com a chave configurada, nada muda.
- **Tarefas agendadas antigas de sessão remota** passam a rodar só com
  leitura e avisam isso na primeira execução. Para devolver o acesso:
  `garra tasks list` mostra as tarefas; `garra tasks authorize <id>` exibe
  o texto que o agente vai receber e, com `--yes`, grava a autorização
  (`garra tasks revoke <id>` desfaz). Tarefa de sessão do operador não
  muda.

## Instalar ou atualizar

```bash
# Linux/macOS
curl -fsSL https://garraia.org/install.sh | sh
```

```powershell
# Windows
irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
```

Já tem a v0.5.1? `garra update`. Os instaladores agora concluem o upgrade
com o gateway rodando (#1862, #1865) e imprimem o comando de restart.

Imagem Docker: `ghcr.io/michelbr84/garraia:0.5.2`.
