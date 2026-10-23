- **Runtime Node.js privado + ponte do WhatsApp empacotados e verificados (#1636, fatia B).**
  Novo `scripts/desktop/node-runtime/build-node-runtime.sh`: dado
  `--platform win-x64|linux-x64|darwin-arm64 --out <dir>`, baixa o artefato
  OFICIAL do Node.js (versao fixada em `node-version.txt`, hoje v24.22.1 LTS
  Krypton), verifica a PROCEDENCIA por GPG contra as chaves do release team
  vendorizadas no repo (`keys/*.asc` + allowlist `keys/FINGERPRINTS.txt`, num
  GNUPGHOME isolado, exigindo que a chave primaria que assinou esteja na lista)
  e a INTEGRIDADE por SHA-256 (nome de arquivo inteiro), extrai so o runtime
  (`node`/`node.exe` + `LICENSE`), monta a ponte com
  `npm ci --omit=dev --no-audit --no-fund` usando o Node e o npm do PROPRIO
  tarball (sem cross-install), gera `THIRD_PARTY_NOTICES.txt` (Node + todos os
  pacotes de `node_modules`, falhando se algum nao tiver licenca identificavel)
  e grava `manifest.json` (versao, plataforma, sha256, fingerprint do
  assinante, estado do lock da ponte). Layout combinado com o resolver Rust:
  `node/node.exe` no Windows, `node/bin/node` no Unix. Novo workflow
  `.github/workflows/node-runtime.yml` (matriz das 3 plataformas em
  `pull_request`, `workflow_dispatch` e `workflow_call` para a fatia C reusar):
  monta o bundle num caminho com espaco, esconde o Node global do runner e
  prova que o bundle roda sozinho (carrega baileys+pino+sharp com o Node
  privado, sem `NODE_PATH`), publica cada bundle como artifact de retencao
  curta e roda um teste negativo que prova a recusa de artefato ou SHASUMS
  adulterado. Nao mexe em `bridge.mjs` nem no codigo Rust da ponte. Refs #1636.
