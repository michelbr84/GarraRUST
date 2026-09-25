# Novidades da v0.4.6

> 🇬🇧 [English version](Whats-New-v0.4.6) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.6)

Release de **estabilização** depois da v0.4.5: nenhuma superfície nova, e
sim o que a instalação limpa da 0.4.5 revelou. O CI volta a funcionar sem
depender de registry para o MinIO. Uma sessão do WhatsApp sem projeto ganha
um workspace padrão seguro, **isolado por sessão**, em vez de `NoRoots`. Um
`X-Session-Id` forjado deixa de alcançar a conversa de outra pessoa. O
WhatsApp pessoal ganha os comandos de administração que faltavam. E o runbook
de release ganha o gate de dogfood manual — porque a v0.4.4 e a v0.4.5 saíram
verdes no CI e quebraram no caminho real.

---

## CI sem registry para o MinIO (#1458)

Em 2026-09-24 o `quay.io` passou a exigir login para toda tag de
`minio/minio` — depois de o Docker Hub já ter removido o repositório (#1230) e
de o `dl.min.io` responder 410 para os binários. O check obrigatório
`Clippy Linting` (que roda a suíte `storage-s3` contra um MinIO real) falhava
em toda PR do repositório.

O que segue público é o **fonte**. `scripts/ci/build-minio-image.sh` compila o
`minio` a partir da tag upstream fixada (`RELEASE.2025-02-28T09-55-16Z`),
recusa se a tag não apontar mais para o commit esperado, embala o binário em
`debian:bookworm-slim` e etiqueta a imagem com o `nome:tag` que o testcontainer
procura — que só faz `pull` quando a imagem não existe localmente. O binário
fica no cache do Actions por tag; só o primeiro run depois de um bump paga o
build. O `docker-compose.minio.yml` de desenvolvimento usa o mesmo script.

## Workspace padrão por sessão (#1378, #1449)

Uma sessão do WhatsApp recém-vinculada nasce sem projeto. Com
`agent.file_roots` vazio — o default de toda instalação — o jail de arquivos
ficava **sem raiz**, e sem raiz significa negar tudo: `file_read`,
`file_write` e `list_dir` apareciam registradas e toda chamada voltava
`NoRoots`.

Agora, quando nada foi declarado, a raiz efetiva é
**`<data_dir>/workspace/<sessão>`** — um subdiretório por sessão (nome =
SHA-256 do `session_id`, nunca o id em claro), dentro do diretório que o ADR
0024 já designa como o do próprio Garra. Nunca `/`, nunca `$HOME`, nunca o
`pod_root`. A revisão de segurança independente (#1449) é o motivo do "por
sessão": a primeira versão tinha uma raiz compartilhada, e um contato do
WhatsApp poderia ler o que outro escreveu. `agent.file_roots` /
`GARRAIA_FILE_ROOTS` declarados continuam vencendo sozinhos, sem escopo por
sessão. O `/api/diagnostics` mostra a fonte em `files.workspace`.

## Um `X-Session-Id` forjado não lê a conversa de outra pessoa (#1462)

`POST /v1/chat/completions` aceitava `X-Session-Id` verbatim e
`POST /api/sessions/{id}/messages` aceitava o id no caminho, sem conferir de
quem era a sessão. Como os ids de canal são adivinháveis por construção
(`whatsapp-linked-<número>`, `telegram-<chat>`), um id forjado carregava o
resumo e até 100 turnos da conversa da vítima para dentro do request — e
gravava o turno do atacante na conversa dela.

Por id, agora só se alcança sessão das **superfícies locais do operador**
(`api`, `vscode`, `web`, `parrot`). Sessão de canal ou do app mobile responde
`404 session not found`, sem confirmar que existe, e não é tocada. A leitura
`GET /api/sessions/{id}/history`, que o Web Console usa para exportar qualquer
sessão, fica como está — é decisão de produto, registrada na issue.

## WhatsApp pessoal: administrar sem editar YAML

- `garraia whatsapp users [--json]` lista quem pode falar com o Garra, por
  papel e pelos quatro últimos dígitos (#1393).
- `garraia whatsapp remove <número>` revoga; dono exige confirmação (#1394).
- `garraia whatsapp owner|unowner <número>` promove e despromove; `owner` só em
  `isolated-pod` (#1395).
- `garraia whatsapp allow '*'` explica que "autorizar todo mundo" **não
  existe** neste canal, em vez de parecer erro de digitação (#1389).
- `garraia whatsapp link` termina mostrando o **acesso em vigor** — canal,
  contagens e identidades — para ninguém sair do wizard sem ver o portão
  (#1429, parcial).
- `garra init` oferece o WhatsApp pessoal ao lado do Telegram (#1430).

## Diagnóstico e console mais honestos

- O `/api/diagnostics` separa **"nunca configurado"** (`not_configured`,
  `disabled`) de **"quebrado"** (`error`): TLS ausente, `.env` inexistente,
  Telegram sem token ou WhatsApp sem aparelho vinculado deixam de acender
  amarelo numa instalação nova (#1437).
- O Web Console mostra o **perfil de execução** como badge fixo no cabeçalho
  (#1410).
- `garra_status` explica o que `withheld` significa: campo retido por política
  não é capacidade ausente (#1382, #1387).
- `repo_search` falha rápido sem repositório ativo, em vez de tentar de novo
  (#1380).

## Segurança

- **Servidores MCP não viram slash commands** (#1386): o registro automático
  expunha toda tool MCP como `/comando`, fora do `ToolGate` dos modos.
- O achado da #1462 acima.

## Processo e ferramentas

- **Gate de dogfood manual** antes do tag, em `docs/releasing.md` §1.5 (#1439):
  a matriz D1–D9 (instalação limpa nos dois instaladores, WhatsApp de ponta a
  ponta, restart sem QR, `update`/`rollback`, bundles do desktop, isolamento
  por sessão, diagnóstico limpo), executada contra o candidato, com data,
  sistema e executor no corpo da PR de release. Linha sem data, release não
  sai.
- O hook `pre-tool-use` dos agentes deixa de bloquear `rm -rf /tmp/…` e
  `rm -rf ./x`: o bloqueio passa a ser ancorado no **alvo** (raiz, home,
  diretório atual, pai), não no prefixo (#1453).
- `scripts/setup-toolchain.sh` fixa a toolchain Rust 1.95 localmente por
  `rustup override`, sem `rust-toolchain.toml` (que quebra o cross-compile do
  CI) (#1452).

## Atualizando da v0.4.5

```bash
garraia update
```

Nada muda de formato: `config.yml`, `session.enc` do WhatsApp, `allow`/`owners`
e o histórico continuam valendo. Duas mudanças de comportamento visíveis:

- Sessões remotas sem projeto passam a ter arquivos em
  `<data_dir>/workspace/<sessão>` — uma pasta nova, vazia, por sessão. Quem já
  declarava `agent.file_roots` não vê diferença.
- Clientes que usavam `X-Session-Id` para **continuar uma sessão de canal**
  pela API compat (por exemplo, retomar uma conversa do Telegram pelo VS Code)
  passam a receber `404`. Era exatamente o caminho que a #1462 fecha; a
  conversa continua acessível pelo próprio canal e pelo Web Console.

## Limites conhecidos

- A leitura de histórico por id (`GET /api/sessions/{id}/history`) continua
  aberta a quem alcança a porta (loopback, ou a `gateway.api_key` num bind
  exposto). Fechar isso quebra o *Export* do Web Console para sessões de canal;
  a decisão fica registrada na #1462.
- Numa instalação limpa o `/api/diagnostics` ainda acende `warning` em
  `tools.bash` (desligado por desenho no perfil `standard`) e em
  `runtime.channels` (nenhum canal) — dois avisos não acionáveis, rastreados na
  #1471.
- O QR do WhatsApp continua **no terminal**: colocá-lo no aplicativo desktop é
  o plano 0363, não esta release.
