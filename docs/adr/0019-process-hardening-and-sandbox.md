# 19. Confinamento das tools: o que fechamos agora e o que fica para depois

- **Status:** Accepted
- **Deciders:** @michelbr84 (decisão final) + Claude (implementação, sessão autônoma 2026-09-09)
- **Date:** 2026-09-09
- **Tags:** seguranca, tools, sandbox, linux, procfs
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: #1084 (itens 1 e 4 do #1078)
  - Avaliação anterior: `docs/security/hardening-decisions.md` §5 e §6
  - Regra absoluta 8 do `CLAUDE.md`: decisão arquitetural irreversível pede ADR

---

## Context and Problem Statement

O `#1075` fechou a **herança** de ambiente: um filho de tool recebe `PATH`,
`HOME`, `LANG`, `LC_ALL`, `TERM`, `USER` e nada mais. O `#1078` fechou os
fake-negativos do gate de comandos e vinculou a aprovação humana ao comando
aprovado.

Ficou aberto o **procfs**: um filho de mesmo UID abre `/proc/<ppid>/environ` e
lê o ambiente do pai — que num gateway é onde vivem `GARRAIA_JWT_SECRET`,
`ANTHROPIC_API_KEY` e companhia. O gate lista `environ` no `CONFIRM_LIST`, mas
isso só inspeciona o **texto** de um comando, não o que roda dentro de um
script que o gate mandou alguém confirmar sem ler.

Medido nesta máquina (Linux 7.0), antes de qualquer mudança:

```text
filho vê: SEGREDO_DO_PAI=abracadabra
```

O canal é real, e não teórico.

## Decision Drivers

1. **Não inventar política sem dono.** A avaliação do `#1078` travou exatamente
   aqui: confinar o agente exige responder "quais caminhos ele pode ler e
   escrever?", e uma resposta errada quebra uso legítimo em vez de proteger.
2. **Não fingir proteção.** Um mecanismo que só existe numa das três
   plataformas obriga a documentação a se desmentir em cada página.
3. **Não trocar vazamento estreito por indisponibilidade.** Um gateway que se
   recusa a subir porque um knob de hardening falhou é pior que um que sobe e
   diz que falhou.

## Considered Options

### A. Landlock, confinando o filho a uma allow-list de caminhos

Era a opção que a avaliação do `#1078` favorecia. Confirmado nesta sessão que o
kernel local expõe `landlock` na lista de LSMs e que o crate está disponível.

Rejeitada **para este canal**, por três razões concretas que só apareceram ao
projetar a implementação:

- Landlock não tem regra de negação: você concede hierarquias. "Negar `/proc`"
  vira "conceder todo o resto", isto é, enumerar `/usr`, `/bin`, `/etc`,
  `/home`, `/tmp`, `/var`, `/opt`… — uma allow-list de sistema de arquivos, que
  é precisamente a política de produto que ninguém decidiu.
- Negar `/proc` inteiro quebra uso legítimo: `cargo` chama `current_exe()`, que
  lê `/proc/self/exe`; runtimes leem `/proc/self/maps`, `/proc/meminfo`,
  `/proc/sys/...`. Reconceder seletivamente esbarra em `/proc/self` ser um
  symlink resolvido **no pai** — abrir `/proc/self` antes do `fork` aponta para
  o processo cujos segredos queremos esconder.
- Exige `pre_exec`, onde só código async-signal-safe é seguro; montar o ruleset
  ali dentro aloca depois do `fork`.

### B. `prctl(PR_SET_DUMPABLE, 0)` no processo que **carrega** os segredos

Inverte a pergunta: em vez de restringir o filho, torna o pai ilegível. O
kernel passa a tratar `/proc/<pid>/environ` como root-only.

### C. Não fazer nada e continuar documentando como "seatbelt, not sandbox"

O status quo desde o `#1078`.

## Decision Outcome

**Escolhida: B**, e a A **não** é descartada — muda de alvo.

O canal do `#1084` item 1 é uma assimetria de leitura entre dois processos do
mesmo UID. Ele se fecha com uma syscall, sem política, sem allow-list para
errar, e sem dano colateral. Medido, antes e depois, nos caminhos que importam:

| | dumpable=1 | dumpable=0 |
| --- | --- | --- |
| filho lendo o environ do pai | **lê o segredo** | **negado (EACCES)** |
| `/proc/self/exe` (usado por `current_exe`) | ok | ok |
| `/proc/self/maps`, `cmdline`, `status`, `fd` | ok | ok |
| `/proc/meminfo` | ok | ok |
| spawn de filho | ok | ok |

Só `environ` muda de estado — que é o que queríamos.

### Por plataforma

| Plataforma | Decisão |
| --- | --- |
| Linux, Android (Termux) | `prctl(PR_SET_DUMPABLE, 0)` no início do `main` |
| macOS, Windows | **nada a fazer**: não existe `/proc/<pid>/environ` |

A objeção de "três implementações" da avaliação anterior **não se aplica a este
canal**, porque o canal é específico do Linux. Ela continua valendo para o
objetivo maior — confinar o que uma tool pode tocar em disco.

### Fail-open, explicitamente

`harden_current_process()` devolve `Hardening::{ProcfsClosed, NotApplicable,
Failed}` e o processo segue em qualquer caso, avisando no `stderr` quando
falha. Um kernel sem o knob não deve derrubar o gateway.

## Consequences

**Positivas**

- O canal nomeado no `#1084` item 1 está fechado, com teste que falha se o fix
  for removido (o controle não-endurecido prova que o canal existia).
- Sem core dump do processo que carrega segredos — desejável por si só.
- Nenhuma política de caminhos foi inventada às pressas.

**Negativas / limites — o que isto NÃO faz**

- **Não é sandbox.** Um filho de tool continua podendo ler qualquer arquivo que
  o usuário possa ler, inclusive um `.env` no disco. Segredo em arquivo não é
  coberto por nada aqui.
- Não cobre descritores já abertos, nem `ptrace` de forma comprovada: nesta
  máquina `yama.ptrace_scope=1` já bloqueia o anexo, então a medição do efeito
  do `dumpable` sobre `ptrace` ficou **inconclusiva** e não é reivindicada.
- `root` continua lendo tudo.

**O que fica em aberto (e continua sendo decisão do dono)**

Confinar o sistema de arquivos de uma tool — o objetivo original da Landlock.
Isso ainda precisa da política de caminhos, ainda vale para os três sistemas
operacionais, e ganha uma ADR própria quando for encarado. O que esta ADR
retira da fila é a confusão entre "fechar o canal procfs" e "construir um
sandbox": eram dois problemas, com custos muito diferentes.

## Nota sobre o `run_tests` (item 4 do #1078)

Registrado aqui porque foi decidido junto, embora não seja decisão
arquitetural.

O `run_tests` era **bloqueado incondicionalmente** sem canal de confirmação. A
regra não protegia nada: no mesmo runtime, `bash("cargo test")` roda, porque
`cargo test` não é comando sensível no gate. Era a mesma capacidade por outra
porta, com o custo de deixar a ferramenta inútil no caminho full-auto.

A regra passou a ser **a mesma do `bash`**, aplicada à linha de comando que
realmente vai rodar. Consequência medida: `cargo test` e `npm test` passam;
`pytest` **continua bloqueado**, porque roda por um interpretador Python, que o
gate trata como código arbitrário. Isso é a paridade funcionando.

Nenhuma capacidade nova é concedida, e por isso a decisão não precisou do
apetite de risco do dono — que era o motivo de o item estar parado.
