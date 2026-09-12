---
name: max-power
description: "Ativação em um comando do harness GarraIA SuperPowers (ClaudeMaxPower): verifica os markers de instalação, repara o harness se algo falta, oferece o plugin oficial Superpowers, valida o setup, apresenta o menu de capacidades e roteia para a skill certa para o objetivo imediato."
disable-model-invocation: true
arguments:
  - name: goal
    description: "O que você quer realizar agora (texto livre; a skill escolhe o melhor ponto de entrada)"
    required: false
  - name: mode
    description: "new-project | existing-project | auto (default: auto)"
    required: false
  - name: install-superpowers-plugin
    description: "yes | no | ask (default: ask)"
    required: false
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Glob
  - Grep
  - Agent
---

# Skill: max-power

Ativação em um comando do harness **GarraIA SuperPowers** — a adaptação local do
[ClaudeMaxPower](https://github.com/michelbr84/ClaudeMaxPower) para o GarraRUST. Detecta o
ambiente, verifica o que falta, oferece o plugin oficial Superpowers, ativa o pipeline de
skills e roteia para o ponto de entrada certo para o objetivo imediato.

Este é o primeiro comando pretendido em qualquer shell novo. Se `/max-power` rodar limpo,
o pipeline completo (brainstorming → plano → dev com subagents → review → finish) está
vivo e todos os hooks de governança estão ativos.

## Announce

Imprima exatamente:

```
Ativando GarraIA SuperPowers (ClaudeMaxPower) em capacidade máxima.
```

Depois siga os passos abaixo. Não pule passos. Se um passo falhar, reporte a falha com
clareza e continue no próximo passo onde for possível.

## Passo 1 — Detectar ambiente

Rode cada verificação e anote o resultado. Prefira comandos shell não-interativos.

### 1.1 Repositório git?

```bash
git rev-parse --is-inside-work-tree 2>/dev/null
```

Anote `IS_GIT=yes|no`. Se não for, avise o usuário — muitas skills (worktrees, branch
finishing, fix-issue, review-pr) exigem git.

### 1.2 Harness já instalado?

Verificação inline dos três markers de instalação do fork (não use os scripts
`skills/references/*.sh` do upstream — este fork não os tem e não os usa):

```bash
test -f .claude/hooks/session-start.sh && echo MARKER_HOOK_OK
test -f skills/assemble-team.md && echo MARKER_SKILL_OK
grep -q "ClaudeMaxPower" CLAUDE.md 2>/dev/null && echo MARKER_CLAUDE_OK
```

Os três presentes → `GARRA_INSTALLED=yes`. Algum faltando → `no` (Passo 2).

### 1.3 Novo ou projeto existente?

O GarraRUST é um projeto existente por construção — a fonte do harness é versionada
neste próprio repo. Use `existing`, salvo se o argumento `mode=new-project` for passado;
nesse caso, avise que o GarraRUST é projeto existente e siga como `existing` mesmo assim.

### 1.4 Detectar stack

Probes inline (as mesmas superfícies que `/assemble-team` valida):

```bash
test -f Cargo.toml && echo rust
test -d apps/garraia-mobile && echo flutter
test -d crates/garraia-desktop && echo tauri
```

Anotado como `rust,flutter,tauri` — usado depois para validar ferramentas e test runners.

## Passo 2 — Reparar o harness (só se algo faltar)

Pule este passo inteiramente se `GARRA_INSTALLED=yes`.

Se algum marker falhou, a fonte de verdade é **este próprio repositório**, não o
upstream: `skills/*.md` (fonte versionada das skills), `.claude/hooks/`,
`.claude/agents/`, `.claude/settings.json` e `CLAUDE.md` são todos versionados no git.

- Liste exatamente quais markers falharam.
- Proponha o reparo via git: `git status` para ver o estado e `git checkout -- <path>`
  (ou reverter a edição do `CLAUDE.md`) para restaurar o que faltou.
- **Nunca** copie arquivos do upstream `michelbr84/ClaudeMaxPower` por cima do fork.
  O fork tem customizações que seriam destruídas: agents com modelos por função
  (`.claude/agents/*.md`), skills em PT-BR com frontmatter `triggers`/`dependencies`,
  hooks GarraIA (o `session-start` sincroniza `skills/*.md` →
  `.claude/skills/<name>/SKILL.md`) e o `CLAUDE.md` do projeto. rsync do upstream aqui
  é destrutivo, não é instalação.
- Usuário recusa o reparo → pare limpo. Não deixe estado parcial.

## Passo 3 — Oferecer o plugin Superpowers (opcional)

Verifique o estado atual (read-only):

```bash
jq -r '.plugins | keys[]' ~/.claude/plugins/installed_plugins.json 2>/dev/null \
  | grep -F 'superpowers@claude-plugins-official'
```

Se já estiver instalado, confirme no dashboard e siga silencioso. Se não, explique:

```
O GarraIA SuperPowers roteia a metodologia Superpowers pelo plugin oficial —
brainstorming, writing-plans, subagent-driven-development, TDD, systematic-debugging,
worktrees e branch finishing ficam sob o namespace /superpowers:* com o plugin
instalado. Sem o plugin, a ponte local é /superpowers-bridge (mapeamento skills locais
↔ Superpowers) e o contexto do projeto para o plugin é .claude/superpowers-config.md.

O plugin é opcional mas recomendado — o pipeline unificado espera ele.
```

Lógica de decisão:

- `install-superpowers-plugin=yes`: diga ao usuário para rodar o comando ele mesmo
  (esta skill não pode executar slash commands). Texto exato para copiar:

  ```
  /plugin install superpowers@claude-plugins-official
  ```

  Alternativa no shell (CLI; aplica-se a partir da próxima sessão):

  ```
  claude plugin install superpowers@claude-plugins-official
  ```

- `install-superpowers-plugin=no`: siga silencioso.
- `ask` ou não passado: pergunte em uma linha. Default: não.

## Passo 4 — Verificação de setup

Este fork não tem o `scripts/setup.sh` do upstream — não rode nada cegamente. Valide as
ferramentas diretamente (avise e continue — não falhe duro):

- `cargo --version` faltando → o workspace inteiro fica inutilizável (essencial)
- `gh --version` faltando → `/fix-issue` e `/review-pr` não conversam com o GitHub
- `gh auth status` falhando → mesmas skills degradam; imprimir `gh auth login`
- `jq` faltando → higiene de changelog e checks de JSON degradam
- `python3` faltando → `scripts/changelog/assemble.py --check` e o quality ratchet
  (`scripts/quality/`) ficam indisponíveis
- `flutter --version` falhando → opcional: o APK sai do CI (`mobile.yml`), este
  container pode não ter o Android SDK
- `.env` ausente ou com placeholders → aviso (o `session-start` já alerta; confirme aqui)

Anote as ferramentas faltando no dashboard (Passo 7) sob Ferramentas.

## Passo 5 — Ativar pipeline de skills

Carregue o contexto do projeto para as skills downstream terem tudo em mãos.

- Leia `CLAUDE.md` — identidade, convenções, regras absolutas.
- Leia `TODO.md` (backlog operacional).
- Leia `Cargo.toml` — workspace e contagem de crates ativas.
- Rode `git status --short` e `git log --oneline -n 10` para entender o trabalho ativo.
- Se `.garra-estado.md` existir, leia — o stop hook escreve o resumo da sessão anterior
  lá. (Não é `.estado.md`, nome do upstream — este fork usa `.garra-estado.md`.)

Mantenha o resumo em working memory para o dashboard do Passo 7. Não imprima conteúdo
cru dos arquivos a menos que o usuário peça.

## Passo 6 — Roteamento para o objetivo imediato

### 6.1 Se um argumento `goal` foi passado, classifique

Tabela determinística — a primeira linha que casar com o goal (substring,
case-insensitive):

| Goal casa com | Ponto de entrada |
| --- | --- |
| issue `<N>`, "corrige issue", "fix issue" | `/fix-issue --issue <N>` |
| PR `<N>`, "revisa PR", "review pr" | `/review-pr --pr <N>` |
| feature nova, ideia, proposta, "desenha" | `/assemble-team` (ou `/superpowers:brainstorming` com plugin) |
| bug complexo, debug, causa raiz | `/superpowers:systematic-debugging` com plugin; sem plugin, `/fix-issue` |
| refatorar, "refactor", módulo | `/refactor-module --file <path> --goal "..."` |
| testes, TDD, "teste de regressão" | `/tdd-loop` |
| docs, documentação, README, CHANGELOG | `/generate-docs` |
| commit, segredos, "valida antes de commitar" | `/pre-commit` |
| git, branch, merge, workflow de PR | `/git-assist` |
| revisão inline de código, "revisa este diff" | `/code-review` |
| traduzir, "translate" | `/translate` |
| explicar comando, shell | `/shell-explain` |
| resumir, "summarize", sessão | `/summarize` |
| pesquisar na web, lookup | `/web-lookup` |
| CLI `garra`/`garraia`, "garra update", instalador | `/garra-cli-operator` |
| varredura, triagem de issues/PRs, autopilot | `/repo-autopilot` |
| superpowers, plugin, mapeamento de skills | `/superpowers-bridge` |
| rotina agendada | `/garra-routine` |
| qualidade, métricas, ratchet | `/quality-babysit` |
| risco R4/R5, release, secrets, destrutivo | **escalar ao humano** (regra do CLAUDE.md) — pergunte ao usuário antes de qualquer ação |

- Match único → recomende o comando exato, prefixado da justificativa. **Não execute a
  skill downstream você mesmo.**
- Múltiplos matches → liste os candidatos e pergunte qual o usuário quis (uma única
  vez), depois roteie.
- Nenhum match → siga para 6.2 (menu).

Exemplo de saída com match único:

```
Rota: /fix-issue --issue 1153
Por quê: o goal pede correção de issue — fix-issue é o caminho TDD para isso.
```

### 6.2 Sem `goal`, mostre o menu

Imprima este menu:

```
Claude Code + GarraIA SuperPowers ativos em capacidade máxima.

Pipeline recomendado (metodologia Superpowers, com o plugin instalado):
  1) /superpowers:brainstorming <tópico>                Desenha a feature (gate de spec)
  2) /superpowers:writing-plans <spec-file>             Quebra a spec em tarefas
  3) /superpowers:subagent-driven-development <plano>   Executa via subagents + review 2 estágios
  4) /superpowers:finishing-a-development-branch        Merge / PR / cleanup

Pontos de entrada nativos do GarraIA SuperPowers:
  /assemble-team                                        Equipe de agentes por risco R0-R5
  /fix-issue --issue <N>                                Corrige issue via TDD
  /review-pr --pr <N>                                   Revisão de PR (reviewer + security)
  /refactor-module --file <path> --goal "..."           Refactoring seguro com testes
  /code-review                                          Revisão inline de código
  /tdd-loop                                             Red-Green-Refactor
  /pre-commit                                           Validação pré-commit (segredos, debug, lint)
  /generate-docs                                        Documentação automática
  /git-assist                                           Workflow git
  /repo-autopilot                                       Varredura autônoma de issues/PRs
  /translate · /shell-explain · /summarize · /web-lookup · /garra-cli-operator   Utilitários
  /superpowers-bridge                                   Mapeamento skills locais ↔ Superpowers

Comandos de projeto (não são skills): /garra-routine · /quality-babysit
Agente dirigidor de PR (lê sozinho, sem slash): steward

Hooks de governança (disparam sozinhos, sem invocação):
  - session-start hook       contexto + estado anterior + sync skills/*.md → .claude/skills/
  - pre-tool-use hook        bloqueia comandos perigosos, audit log
  - post-tool-use hook       testes automáticos em edições
  - stop hook                persiste .garra-estado.md

Qual é o seu objetivo? (texto livre ou número acima)
```

## Passo 7 — Dashboard de status

Renderize o template abaixo substituindo os valores detectados nos Passos 1–6.

```
════════════════════════════════════════════════════════════════
 GarraIA SuperPowers — status
════════════════════════════════════════════════════════════════
 Projeto:             GarraRUST (GarraIA) — branch <branch>
 Tipo:                existing
 Stack:               Rust (Axum 0.8, <N> crates) · Flutter · Tauri v2
 Harness:             <instalado | faltam: <markers>>
 Plugin Superpowers:  <instalado | NÃO instalado → /plugin install superpowers@claude-plugins-official>
 Ferramentas:         cargo <✓/✗> · gh <✓/✗ auth> · jq <✓/✗> · python3 <✓/✗> · flutter <✓/✗>
 Estado:              .garra-estado.md <presente | ausente>
 Skills:              <N> (sync skills/*.md → .claude/skills/)
 Próxima ação:        <rota do 6.1 | pergunta do 6.2>
════════════════════════════════════════════════════════════════
```

## Resumo de tratamento de erro

- Marker faltando → Passo 2 (reparo via git; nunca rsync do upstream).
- Ferramenta faltando → avise, continue, anote no dashboard sob Ferramentas.
- `goal` ambíguo (casa múltiplos intents) → pergunte uma vez, depois roteie.
- Usuário recusa reparo ou plugin → pare limpo / siga sem o plugin. Não deixe estado
  parcial.
- Banner `🦀 GarraIA SuperPowers` não apareceu nesta sessão → o session-start hook não
  rodou; verifique `.claude/settings.json` e, para o sync das skills, rode manualmente
  `bash .claude/hooks/session-start.sh`.

## Cross-references

- Regras do projeto: [`CLAUDE.md`](../CLAUDE.md)
- Contexto do projeto para o plugin Superpowers: [`.claude/superpowers-config.md`](../.claude/superpowers-config.md)
- Ponte local ↔ Superpowers: [`skills/superpowers-bridge.md`](superpowers-bridge.md)
- Mecânica de sync das skills: [`.claude/hooks/session-start.sh`](../.claude/hooks/session-start.sh)
- Agents do time: [`.claude/agents/`](../.claude/agents/)

## Critérios de sucesso

`/max-power` teve sucesso quando todos os itens abaixo forem verdade:

- Skills versionadas presentes (`skills/*.md`) e geradas em `.claude/skills/<name>/SKILL.md`
- `.claude/settings.json` tem `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` (agent teams vivos)
- O session-start hook rodou nesta sessão (banner 🦀 apareceu)
- O usuário tem uma ação clara — rota de skill invocável ou o menu
- O dashboard de status foi impresso

Se algum critério falhar, reporte qual falhou e o que fazer a respeito.

**Feedback:** `/max-power` te deixou no próximo passo certo? Responda com nota 1–10, o
que te atrasou, ou um caminho mais rápido de onde você começou para onde você chegou.
