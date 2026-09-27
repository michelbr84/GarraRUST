- **`scripts/dogfood/linux-clean-install.sh` automatiza o dogfood Linux
  (D1/D9 da matriz de release) num container limpo (#1426, #1439).**
  A v0.4.4 e a v0.4.5 sairam com CI verde e quebraram no caminho real de
  quem instala; o CI prova que o codigo compila, nao que o PACOTE funciona
  para uma pessoa. O script obtem um `.deb` x86_64 da CLI (artefato
  `garraia-linux-packages` de um run do `release.yml` via `gh run
  download`, ou build local empacotado com o mesmo `nfpm` pinado e o mesmo
  `packaging/nfpm.yaml` da release), sobe um `ubuntu:24.04` cru com
  `--network host`, instala com `apt-get install ./garraia.deb` e prova o
  fluxo inteiro: `garraia --version` (e o alias `garra`), `doctor --json`
  (exit 0, JSON valido), `doctor whatsapp --json` (exit 69 sem WhatsApp,
  linha `whatsapp.linked` presente), `config set-model` apontando para o
  Ollama do host sem chave paga, `start -d` na 3899 ate `/api/health`
  `healthy`, sessao REST com resposta real do modelo, `restart -d` com
  sessao e config sobrevivendo (historico readotado do `sessions.db`,
  provider/modelo no health do processo novo) e `stop` com porta fechada
  e pid file removido. O curl/jq ficam do lado do host, entao o container
  nao ganha nenhuma ferramenta que um usuario nao teria. A evidencia
  (logs, JSONs, resposta do modelo, `.deb` testado, `resumo.txt` e
  `resumo.json` PASSOU/FALHOU por passo) vai para `dogfood/linux/<data>/`,
  gitignored. `docs/releasing.md` §1.5 registra o comando na matriz.
