# garraia-desktop-core

Núcleo do **GarraIA Desktop Control Center** — a lógica do control center que
**não** depende de Tauri.

Decisão: [`docs/adr/0021-garraia-desktop-control-center.md`](../../docs/adr/0021-garraia-desktop-control-center.md)
(opção D). Épico: [#1181](https://github.com/michelbr84/GarraRUST/issues/1181),
milestone **M0**.

## Por que esta crate existe

`garraia-desktop` (a casca Tauri) está excluída de todos os gates obrigatórios
de CI — clippy, build e test rodam com `--exclude garraia-desktop`, porque o
`build.rs` do Tauri exige GTK/webkit que os runners não têm. O efeito é que
aquela crate tem **zero cobertura automatizada**.

Construir o control center inteiro lá dentro seria escrever milhares de linhas
que ninguém sabe quando quebram. Esta crate é o outro lado dessa fronteira:
toda a lógica que não precisa de janela mora aqui, **entra nos gates normais** e
tem teste. A casca Tauri fica fina de propósito — janelas, bandeja, hotkeys,
autostart, updater.

Não é uma camada de abstração especulativa: é onde o código testável mora.

## Módulos

| Módulo | O que é | Invariante que o define |
| --- | --- | --- |
| `state` | Estado ligado/desligado dos módulos do desktop | Puro: sem relógio, sem I/O, sem thread. Só avança quando alguém o manda avançar — mesmo padrão do `spinner.rs` da CLI |
| `detect` | Detecção de agentes externos (GarraIA, Hermes, OpenClaw, Claude Code, AgentDeck) | **Leitura, nunca execução.** Um nome na `PATH` não prova identidade; sem corroboração a detecção fica `Ambiguous` |
| `supervise` | Launch / restart / kill de processo filho | O filho morre junto com o supervisor, via `Drop` — inclusive em caminho de panic |

### `state` — intenção separada de realidade

`Desired` é o que o usuário pediu; `Power` é o que de fato está acontecendo.
Separá-las é o que permite a UI mostrar "ligando…" sem mentir que já ligou, e o
que distingue *"o usuário desligou"* de *"caiu sozinho"*: um módulo que cai
continua com `Desired::On` e aparece como `Power::Failed`.

### `detect` — fail-closed, por construção

As três regras da issue #1181: detecção é leitura; adicionar é decisão do
usuário; executar é ação explícita.

A lição vem de `crates/garraia-cli/src/agents.rs`: o nome `agentdeck` no npm
pertence a um projeto **diferente e sem relação**. A CLI resolve isso rodando
`agentdeck agents --help` — recurso que aqui é proibido, porque executar é
exatamente o que a regra 1 veda.

A saída é classificar em vez de adivinhar. Só o binário na `PATH` →
`Confidence::Ambiguous`, e `is_identified()` responde `false`. Binário **mais**
uma marca que o agente de verdade deixa (seu diretório de config) →
`Confidence::Confirmed`. A dúvida nunca vira confirmação.

A garantia é estrutural, não documental: o trait `Filesystem` só sabe responder
"isso existe?", e um teste varre o próprio fonte do módulo atrás de
`Command::new`, `.spawn()` e companhia.

### `supervise` — o mesmo primitivo, agora com teste

Extraído de `crates/garraia-desktop/src-tauri/src/gateway.rs`, que já
supervisiona o `garraia` como sidecar. O que mudou:

- **Sem `unwrap` em lock** — lock envenenado vira `SuperviseError::Poisoned`.
- **Sem `sleep` por dentro** — `RestartPolicy::backoff` devolve o intervalo e
  quem tem o relógio espera. É o que mantém o teste sem `sleep` real.
- **`Spawner` injetável** — o ciclo de vida inteiro é testável sem criar
  processo de verdade.

## Estado atual

M0 entrega a crate, os três módulos e os testes. **Nenhuma outra crate a
consome ainda**: migrar `garraia-desktop` para usar estes primitivos é dos
milestones seguintes, e a CLI ganha `garraia desktop` no M1.

## Testes

```bash
cargo test -p garraia-desktop-core
cargo clippy -p garraia-desktop-core --all-targets --all-features -- -D warnings
```
