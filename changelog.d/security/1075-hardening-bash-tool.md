- **Hardening do BashTool e do safety_gate: tier de risco fail-closed e
  isolamento de ambiente (#1075).** O tier de risco do bash deixa de
  auto-executar quando nao ha canal de confirmacao: com
  `tool_confirmation_enabled: false` (MCP `garra_agent`, heartbeats), um
  comando sensivel e BLOQUEADO; com confirmacao habilitada (gateway/
  Telegram), continua pedindo aprovacao como antes. Em modo fail-closed o
  flag `is_confirmation_approved` e ignorado: ele vem do historico da
  conversa (marcador `[CONFIRM_REQUIRED]`) e pode ser contaminado pela
  saida do modelo. `run_tests` segue a mesma regra — sem canal de
  confirmacao, e bloqueado (npm/cargo scripts sao codigo arbitrario).
  O `is_risky` ganha normalizacao de whitespace (mata o bypass
  `rm  -rf` por substring), deteccao program-aware POR SEGMENTO de
  metacaracteres com resolucao de wrappers (`sudo curl`, `env VAR=x cargo
  test`, `timeout 10 curl`; subcomandos mutantes de git/systemctl/docker/
  kubectl/helm/terraform/npm/cargo, `deploy`), programas exfiltraveis
  (`curl`, `wget`, `ssh`, `env`, `printenv`, ...), interpolacao de env
  (`$(env)`, backticks), pipe para shell sem espaco (`curl x|bash`),
  leitura de procfs (`/proc/*/environ`), `rm` destrutivo token-aware
  (`rm -fr /`, `-f -r`, `--recursive`), `find -delete`, `dd of=`,
  desembrulho recursivo de `sh -c '...'` e gating de
  `python3 -c`/`perl -e`; consequencia visivel: TODO `git push` e
  `curl`/`wget` agora passam pelo tier de confirmacao. No unix, os filhos
  de bash, git_diff, code_review e repo_search herdam apenas
  `PATH/HOME/LANG/LC_ALL/TERM/USER` do processo pai (a politica vive em
  `garraia-common::safety_gate::allowed_child_env`), o bash roda no
  `working_dir` da sessao via `current_dir` e o `git diff` usa
  `--no-ext-diff` contra `.git/config` plantado — o canal de heranca de
  env para segredos do pai esta fechado; leitura direta de
  `/proc/<pid>/environ` por mesmo UID passa pelo tier de risco (padrao
  `environ`), mas so um sandbox real a fecha por completo (follow-up).
  No Windows o scrub fica desligado (PowerShell precisa do proprio
  ambiente).
