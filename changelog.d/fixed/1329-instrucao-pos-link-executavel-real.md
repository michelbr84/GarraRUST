- **A CLI nomeia o executavel que esta na maquina, nao o alias (#1329).** A
  instrucao pos-link do `whatsapp` mandava rodar `garra start` como literal, e
  quem so tem o `garraia` (Docker, `cargo install`, `install.sh` sem o alias)
  recebia um comando inexistente. Novo helper `binario::nome()` resolve
  `garra`/`garraia` a partir do proprio executavel (qualquer outro nome cai em
  `garraia`), e `whatsapp`, `desktop`, `logs` e o `about` passam a usa-lo. Um
  teste varre `whatsapp.rs` e proibe o literal voltar.
- **`garraia whatsapp status` mostra o perfil de execucao (ADR 0024, #1329).**
  Depois do bloco "Vinculado", uma linha bilingue diz o perfil (`standard` |
  `isolated-pod`), a origem (`default` | `file` | `env`), o piso que um dono
  recebe em conversa 1:1 (`default_mode` explicito, senao `search` em
  `standard` e `code` em `isolated-pod`) e a contagem de `owners` — nunca as
  identidades. Sem config carregavel a linha nao aparece e o exit code nao muda.
