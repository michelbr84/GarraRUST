# Novidades da v0.3.9

> 🇬🇧 [English version](Whats-New-v0.3.9) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.9)

A maior release do projeto até aqui: **48 issues e PRs**, de #933 a #1012, além
do lote de retorno de campo que já estava pronto desde 05/09 (#920–#930, #966).

A v0.3.9 chegou a ser preparada em 05/09 e nunca foi publicada — não houve tag.
O que estava pronto naquele dia ficou, e o que entrou depois entrou junto.

---

## O que você vai notar usando

### O terminal parou de ser uma caixa preta

Antes, executar uma ferramenta era invisível: você via o indicador de atividade
e, minutos depois, a resposta pronta — sem saber se o agente estava lendo um
arquivo, rodando `cargo test` ou simplesmente parado.

```
● Bash cargo test
  └─ 148 passed · 6.3s · #7

× Bash └─ error: exit 101 · 4.2s · #8
```

Na falha o nome aparece de novo na mesma linha, de proposito: depois de uma
saida longa, a linha de inicio pode ter rolado para fora da tela. O `#7` e um
ponteiro — `/tool 7` mostra a saida inteira daquela chamada.

Junto vieram **Markdown renderizado sem parar o streaming** (o texto continua
aparecendo enquanto chega, com títulos, listas e blocos de código formatados),
novas superfícies de status (`/status`, `/tools`, `/logs`) e o comando
`garra logs`.

### `garra chat 2>/dev/null | cat` não carrega mais escape ANSI

A cor era incondicional em vários caminhos — `/help`, `/context`, `/history`, a
despedida. Redirecionar a saída trazia lixo de escape junto. Agora a CLI diz
*o quê* e o renderizador decide *como*, olhando se há terminal do outro lado.

### A memória ficou inspecionável

O `garra memory` deixou de ser só leitura:

```bash
garra memory add "O condominio fica na rua das Flores" --session projeto-x
garra memory reindex        # gera vetor para o que ficou sem
garra memory backup         # snapshot consistente, com retenção
garra memory pin <id>       # a retenção nunca apaga
garra memory ttl <id> 30    # ou expira em 30 dias (numero, nao "30d")
garra memory stats          # contagens + relatório de integridade do índice
```

### Os modos passaram a valer de verdade

Este é o item que mais muda comportamento — veja abaixo.

---

## Um padrão atravessa o lote: *restrição declarada que ninguém aplica*

Quatro defeitos diferentes, a mesma forma. Cada um prometia uma propriedade que
o código não entregava:

| Onde | O que prometia | O que acontecia |
|---|---|---|
| `ToolPolicy` dos modos (#988) | modos `search`/`review`/`architect` são somente-leitura | **nunca verificada no executor** — só pedia no prompt do sistema |
| `tools:` de agente nomeado (#965) | *"Restrict which tools this agent can use"* | não era lido em lugar nenhum |
| `X-User-Id` em `/v1/chat/completions` (#1012) | identidade do chamador | header forjável decidia o que era gravado |
| `continuity_key(_user_id)` | barramento de memória por pessoa | devolvia `bus:shared-global` para todos |

**Se você usa modos**, é a mudança que mais importa: um modo anunciado como
somente-leitura agora **bloqueia** `file_write` e `bash` no executor, em vez de
apenas pedir educadamente no prompt.

---

## Segurança

- **`/v1/chat/completions` deixou de derivar identidade do chamador** (#1012).
  A rota é auth-free por desenho, como todo o `/api/*` — mas auth-free significa
  "não exige credencial", e não "aceita a identidade que o chamador afirmar".
  Um `Bearer` arbitrário virava o `user_id`; sem ele, o `X-User-Id` era usado
  cru. Impacto medido: **atribuição falsa**, não leitura cruzada — a carga de
  histórico sempre foi chaveada por `session_id`. Nenhum cliente quebra.
- **Isolamento de tenant no caminho KNN** do recall. O índice vetorial só
  conhece distância; o fetch dos candidatos não aplicava nenhum dos filtros da
  query. Com o índice ativo, um recall com `tenant_id` definido podia devolver
  memória de outro tenant.
- **Saída de ferramenta e texto do modelo não injetam ANSI** no terminal
  (#995, #996) — um arquivo lido com escape dentro não repinta mais a sua tela.
- **Corpo de erro de provider de embeddings** deixou de poder ir cru para o log:
  401/403 perdem o corpo por completo, e os demais são truncados com tokens
  raspados. (A OpenAI ecoa de volta a chave que você mandou quando ela está
  errada.)

---

## Três documentos que contradiziam o binário

Corrigidos ao serem conferidos contra ele, não relidos:

- o `retriever` do `garraia-learning`, que se descrevia como funcional sendo um
  stub (#964);
- o `docs/src/memory.md`, que negava a existência de um comando que passou a
  existir (#963);
- o `docs/memory.md` — a página para onde **este wiki apontava** — que ainda
  descrevia `facts.json` e os comandos `clear`/`export`/`disable`, que nunca
  existiram.

---

## Como atualizar

```bash
garra update          # instalações existentes
```

Ou reinstale:

```bash
curl -fsSL https://garraia.org/install.sh | sh          # Linux / macOS
irm https://garraia.org/install.ps1 | iex               # Windows
```

Os binários crus (`garraia-<os>-<arch>`) continuam publicados ao lado dos
archives — o `garra update` resolve o asset por nome exato e exige o
`<asset>.sha256` irmão.

---

## Também nesta versão

- **Benchmark de qualidade de recall em português** (#958): 40 consultas com
  ground truth em 13 grupos que separam tipos de falha — paráfrase, sinônimo,
  consulta de uma palavra, consulta em espanhol contra corpus em português.
  Não é gate de CI, e não deve virar um.
- **ADR 0017** (TerminalRenderer) e **ADR 0018** (o crate órfão
  `garraia-embeddings`, ainda `Proposed`).
- **`changelog.d/`**: PRs não editam mais o `CHANGELOG.md` direto — cada um
  deixa um fragmento, e dois PRs paralelos deixam de colidir.
- `/goal` persistente por sessão e `/stats` mostrando o turno efetivo.

---

**Detalhe completo, item por item:** [CHANGELOG.md](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md)
