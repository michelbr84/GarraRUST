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

### FECHADO em 2026-09-09 (#1084, ADR 0019)

O canal do procfs esta fechado. `harden_current_process()`
(`garraia-common/src/process_hardening.rs`) roda `prctl(PR_SET_DUMPABLE, 0)`
no inicio do `main` da CLI, e o kernel passa a tratar `/proc/<pid>/environ`
deste processo como root-only. Um filho de tool de mesmo UID recebe `EACCES`.

Medido, com controle:

```text
sem hardening   filho le: SEGREDO_DO_PAI=abracadabra
com hardening   filho le: (negado)
```

**Nao foi Landlock**, e a ADR 0019 explica por que: Landlock nao tem regra de
negacao, entao "negar /proc" vira "enumerar todo o resto" — exatamente a
politica de caminhos que ninguem decidiu. E negar `/proc` inteiro quebraria
`cargo` (`current_exe()` le `/proc/self/exe`). Inverter a pergunta — tornar o
**pai** ilegivel em vez de restringir o filho — fecha o canal com uma syscall e
sem politica nenhuma.

O texto abaixo e a avaliacao original de 2026-04, mantida como registro do
raciocinio que levou ate aqui.

### O que continua aberto

Confinar o que uma tool pode **tocar em disco**. Um filho segue lendo qualquer
arquivo que o usuario possa ler, inclusive um `.env`. Isso e o objetivo
original da Landlock, ainda precisa da politica de caminhos, e continua sendo
decisao de produto.

### Avaliacao original (2026-04, #1078)

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

### RESOLVIDO em 2026-09-09 (#1084)

Nenhuma das tres opcoes acima: a pergunta estava mal posta.

O bloqueio incondicional nao protegia nada. No mesmo runtime sem canal,
`bash("cargo test")` roda — `cargo test` nao e comando sensivel no gate. Era a
mesma capacidade por outra porta, com o custo de deixar `run_tests` inutil no
full-auto.

A regra passou a ser **a mesma do `bash`**, aplicada a linha de comando que vai
rodar de verdade (`RunTestsTool::command_line`, montada a partir do `Command`,
para o gate nunca julgar algo menor do que executa). Consequencia medida:

