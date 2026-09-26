# ADR 0025 — Classes de capacidade, teto por principal e Access Policy v2 do WhatsApp

- **Estado:** Accepted (2026-09-26, decisão do dono no prompt da missão v0.4.6: escopo integral, #1462 opção 3, #1425 opção B)
- **Issues:** #1385, #1392, #1391, #1388, #1390, #1398, #1397, #1399, #1423, #1412, #1421, #1381, #1387, #1425, #1482
- **Supersede parcial:** o piso por nome de modo da #1327/#1384 (continua valendo como precisão e compatibilidade, deixa de ser a única regra)

## Contexto

Ate a v0.4.5 a politica de ferramentas era **por nome**: `allowed`/`denied` de um `ModeProfile`, com
`servidor/*` e, desde a #1384, `*/<operacao>` para MCP. Funcionava enquanto o WhatsApp pessoal tinha um
unico piso (`search`) e um unico privilegio (dono em `isolated-pod`). Tres coisas quebraram isso:

1. **#1482** — a raiz do MCP `filesystem` em `standard` e o pai de todo diretorio de sessao; com a
   leitura MCP no `search`, um contato lia a sessao do outro. O portao por nome nunca ve o `path`.
2. **#1391/#1392/#1398** — o produto precisa de principais com politicas diferentes no mesmo numero
   (dono, usuario, desconhecido, grupo), de um `write on|off` que nao seja um booleano que atravessa
   sandbox/jail/confirmacao, e de niveis `chat|read|full` legiveis por quem administra.
3. **#1385/#1387** — uma ferramenta MCP nova de leitura precisava de entrada manual para ser vista; uma
   de escrita passava se alguem liberasse o servidor inteiro; e o modelo dizia "nao tenho MCP" quando o
   MCP existia e estava escondido.

## Decisao

### 1. Classes de capacidade (`garraia_agents::capacidades`)

Toda ferramenta declara **o que faz** em classes fechadas (`filesystem.read`, `filesystem.write`,
`process.execute`, `network.read`, `message.send`, `device.read`, `device.execute`, `memory.read`,
`memory.write`, `runtime.inspect`, `scheduling`, `mcp.read`, `mcp.write`):

- nativa: tabela fechada por nome (`NATIVAS`); nome fora da tabela nao tem classe;
- MCP: operacao de filesystem conhecida (por nome, em qualquer servidor — a mesma lista do confinamento
  da #1482) → `filesystem.*`; senao as anotacoes do servidor (`readOnlyHint` → `mcp.read`,
  `destructiveHint`/`readOnlyHint=false` → `mcp.write`); sem anotacao, **sem classe**;
- **sem classe e fail-closed** num modo que restringe por classe.

`ToolPolicy` ganha `allowed_capabilities`, `denied_capabilities` e `no_tools`. Regra de decisao
(`politica_permite`): `no_tools` nega tudo; `denied` por nome ou por classe vence; com `whitelist_mode`, a
ferramenta passa pelo nome (as tres formas de sempre) **ou** pela classe (todas as classes dela
liberadas, e pelo menos uma); listas vazias com `whitelist_mode` seguem permitindo tudo (#1264).

### 2. Teto por principal, composto no `ToolGate`

O `ExecContext` ganha `teto: Option<TetoDeCapacidades>` (`nome` + `ToolPolicy`). O portao do turno e
`modo ∧ teto`: o modo escolhido pela sessao (`/mode`, customizado, `auto`) libera o que o teto tambem
libera, e nada alem. O teto **nunca acrescenta**. A recusa cita o modo quando foi o modo, e a politica de
acesso quando foi o teto — porque trocar de modo nao resolve o segundo caso.

Todo ponto do runtime pergunta por **nome e classe** (`AgentRuntime::portao_permite`): as tres listas
que o modelo ve, o despacho e o `garra_status`. Guardas de fonte prendem isso.

### 3. Niveis e `write` como compilacao, nunca como bypass

`Nivel { chat, read, full }` × `write on|off` compilam para um `ToolPolicy` de teto
(`politica_do_nivel`):

| Nivel | Teto |
|---|---|
| `chat` | `no_tools`: nenhuma ferramenta |
| `read` | whitelist por classe = `LEITURA` (`filesystem.read`, `network.read`, `device.read`, `memory.read`, `runtime.inspect`, `mcp.read`); nega `process.execute`, `device.execute`, `message.send`, `memory.write`, `scheduling`; `write on` acrescenta `filesystem.write` + `mcp.write` |
| `full` | sem restricao propria (o modo e o perfil de execucao decidem); `write off` nega so `filesystem.write` + `mcp.write` |

`write` mexe **so** em escrita de arquivo (nativa e MCP); shell, dispositivo, mensagem e agenda sao
controles independentes. `write on` nao desliga sandbox, jail, gate de comando arriscado nem a
confirmacao GAR-187: o teto so tira, nunca poe.

### 4. Access Policy v2 do WhatsApp pessoal (secao `channels.whatsapp_linked.access`)

```yaml
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    allow: ["+5511999998888"]        # legado, continua valendo (= usuario sem teto)
    owners: ["+5511977776666"]       # legado, continua valendo (= dono)
    reply_in_groups: false           # legado (= access.groups.enabled)
    access:
      admission: restricted          # restricted | open; default restricted
      default: { level: chat, write: false }   # o que um DESCONHECIDO recebe quando admission=open
      users:
        "+5511999998888": { level: read, write: false }
        "+5511977776666": { role: owner }
        "+5511955554444": { blocked: true }
      groups:
        enabled: false
        default: { level: read, write: false }  # o piso de hoje para grupo
        "120363@g.us": { level: read }
```

- **Principais** (resolvidos por turno, pela `chave_do_portao` de sempre): `Dono`, `Usuario`,
  `Pareado` (codigo, memoria do processo), `Desconhecido` (so com `admission: open`), `Bloqueado`
  (nega mesmo em `open`), `Grupo` (politica do grupo; identidade do remetente so para aprovacoes) e
  `Estranho` (ninguem o conhece em `restricted`: nao e admitido). Grupo **nunca** herda dono.
- **Efetivo** = nivel/write do principal → `politica_do_nivel` → teto; o piso de modo continua o de
  hoje (`search`; dono em pod `code`); `full` do dono em `standard` e `code` sem `bash` (o runtime nao
  registra `bash` sem sandbox, ADR 0024). `open` nunca da `full` por default (#1390); `chat` com
  `write: true` e invalido e cai em `chat`. Nivel e `write` so existem onde foram **declarados**
  (`access.users`, `access.default`, `access.groups`): `allow` legado e grupo sem politica ficam
  **sem teto** — o piso de modo decide sozinho, como sempre decidiu. A unica excecao e o pareado por
  codigo, que passa a ter teto `read` (credencial fraca). Nivel declarado em grupo e honrado (inclusive
  `full`), mas o grupo continua no piso de grupo: nunca o perfil `Completo` do dono.
- **Compatibilidade:** `allow`/`owners`/`reply_in_groups` sem `access:` produzem o comportamento da
  v0.4.5 (a menos do pareado, acima). `access.users` vence o legado para a mesma identidade.
- **Hot reload (#1412):** a secao inteira e relida por turno da config viva (como `allow`/`owners`
  hoje); cada turno ve um snapshot consistente; config semanticamente invalida normaliza fail-closed
  com um `warn!` unico.
- **Uma unica trilha de mutacao (#1412/#1414):** CLI, admin API e Web Console chamam as mesmas funcoes
  do gateway (`bootstrap::whatsapp_linked::politica`), que gravam `config.yml`, validam, e anotam no
  audit log local (`<data_dir>/audit/whatsapp-access.jsonl`: ator, origem, alvo mascarado, antes,
  depois; nunca segredo nem conteudo de mensagem; retencao configuravel).
- **Dry-run (#1413):** a mesma engine calcula a matriz efetiva antes e depois; nada e gravado.

### 5. Registro de capacidades e honestidade (#1381, #1387, #1425 opcao B)

Uma unica funcao no gateway produz, para (estado, sessao, principal), a lista de capacidades com
estado `registered | visible | denied(policy) | unavailable(reason) | unhealthy | not_configured` e
motivo legivel por maquina e por humano; `garra_status`, `/api/diagnostics`, o painel por conversa e a
matriz por principal consomem dela. Tools de acao de canal (`telegram_send`) so entram na lista
chamavel quando o canal esta configurado **e** conectado; fora disso aparecem como `unavailable` com o
motivo — e nunca somem em silencio.

## Consequencias

- `search` e os outros nativos **continuam por nome** (compatibilidade); ganham `allowed_capabilities`
  onde isso so acrescenta leitura segura. Uma tool MCP de leitura anotada passa a ser vista num modo
  restrito sem entrada manual (#1385 c6).
- Toda tool nativa nova precisa de linha em `NATIVAS`, senao fica sem classe (um teste lista as 17).
- O confinamento do MCP `filesystem` (#1482) e a outra metade desta decisao: classe diz **o que**
  a ferramenta faz; o jail diz **onde**.
- Divida assumida: `tool_program` fica sem classe (cada passo passa pelo proprio portao) e por isso
  fora dos niveis `chat`/`read`.

## Alternativas rejeitadas

- **Booleano `write` no `ExecContext`** que a tool consulta: atravessa o `ToolGate`, o `HardwareGate` e a
  confirmacao; foi o que a #1392 proibiu.
- **`open` como `*` na lista `allow`**: uma identidade falsa; `admission` e politica declarada (#1405).
- **Servidor MCP `filesystem` por sessao**: um processo `node` por conversa; o confinamento de
  argumentos da o mesmo resultado sem o custo.
