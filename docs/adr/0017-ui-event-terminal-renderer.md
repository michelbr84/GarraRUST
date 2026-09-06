# 17. Camada de apresentação do CLI — `UiEvent` + `TerminalRenderer`

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão 2026-09-05)
- **Date:** 2026-09-05
- **Tags:** cli, terminal-ux, arquitetura, tracing, testabilidade
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Épico: #944 (Garra Terminal UX v2) — este ADR abre a Fase 2
  - Issue: #942 (introduce `UiEvent` + `TerminalRenderer`)
  - Fase 1 entregue: #933 (sinks de tracing separados), #936 (spinner de estado
    puro), #934 + #935 (layout de conversa e cabeçalho compacto)
  - Consome: #937, #938, #941 dependem desta camada existir

---

## Context and Problem Statement

Hoje a saída do `garra chat` é montada por três donos diferentes que não se
conhecem:

1. **`println!`/`print!` diretos** espalhados por `chat.rs` — prompts, avisos de
   timeout, cancelamento, `/help`, `/context`.
2. **`stream_turn`**, que escreve os deltas do modelo e o rótulo da resposta num
   `impl io::Write`, e ainda hospeda o spinner como um braço do `select!`.
3. **`tracing`**, que até o #933 escrevia no mesmo stderr da conversa. O #933
   separou os sinks, mas nada impede que uma linha de INFO volte a ser usada
   como interface — a separação hoje é convenção, não estrutura.

A Fase 1 do épico arrumou o que dava para arrumar sem mexer na arquitetura: o
console ficou limpo, o spinner virou estado puro, o layout ficou moderno. Mas as
issues restantes (#937 eventos de tool, #938 sumarização de output, #941 erros
acionáveis) **não são cosméticas** — todas precisam saber *o que está
acontecendo no runtime* para decidir o que desenhar, e hoje a única forma de
saber isso seria ler o log ou espalhar mais `println!` dentro do runtime.

A pergunta arquitetural: **como o runtime conta o que está fazendo, sem que
contar vire desenhar?**

## Decision Drivers

1. **★★★★★ Não quebrar a drenagem concorrente do canal limitado.** O runtime
   empurra deltas por um `mpsc` com `send().await`; o receptor **precisa** ser
   drenado durante a chamada, senão o produtor trava quando o buffer enche —
   foi o travamento original do `garra chat`, documentado em `chat.rs`. Qualquer
   desenho novo entra no mesmo `select!`, nunca numa task separada.
2. **★★★★★ Estado puro, relógio de fora.** O `spinner.rs` já é assim
   (`SpinnerState::tick`, sem relógio interno) e é por isso que cada quadro é
   afirmável contra um `Vec<u8>`. O renderer herda a regra, não a revoga.
3. **★★★★ `tracing` continua existindo e independente.** O log persistente é
   diagnóstico, não interface. O renderer não substitui o `tracing`; os dois
   passam a ter destinos e públicos declarados.
4. **★★★★ Não-TTY é comportamento definido, não acidente.** Hoje `NO_COLOR`,
   `TERM=dumb` e stdout redirecionado são tratados em dois lugares
   (`spinner::detect` e `conversation::Style::detect`) que já foram escritos
   para concordar. O renderer passa a ser o dono único dessa decisão.
5. **★★★ Migração incremental.** "Existing chat behavior remains functional" é
   critério de aceite do próprio épico. Nenhum PR desta fase pode exigir um
   big-bang.

## Considered Options

### A. Manter `println!` direto e adicionar helpers (status quo estendido)

Barato e sem arquitetura nova. Mas cada issue da Fase 2 precisaria alcançar o
runtime de algum jeito, e o jeito disponível seria passar mais um `impl Write`
(ou pior, um `&mut dyn FnMut`) por dentro das assinaturas do `AgentRuntime` —
acoplando o crate de agentes à apresentação do CLI. É exatamente o que o #942
existe para evitar.

### B. Adotar um framework de TUI (`ratatui` / `crossterm` full-screen)

Rejeitado, e vale registrar por quê para ninguém propor de novo: um TUI de tela
cheia toma a tela alternativa do terminal, o que **quebra o scrollback** (a
conversa some ao sair), **quebra o pipe** (`garraia 2>/dev/null | cat` deixa de
ter sentido) e obriga a redesenhar em vez de acrescentar linhas. O Garra é uma
CLI de conversa em fluxo, não um painel: a saída é uma sequência de linhas que o
usuário rola, copia e redireciona. O custo do framework é alto e o benefício —
layout bidimensional — é justamente o que não queremos.

### C. `UiEvent` + `TerminalRenderer` (escolhida)

Um enum de eventos de *interface* que o produtor emite e um renderer que decide
como (e se) desenhar cada um.

## Decision

Adotamos **C**.