| Suite | Sem canal de confirmacao |
| --- | --- |
| `cargo test` | roda |
| `npm test` | roda |
| `pytest` | **bloqueada** — roda por interpretador Python, sensivel no gate |
| filtro cuja linha toca os padroes ancorados (`/proc/…/environ`, `os.environ`, `import environ`) | **bloqueada** (ancorados desde o #1781: um filtro que so se chama `environ` roda, e nao e mais bloqueada por substring solto) |

Nenhuma capacidade nova e concedida, e por isso a decisao nao precisou do
apetite de risco do dono. Com canal de confirmacao nada mudou: toda suite
continua pedindo aprovacao, vinculada ao diretorio.

Junto veio um buraco que a leitura do arquivo revelou: `test_name` vinha do
modelo e ia cru como argumento do runner, e `cargo test --config
'target.<cfg>.runner=...'` e execucao arbitraria. Agora e validado
(`validate_test_name`), com a forma documentada `-p <crate>` como unica
excecao.

### Avaliacao original (2026-04, #1078)

Nao e decisao tecnica: e quanto risco o produto aceita. Fica com o dono.

## 7. Gate de comandos: familia .NET/PowerShell e o alias `import os as o` (#1791)

O #1786 ancorou o `environ` (`/environ`, `os.environ`, `import environ`) e
deixou anotado no fonte um residuo sem ancora barata: o alias
`import os as o; o.environ`. A revisao de seguranca daquele PR tambem mapeou
os vizinhos .NET/PowerShell que o gate nao cobria. Os dois itens estao
decididos aqui.

### Item 12 — o alias fica como residuo ACEITO, sem ancora textual

**Decisao:** nenhuma ancora para `import os as X` + `X.environ`.

**Por que:**

1. O texto so e perigoso quando **executa**, e so executa passando por um
   interpretador. O gate trata interpretador pelo **programa**, nao pelo
   texto: `python3 -c '...'` (qualquer codigo), script (`python3 x.py`,
   `-m modulo`), redirecionamento (`python3 < x.py`) e, desde esta issue,
   pipe (`echo '...' | python3`) exigem confirmacao. Em todas essas portas o
   alias ja e pego — junto com qualquer outra grafia.
2. Fora dessas portas o texto e inerte: `echo`, mensagem de commit, `grep`.
   Uma ancora para o alias so dispararia ai, ou seja, so acrescentaria falso
   positivo — e em modo fail-closed falso positivo e BLOCK — sem pegar
   nenhuma execucao a mais.
3. Ancora textual nao fecha a classe: `__import__('os').environ`,
   `getattr(os, 'environ')`, `vars(os)['environ']`, `os.getenv('X')`. A
   regra de programa nao depende da grafia.

**O que a analise achou e esta issue fechou.** O argumento 1 so vale se
TODA porta de execucao passar por regra de programa. Tres nao passavam, e o
codigo chegava ao interpretador sem confirmacao nenhuma:

| Porta | Antes | Agora |
| --- | --- | --- |
| codigo pelo stdin: `echo '...' \| python3`, `Get-Content x.py \| python` | livre | confirmacao (`pipe into interpreter`, mesmo canal do `\| sh`) |
| versao no nome: `python3.12 -c`, `/usr/bin/python3.11 x.py`, `perl5.36.0 -e`; e `py -3 -c` (lancador do Windows), `pypy3`, `nodejs` | livre | o tratamento do nome sem versao (`code_interpreter`) |
| atribuicao na frente: `X=1 python3 -c '...'` | livre | o tratamento sem a atribuicao |

**Residuo que continua, conhecido:** as ancoras textuais que ja existiam
(`os.environ`, `import environ`, `/environ`) seguem casando tambem em texto
inerte (`git commit -m "remove os.environ"` pede confirmacao) — residuo desde
o #1075, e exatamente o custo que este item se recusa a ampliar.
Interpretadores de shell do Windows com codigo inline (`powershell -c`,
`cmd /c`) seguem sem desembrulho, como anotado no #1798.

### Item 10 — o nivel de cada forma nova

Cada forma nova entra no **mesmo nivel** da forma equivalente que o gate ja
tratava. Nenhuma vira bloqueio duro: o DENY_LIST e para o destrutivo
(`rm -rf /`, `format c:`), e leitura de ambiente e download sao tier de
confirmacao desde o #1075.

| Forma | Equivalente ja tratado | Nivel | Como casa |
| --- | --- | --- | --- |
| `[Environment]::GetEnvironmentVariable(s)` | `os.environ` | confirmacao | `::getenvironmentvariable` — o `::` e nao o nome solto: a busca pela API em codigo C# usa `.` |
| `reg query HKCU\Environment`, `Get-ItemProperty HKCU:\Environment` | `/etc/environment` (`/environ`) | confirmacao | `\environ`, gemeo Windows do `/environ` |
| `Get-ChildItem Env:`, `gci env:*KEY*` | `printenv` | confirmacao | programa de listagem + drive `env:` inteiro ou com curinga |
| `$env:PATH`, `Get-Item env:PATH` | `echo $HOME` | **livre** | — o ambiente do filho ja passou pelo scrub R3 |
| `WebClient.DownloadString/File/Data`, `.Upload*` | `iwr`, `curl` | confirmacao | a chamada, nao o tipo — criar o objeto nao move byte, e `rg WebClient` nao pode virar BLOCK |
| `Start-BitsTransfer`, `bitsadmin` | `curl` | confirmacao | programa (cmdlet e CLI juntos, como no #1781) |
| `Start-Process X ...` | `nohup X ...` | **o de X** | wrapper; os argumentos do `-ArgumentList` voltam ao gate sem as aspas. Sem programa reconhecivel depois dele, confirmacao |
| `[Diagnostics.Process]::Start(...)` | — | confirmacao | falha fechado: os argumentos vem dentro de uma chamada .NET que o gate nao parseia |
| `$r = iwr ...`, `FOO=1 curl ...` | `iwr`, `curl` | o do programa | a atribuicao na frente e pulada |
| `kubectl exec pod -- env` (item 11) | `docker exec` | confirmacao | subcomando, desde o #1798; a paridade em todas as formas (wrapper, `.exe`, `-c`, atribuicao, `Start-Process`) ficou fixada em teste |

Por que `GetEnvironmentVariable` no singular e gated e `$env:X` nao: com o
segundo argumento (`'User'`/`'Machine'`) ele le o ambiente **persistente**,
gravado no registro (onde `setx OPENAI_API_KEY ...` deixa a chave), que o
scrub R3 do processo filho nao alcanca. O alvo pode vir de variavel, enum ou
inteiro, entao nao da para ancorar so a forma com alvo; o metodo inteiro vai
para confirmacao, como o `os.environ` inteiro.

### Fora do escopo, encontrado e nao tocado

Formas que escondem o programa de toda regra por programa, e nao so das
novas — merecem issue propria:

- **Bloco/palavra-chave na frente do programa:** `{ curl x; }`,
  `if ...; then curl x; fi`, `for ...; do curl x; done` (bash) e os
  scriptblocks do PowerShell (`& { iwr x }`, `1 | % { iwr x }`,
  `Start-Job { iwr x }`). O programa do segmento vira `{`/`then`/`do`/`%`.
- **Metacaractere entre aspas na atribuicao:** `X="a;b" curl x` — o split
  legado parte dentro das aspas (o mesmo limite documentado no #1078).
- **Vizinhos de download fora da familia pedida:** `certutil -urlcache`,
  `[Net.Http.HttpClient]`/`[Net.WebRequest]`.
- **Indirecao por variavel:** `$m='GetEnvironmentVariable'; [Environment]::$m('X')`
  — nenhuma ancora textual pega; mesma classe do alias do item 12.
- `kubectl debug`/`kubectl attach` (ja anotados no #1798) e `docker cp`.
