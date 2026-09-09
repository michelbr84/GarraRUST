# Decisões de Hardening e Postura de Segurança

Este documento consolida as decisões arquiteturais de segurança e mitigação de riscos no `michelbr84/GarraRUST`.

## 1. Zero Tolerância a Vulnerabilidades Conhecidas

- O repositório mantém **zero alertas abertos** no GitHub Dependabot e zero vulnerabilidades ativas no `cargo audit`.
- Atualizações de segurança são priorizadas como caminho crítico.
- Quando uma vulnerabilidade em dependência transitiva não possui correção imediata viável (ex.: dependência upstream não atualizada), a supressão temporária exige:
  - Justificativa técnica formal documentada em `.cargo/audit.toml` e `deny.toml`.
  - Issue de tracking aberta no repositório com prazo de expiração (máximo 90 dias).
  - Análise de impacto comprovando que o caminho de código vulnerável não é alcançável em tempo de execução.

## 2. Proteção de Segredos e Push Protection

- GitHub Secret Scanning e Push Protection estão habilitados.
- Nenhuma credencial, token ou chave privada deve ser commitada no histórico ou arquivos de configuração.
- Scripts de varredura pré-commit e validações CI utilizam ferramentas especializadas (Gitleaks) e sanitização em tempo de build.

## 3. Isolamento Multi-Tenant e RLS (Row-Level Security)

- Toda persistência multi-tenant utiliza PostgreSQL 16 com políticas estritas de Row-Level Security (`FORCE RLS`).
- As transações de handlers autenticados executam obrigatoriamente `set_config('app.current_user_id', ...)` e `set_config('app.current_group_id', ...)`.
- As suítes de teste de autorização (`Security Gate (BOLA)`) exercitam isolamento entre grupos e verificação de não-vazamento de existência em rotas REST (retornando 404 em acessos não autorizados).

## 4. Proteção de Branches e Rulesets

- A branch `main` é protegida pelo Ruleset `15901595` com verificação estrita de checks de CI (Format Check, Clippy Linting, Tests Ubuntu/Windows, cargo-deny, Secret Scan).
- Merges diretos em `main` sem validação de CI são bloqueados por padrão.

## 5. Sandbox das tools: por que ainda e "seatbelt, not sandbox"

O #1075 fechou a heranca de ambiente (`R3_ENV_ALLOWLIST`: um processo filho de
tool herda `PATH`, `HOME`, `LANG`, `LC_ALL`, `TERM` e `USER`, e mais nada). O
#1078 fechou os fake-negativos do gate de comandos e vinculou a aprovacao
humana ao comando aprovado.

O que **nao** esta fechado: um processo filho de mesmo UID pode ler
`/proc/<ppid>/environ` e alcancar os segredos do processo pai. O gate cobre o
padrao `environ` no `CONFIRM_LIST`, mas isso so vale para comandos que passam
pelo gate — nao para codigo dentro de um script que o gate agora obriga a
confirmar, mas nao le.

Fechar isso de verdade exige confinamento do processo, e nao mais uma regra de
texto.

### Avaliacao (feita no #1078, sem implementacao)

**Landlock** e a opcao mais promissora para Linux: controle de acesso a
sistema de arquivos aplicado pelo proprio processo, sem daemon, sem root e sem
privilegio especial. Negar leitura sob `/proc` fecharia o canal procfs. Duas
limitacoes conhecidas que a implementacao tem de tratar: ela nao alcanca
descritores ja abertos antes de a regra entrar em vigor, e nao cobre `ptrace`
(que depende do `yama.ptrace_scope` do host). A versao minima de kernel e o
conjunto de ABIs disponiveis precisam ser confirmados no momento da
implementacao, porque a cobertura de rede so entrou em ABIs mais recentes.

**Por que nao entrou junto com o #1078**, apesar de ser o item de maior valor
da lista:

1. **Sao tres implementacoes, nao uma.** O projeto roda em Linux, macOS (canal
   iMessage) e Windows (desktop Tauri, MSI/NSIS). Landlock e Linux. macOS
   precisaria de `sandbox-exec`, Windows de AppContainer ou job object. Um
   sandbox que so existe num dos tres da uma sensacao de protecao que a
   documentacao teria de desmentir em cada pagina.
2. **A politica e decisao de produto, nao detalhe de implementacao.** Quais
   caminhos o agente pode ler e escrever? O repositorio inteiro? Só o
   `working_dir` da sessao? O cache do cargo? Cada resposta muda o que o
   produto consegue fazer, e uma resposta errada quebra o uso legitimo em vez
   de proteger.
3. **E decisao arquitetural irreversivel**, e a regra 8 do `CLAUDE.md` pede
   ADR antes. Escolher o mecanismo de confinamento amarra o projeto a ele.

Ate la, o bash do `garra_agent` continua sendo **seatbelt, e nao sandbox**: o
gate reduz a superficie e pede confirmacao, mas nao confina o processo.

## 6. `run_tests` fail-closed: decisao de produto em aberto

Depois do #1075, `run_tests` sem canal de confirmacao e **BLOQUEADO**, e nao
executado em silencio. `npm test` e `cargo test` rodam o que o projeto mandar
(scripts de `package.json`, build scripts), o que e codigo arbitrario — a
mesma regra do bash.

O custo e real: no caminho MCP full-auto, que nao tem canal de confirmacao, o
`run_tests` simplesmente nao roda. O fluxo benigno continua disponivel via
`bash` (`cargo test` e `npm test` nao sao comandos sensiveis no
`safety_gate`), o que e uma inconsistencia conhecida e nao um descuido: o
`bash` esta atras do mesmo gate, e quem chama assume o que o gate deixa
passar.

As alternativas, para quando o dono decidir:

| Opcao | O que muda | O que custa |
| --- | --- | --- |
| Manter fail-closed | nada | `run_tests` inutil no caminho full-auto |
| Sandbox (secao 5) | resolve de graca: o runner roda confinado | depende do sandbox existir |
| Permitir com env scrubbed + rede negada | `run_tests` volta a funcionar | assume o risco de codigo do projeto rodar sem aprovacao |

Nao e decisao tecnica: e quanto risco o produto aceita. Fica com o dono.