```text
AgentRuntime
   │
   ├── tracing ──────────────────> arquivo (~/.garraia/garraia.log), redigido
   │                               e stderr filtrado (#933)
   │
   └── UiEvent ──> TerminalRenderer ──> impl io::Write (stdout, ou Vec<u8> em teste)
```

### Onde mora

`crates/garraia-cli/src/ui/` — **no CLI, não em `garraia-agents`**. O runtime não
depende do renderer; ele emite eventos por um canal que o CLI fornece. Um
consumidor que não queira interface (o `garra ask`, o `mcp_server`) simplesmente
não conecta renderer nenhum, e nada muda para ele.

### O que o renderer possui

- a linha de atividade (o `SpinnerState` migra para dentro dele, intacto);
- o texto streamado da resposta e o rótulo `Garra` do #934;
- o ciclo de vida das tools (#937) e a sumarização de output grande (#938);
- avisos e erros de usuário (#941);
- a decisão de cor/Unicode/TTY — hoje espalhada entre `spinner::detect` e
  `conversation::Style::detect`, que passam a ser uma coisa só;
- a largura do terminal.

### O que o renderer NÃO possui

- o relógio: continua vindo do `tokio::time::interval` do `select!`, passado
  como tick. O renderer não dorme, não mede tempo de parede, não spawna nada;
- o `tracing`: um `UiEvent::Warning` **não** vira log, e um `warn!` **não** vira
  linha de interface. Quando os dois precisam acontecer, o produtor faz os dois
  explicitamente — a duplicação é intencional e visível.

### Invariantes que este ADR fixa

1. **O renderer é chamado de dentro do `select!` de `stream_turn`, nunca de uma
   task própria.** Esta é a linha que protege a drenagem do canal limitado. Um
   renderer com task própria e canal próprio seria a forma mais natural de
   escrever isto e é exatamente o bug que já derrubou o `garra chat` uma vez.
2. **Todo caminho de desenho aceita um `impl io::Write`**, então todo teste
   afirma contra um `Vec<u8>` — inclusive os caminhos ASCII, sem cor e não-TTY,
   que são os que ninguém exercita à mão e por isso quebram sem ninguém ver.
3. **O cursor nunca é escondido.** `\x1b[?25l` não é emitido em lugar nenhum,
   como já vale para o spinner: nenhum caminho de saída — sucesso, erro,
   timeout, Ctrl+C, pânico — pode deixar o terminal sem cursor.
4. **Não-TTY não emite sequência de escape alguma.** Verificável por teste que
   varre a saída procurando `\x1b`.

## Consequences

### Positivas

- #937, #938 e #941 passam a ser trabalho de renderer, não de espalhar `println!`
  pelo runtime.
- O `AgentRuntime` deixa de ter qualquer motivo para escrever no terminal, o que
  torna o crate reusável fora do CLI sem carregar apresentação junto.
- Snapshot test vira o modo default de testar UX: hoje os invariantes do spinner
  já são afirmados assim, e a Fase 2 herda a prática em vez de inventá-la.

### Negativas, assumidas

- **Uma indireção a mais** entre "aconteceu" e "apareceu". Para o `println!` de
  duas linhas do `/help` isso é overhead puro — por isso comandos síncronos que
  não participam de um turno podem continuar escrevendo direto; o renderer é
  para o que compete pela mesma linha durante o streaming.
- **Risco de reintroduzir o travamento** se alguém, no futuro, "melhorar" o
  renderer dando a ele uma task própria. Mitigado pelo invariante 1 estar aqui e
  no doc de `stream_turn`, e por um teste que falha se o spinner deixar de ser
  braço do `select!`.
- **Duas verdades sobre o mesmo evento** quando algo precisa ser logado *e*
  mostrado. É deliberado: unificar os dois foi o que fez o console virar despejo
  de INFO, que é o problema que o #933 acabou de consertar.

## Plano de migração (incremental, sem big-bang)

1. **#942** cria `ui/` com o enum e o renderer, e migra para dentro dele **o que
   já é puro**: o `SpinnerState` e o `conversation::Style`. Comportamento
   visível idêntico — o PR se prova por os testes existentes continuarem
   passando sem alteração.
2. **#937** acrescenta os eventos de tool, que hoje não existem no stream. É a
   primeira capacidade nova, e é o que destrava o "retomar o spinner após a
   tool" que ficou de fora do #936 por não haver evento para ouvir.
3. **#938** e **#941** entram como novos braços do mesmo enum.
4. Os `println!` de comandos síncronos (`/help`, `/context`, `/history`) migram
   por último, ou não migram — decisão do PR que encostar neles.

## Verificação

```bash
cargo +1.95 test -p garraia --bin garra ui::          # snapshot do renderer
cargo +1.95 test -p garraia --bin garra spinner::     # invariantes preservados
cargo +1.95 test -p garraia --bin garra conversation::
```

Fim a fim: `garraia` interativo comparado ao mockup do épico; `NO_COLOR=1`,
`TERM=dumb` e `garraia 2>/dev/null | cat` para os três caminhos degradados.
