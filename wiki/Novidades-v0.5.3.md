# Novidades da v0.5.3

> 🇬🇧 [English version](Whats-New-v0.5.3) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.3)

Release estável de 27 de novembro de 2026 — substitui a v0.5.2 como release `latest`.

A v0.5.3 é uma versão de correções, quase todas de segurança. Ela corrige o
aviso [GHSA-g6c8-mx6x-5g92](https://github.com/michelbr84/GarraRUST/security/advisories/GHSA-g6c8-mx6x-5g92),
que afeta da v0.4.1 à v0.5.2. **Quem tem canal IRC configurado deve
atualizar** e ler antes a seção **Atenção ao atualizar**, mais abaixo.

## IRC

- **Allowlist (GHSA-g6c8-mx6x-5g92, #2015).** A allowlist é uma só para todos
  os canais, e os ids dos outros canais ficam nela sem prefixo. Um nick igual
  ao id de alguém admitido por outro canal passava como aquela pessoa,
  inclusive como o dono: o id do Slack (`U01ABCDEF`) é um nick válido. Agora
  o IRC só aceita entradas `irc:<nick>`, e o nick é comparado pelo
  `CASEMAPPING` do servidor: `Ana` admitida volta como `ana` e, sob
  `rfc1459`, `[bia]` volta como `{bia}`.
- **Uma conversa privada por pessoa (#2011).** As conversas privadas de todo
  mundo caíam juntas na sessão do nick do bot, e o modelo via as mensagens de
  uma pessoa no turno de outra. Agora cada pessoa tem a própria sessão, em
  cada conexão IRC da config.
- **Resposta do modelo não vira comando (#2007).** Um `\r` no texto do modelo,
  que uma injeção de prompt pode influenciar, chegava cru ao socket e virava
  comando IRC (`QUIT`, `JOIN`). Agora toda escrita de texto do modelo passa
  por um ponto só, que quebra as linhas e respeita os 512 bytes do protocolo.

Detalhes: [docs/hardening-gateway.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md).

## Console do admin fail-closed (#1992, #2000, #2013)

- **Chave mestra insegura ou errada.** Isso acontece com a linha de KDF
  ilegível ou apagada, com a frase-senha trocada ou com o `master.key`
  trocado. Nesses casos, o `/admin` responde 503
  `{"error":"admin_unavailable"}` e o log ganha uma linha `ERROR` com o
  motivo. Antes, o boot derivava uma chave do salt legado ou cifrava segredos
  sob a chave errada. Chat, canais e `/api/*` seguem no ar.
- **`admin.db` que existe e não abre recusa o boot.** Antes, ele virava um
  console vazio em memória, e o primeiro a chegar em `/admin` criava o admin.
- **`master.key` novo nasce com modo 0600** desde o primeiro byte.

Guia: [docs/security.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security.md), seção "Admin console answering 503".

## Tarefas agendadas respondem em mais canais (#2002)

A resposta de uma tarefa agendada chega ao Discord, ao Slack, ao Matrix e ao
WhatsApp vinculado, além do Telegram. Os demais canais ficam sem entrega; no
IRC e no iMessage, de propósito. O `garraia runs list`, o `--json` e o
`GET /api/runs` mostram o desfecho na coluna `delivery`: `entregue`,
`sem_entrega` ou `nao_entregue:<motivo>`.

## Instaladores (#2016, #2023)

- **Windows pela última release.** O comando baixa o `install.ps1` que saiu
  com os binários que ele instala. O raw da `main` fica como espelho, e o
  `garraia.org/install.ps1` não serve ao `irm`.
- **Sai o jsDelivr.** Ele deixa de ser espelho dos instaladores, porque recusa
  repositório acima de 50 MB.

## Atenção ao atualizar

- **Pareados pelo IRC até a v0.5.2:** a entrada na allowlist não tem o
  prefixo `irc:` e deixa de valer no IRC. A pessoa precisa de um código novo
  de `/pair`, que o dono gera no Telegram ou no Discord, num canal em que ele
  é o dono. O bot fica em silêncio com quem é barrado.
- **Sem onde gerar o código:** isso acontece quando o dono foi pareado pelo
  IRC, porque o `/pair` dos outros canais só atende o dono daquele canal, e
  quando a instalação só usa o IRC. A CLI e a API não geram código, e a saída
  é o arquivo. Com o gateway parado, troque cada `"<nick>"` por
  `"irc:<nick>"` no `allowlist.json` e suba de novo. A troca vale em `users`,
  para cada pessoa afetada e também para o dono, e no `owner`. Trocar só o
  `owner` não basta: no IRC, quem admite é `users`. Faça isso só para nicks
  que você conhece:
  `irc:<nick>` admite quem usar aquele nick no servidor. A mensagem de
  boas-vindas que o IRC manda ao novo dono aponta o mesmo caminho.
- **`admin.db` que não abre:** o boot é recusado com o nome do arquivo.
  Corrija a permissão ou o espaço em disco, ou restaure de backup.
- **`/admin` em 503:** veja o guia acima.
- **Conversas privadas antigas do IRC:** ficam na sessão antiga, que não
  recebe turno novo. Cada pessoa começa uma conversa nova.
- **Navegador do Web Console:** passa a exigir Chrome 93, Firefox 92 ou
  Safari 15.4.

## Instalar ou atualizar

```bash
# Linux/macOS
curl -fsSL https://garraia.org/install.sh | sh
```

```powershell
# Windows
irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
```

Já tem a v0.5.2? `garra update`.

Imagem Docker: `ghcr.io/michelbr84/garraia:0.5.3`.
