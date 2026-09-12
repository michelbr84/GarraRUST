# 20. Crate `garraia-hardware` — abstração de dispositivos físicos

- **Status:** Accepted (2026-09-12, decisão do dono — opção A, ver nota abaixo)
- **Deciders:** @michelbr84 (decisão final) + Claude (levantamento, sessão
  autônoma 2026-09-11)
- **Date:** 2026-09-11
- **Tags:** hardware, iot, plataforma, arquitetura, seguranca
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Epic: [#1124](https://github.com/michelbr84/GarraRUST/issues/1124) —
    "roadmap: garraia-hardware — observar, entender e agir no mundo físico"
  - Issues-filhas: [#1125](https://github.com/michelbr84/GarraRUST/issues/1125)
    (crate + `trait Device`), [#1126](https://github.com/michelbr84/GarraRUST/issues/1126)
    (adapter MQTT), [#1127](https://github.com/michelbr84/GarraRUST/issues/1127)
    (adapter Home Assistant), [#1128](https://github.com/michelbr84/GarraRUST/issues/1128)
    (motor de automações), [#1129](https://github.com/michelbr84/GarraRUST/issues/1129)
    (modelo de risco R0-R5), [#1130](https://github.com/michelbr84/GarraRUST/issues/1130)
    (Serial/USB + GPIO), [#1131](https://github.com/michelbr84/GarraRUST/issues/1131)
    (hardware skills)
  - Regra absoluta 8 do `CLAUDE.md`: decisão arquitetural irreversível pede ADR
  - Precedente de processo: [ADR 0018](0018-crate-garraia-embeddings.md) — mesmo
    padrão de "ADR Proposed escrito numa sessão autônoma, execução aguarda aceite"

## Context and Problem Statement

O epic #1124 (crédito: "conselho Hera+Garra", 2026-09-10) propõe posicionar o
GarraIA para atuar no mundo físico — não como feature isolada de GPIO, mas
como camada de plataforma: dispositivos IoT/hardware viram mais uma superfície
que o agente enxerga e opera, ao lado de arquivos, chat e ferramentas web. A
alavancagem citada é real: adaptando MQTT e Home Assistant, o Garra herda
descoberta de milhares de dispositivos sem escrever driver por fabricante.

O epic já chegou **com 7 issues-filhas sequenciadas** pelo dono
(comentário em #1124), acceptance criteria concretos em cada uma (ex.: #1125
pede `cargo test -p garraia-hardware` com `MockDevice`, tool exposta em
`GET /api/mcp/health`), e um modelo de risco (#1129) desenhado para reusar o
`safety_gate`/`ToolApproval` existentes em vez de duplicá-los. Isto não é
brainstorm — é plano de execução.

O que falta é o documento que a regra absoluta 8 exige **antes** de a crate
nascer: hoje `CLAUDE.md` lista o workspace como "sem crates planejados no
momento", e toda crate ativa do repo (`garraia-embeddings` incluída, via ADR
0002) tem uma decisão arquitetural registrada antes do primeiro `cargo new`.
Uma crate nova — com trait pública, modelo de segurança próprio e adapters
que falam com rede local e serviços de terceiros — é exatamente o tipo de
decisão que o índice de ADRs (`docs/adr/README.md`) cita como exemplo:
"escolha de vector store", "protocolo de autenticação", "runtime de sandbox".
Hardware físico half-implica todas as três: é superfície nova de I/O, sujeita
a autorização, e roda fora do sandbox de processo (`PR_SET_DUMPABLE`/Landlock,
ADR 0019) porque fala com o mundo via rede/serial, não com o filesystem local.

## Decision Drivers

1. **Reversibilidade real, mas não trivial.** Ao contrário de
   `garraia-embeddings` (crate sem consumidores, remoção = `git revert`), uma
   vez que `garraia-hardware` ganhe adapters e a tool for exposta ao agente
   via MCP/tool nativa, desfazer significa também revisar qualquer automação
   ou memória que já referencie um `device_id`. A decisão de nascer bem —
   risco embutido na `Capability` desde o primeiro commit (#1129 "transversal,
   deve nascer com o Device trait") — evita a alternativa cara: adicionar
   controle de acesso depois que dispositivos reais já estão conectados.
2. **Superfície de segurança nova, não delegável a review tardia.** Hardware
   real tem ações irreversíveis no mundo físico (destrancar porta, mover
   braço robótico) — categoria de risco que o repo não tinha até agora.
   `ToolApproval` e o tier *risky* do `safety_gate` (GAR-187/GAR-497) cobrem
   comandos de shell; estender o mesmo gate para `device_execute` é reuso
   deliberado, não um sistema paralelo — mas precisa estar decidido, não
   descoberto issue a issue.
3. **Não é dívida técnica especulativa.** Diferente do crate de embeddings
   (escrito 4 meses antes de ter consumidor), aqui as issues-filhas têm
   aceite testável e ordem de dependência explícita. O ADR não está tentando
   *prever* um caso de uso — está formalizando um que já tem 7 issues
   detalhadas.
4. **Escopo do core.** O epic é explícito: "sem drivers no core" — a crate
   define só `trait Device` + registry + política; MQTT/HA/Serial vivem em
   crates/adapters separados, adicionados aditivamente. Isso limita o raio
   da decisão irreversível ao trait em si, não a cada transporte.

## Considered Options

### A. Criar a crate `garraia-hardware` agora, como especificado nas issues-filhas

Nasce com `trait Device` (`id`/`capabilities`/`read`/`execute`,
`#[async_trait]` por ser usado como `dyn Trait` — mesma exceção documentada em
`CLAUDE.md` para `garraia_storage::ObjectStore`), `Capability` com risk class
R0-R5 embutida, registry em `Arc<dyn Device>`, e o gate de execução ligado ao
`ToolApproval`/`safety_gate` existente desde o primeiro commit. Adapters
(MQTT, Home Assistant, Serial/GPIO) entram depois, um por issue, cada um
aditivo ao workspace.

- **A favor:** segue o plano já sequenciado pelo dono; risco nasce embutido
  em vez de vir depois; adapters isolados mantêm o core pequeno e testável
  (`MockDevice` in-memory, sem hardware real no CI); reusa infraestrutura de
  autorização existente em vez de duplicá-la.
- **Contra:** adiciona superfície de I/O física nova ao produto — a primeira
  categoria de ação do agente com efeito fora do processo/filesystem/rede já
  conhecidos. Compromisso de manutenção de longo prazo (7+ issues, múltiplos
  adapters).
- **Mitigação do contra:** o modelo R0-R5 (#1129) é *fail-closed por padrão*
  — R5 é deny-by-default com allowlist explícita, e nenhuma capability roda
  sem classificação. O escopo inicial (#1125+#1129 juntos) não conecta
  hardware real nenhum; só depois de MQTT/HA (#1126/#1127) existe qualquer
  I/O físico de verdade, e cada um é seu próprio issue/PR/review.

### B. Dobrar em `garraia-tools` em vez de crate nova

Hardware seria só mais um grupo de tools dentro do crate de tools
compartilhadas existente, sem trait/registry dedicados.

- **A favor:** zero crate nova, zero linha no `Cargo.toml` raiz.
- **Contra:** `garraia-tools` hoje é file ops/search/web — tools
  request-response, sem noção de dispositivo com estado (online/offline,
  `last_seen`) nem de descoberta dinâmica. Forçar isso lá dentro mistura
  domínios e contradiz o próprio desenho das issues-filhas (registry +
  adapters como conceito de primeira classe). Reverter essa mistura depois
  é mais caro que separar desde o início.

### C. Adiar — não decidir agora, esperar caso de uso mais concreto

Mesmo raciocínio que motivou o ADR 0002 a não implementar `garraia-embeddings`
cedo demais.

- **A favor:** zero risco imediato; nenhuma superfície nova até haver certeza.
- **Contra:** ao contrário do cenário de embeddings, aqui **já existe** o
  caso de uso concreto — 7 issues com aceite testável, dispositivo mock
  incluso no critério de aceite do primeiro (#1125). Adiar não elimina
  ambiguidade (não há ambiguidade: o plano está escrito); só atrasa o
  trabalho que o dono já pediu.

## Decision Outcome

**Recomendação: opção A — criar a crate `garraia-hardware` seguindo a
sequência já definida pelo dono em #1124** (fundação #1125+#1129 juntos →
#1126 → #1127 → #1128 → #1130 → #1131; ROS2 fora do escopo inicial, de
propósito).

O argumento decisivo é que o epic não é uma proposta em aberto — é um plano
com critérios de aceite testáveis, ordem de dependência explícita e um
modelo de segurança desenhado para reusar (não duplicar) o gate de risco que
o repo já tem para bash/tools. O que faltava para essa primeira issue
(#1125) poder começar sob a regra 8 era este documento.

> **Aceito em 2026-09-12 (opção A)** — decisão do dono na sessão de
> implementação: "Sim — aceita e implementa". A implementação começou na
> mesma sessão: #1125 (crate `garraia-hardware`) + #1129 (risco R0–R5
> embutido na `Capability` e no `HardwareGate`) entregues juntos, conforme
> a sequência cravada pelo epic #1124.

## Consequences

**Se aceito (A):**

- `crates/garraia-hardware/` nasce como novo membro do workspace, seguindo
  #1125: `trait Device` + `Capability` (nome, risk class R0-R5, schema leve
  de args, read-only flag) + registry `Arc<dyn Device>` + `MockDevice` de
  teste. `CLAUDE.md` §"Estrutura de crates" ganha a entrada e a contagem sobe
  de 22 para 23; a nota "sem crates planejados no momento" sai.
  Este ADR não implementa nada — a implementação é o trabalho de #1125.
- #1129 (risco R0-R5) nasce **junto** com #1125, não depois — a
  `Capability` já carrega risk class desde o primeiro commit, e o gate
  reusa `ToolApproval`/`safety_gate` em vez de um sistema paralelo.
- Adapters (#1126 MQTT, #1127 Home Assistant, #1130 Serial/GPIO) entram
  aditivamente, cada um seu próprio PR/review, sem tocar o core.
- Nenhum dispositivo físico real é conectado por este ADR nem por #1125 —
  isso só começa em #1126/#1127, cada um com seu próprio risco a avaliar
  (rede local, credenciais de terceiros) no momento do PR correspondente.

**Se recusado (B ou C):**

- O epic #1124 fica bloqueado sem crate — as 7 issues-filhas continuam
  abertas e nenhuma pode começar sob a regra 8 sem um novo ADR substituindo
  este.
- Se a escolha for B (dobrar em `garraia-tools`), as issues-filhas precisam
  ser reescritas — o desenho de registry/adapter delas assume crate própria.

## Verificação

A afirmação central deste ADR — "o plano já está sequenciado e tem aceite
testável" — é verificável relendo #1124 (comentário de sequenciamento) e
#1125/#1129 (critérios de aceite), e deve ser reconferida antes de #1125
começar, caso as issues tenham sido editadas desde 2026-09-11.
