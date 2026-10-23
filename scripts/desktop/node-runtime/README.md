# `scripts/desktop/node-runtime` — runtime Node.js privado + ponte do WhatsApp

Monta, de forma reprodutivel e verificavel, o runtime **Node.js privado** que o
instalador do GarraIA Desktop empacota, junto com o `node_modules`
pre-construido da ponte do WhatsApp (`bridge/whatsapp`). Esta e a **fatia B** da
issue #1636 (decisao do dono 2026-10-22): o script + o job de CI que produzem e
validam o bundle. Quem consome o bundle no `tauri.conf.json` e a fatia C; o
resolver Rust e o `bridge.mjs --self-test` sao a fatia A.

Por que empacotar o Node: o usuario final nao deve precisar instalar Node, e a
ponte depende de `sharp` (binarios nativos por plataforma). O bundle carrega
**tudo o que a ponte precisa em runtime** — so `node bridge.mjs`, sem npm.

## O que o script garante

`build-node-runtime.sh --platform <p> --out <dir>` faz, nesta ordem:

| passo | garantia |
|---|---|
| a | baixa o artefato **oficial** de `https://nodejs.org/dist/v<versao>/` |
| b | **procedencia**: `gpg --verify` do `SHASUMS256.txt.sig` num keyring isolado (GNUPGHOME temporario) montado **so com as chaves allowlistadas** (`keys/<FPR>.asc`), exigindo `GOODSIG` e **recusando** chave expirada/revogada ou assinatura ruim (`EXPKEYSIG`/`REVKEYSIG`/`EXPSIG`/`BADSIG`/`ERRSIG`/`NO_PUBKEY` — expirada/revogada emitem `VALIDSIG` e saem 0!), e so entao conferindo a chave **primaria** contra `keys/FINGERPRINTS.txt`. A decisao vive em `lib/check-gpg-status.sh` (testavel sem chave privada). |
| c | **integridade**: SHA-256 do artefato contra a linha exata do `SHASUMS256.txt` (match de nome inteiro, nao regex) |
| d | extrai so o runtime: binario `node`/`node.exe` + `LICENSE` |
| e | copia `bridge/whatsapp` (sem `node_modules`) e roda `npm ci --omit=dev --ignore-scripts --no-audit --no-fund` com o **Node e o npm do mesmo tarball oficial** (nunca o do runner). `--ignore-scripts` endurece a supply chain: nenhum install-script de dependencia roda — o `sharp` 0.34 usa binarios pre-compilados `@img/sharp-*` (deps opcionais), entao a ponte carrega normalmente mesmo assim (provado no CI por `prove-standalone.sh`). |
| f | gera `THIRD_PARTY_NOTICES.txt` (licenca do Node + de todos os pacotes de `node_modules`); **falha** se algum pacote nao tiver licenca identificavel |
| g | grava `manifest.json` (versao, plataforma, sha256, fingerprint do assinante, estado do lock) |

Plataformas suportadas (espelham `desktop.yml`/`release.yml`):
`win-x64` (Windows x86_64), `linux-x64` (Linux x86_64 glibc, baseline
ubuntu-22.04 / glibc 2.35) e `darwin-arm64` (macOS arm64).

## Contrato de layout do bundle (combinado com a fatia A)

```
<out>/
  node/
    node.exe            # Windows
    bin/node            # Linux e macOS
    LICENSE             # licenca do Node.js
  bridge-whatsapp/      # copia de bridge/whatsapp + node_modules (npm ci)
    bridge.mjs
    package.json
    package-lock.json
    node_modules/
      .package-lock.json
  THIRD_PARTY_NOTICES.txt
  manifest.json
```

O binario `node` fica em `node/node.exe` (Windows) ou `node/bin/node` (Unix).
**Nao mude esse layout sem alinhar com a fatia A** (o resolver Rust procura o
binario exatamente ali).

## Como rodar

```bash
# precisa de: bash, curl, gpg, tar (+ xz no Linux), unzip (Windows),
# sha256sum OU shasum. Node/npm vem do proprio tarball.
bash scripts/desktop/node-runtime/build-node-runtime.sh \
  --platform linux-x64 \
  --out /caminho/para/bundle

# provar que o bundle roda sozinho (esconde o Node global, sem NODE_PATH):
bash scripts/desktop/node-runtime/tests/prove-standalone.sh \
  --out /caminho/para/bundle \
  --expect-version "$(cat scripts/desktop/node-runtime/node-version.txt)"
```

Rode **na plataforma alvo** — nao ha cross-install: o `npm ci` resolve os
binarios nativos de `sharp` para a plataforma/arch/libc do runner. O CI faz
exatamente isso, um runner por plataforma (`.github/workflows/node-runtime.yml`).

Opcoes uteis: `--node-version X.Y.Z` (sobrepoe `node-version.txt`),
`--dist-base-url` (mirror; aceita `file://` para teste), `--keys-dir`,
`--bridge-dir`.

## Atualizar a versao do Node (com seguranca)

1. Confira o calendario oficial: a versao tem de ser uma linha **ativa** (nao
   EOL) em `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`
   e existir em `https://nodejs.org/dist/index.json`.
2. Edite **`node-version.txt`** (fonte unica de verdade; ex.: `24.22.1`).
3. Rode o script nas tres plataformas (o CI faz isso no PR) e confirme o
   `prove-standalone.sh`.
4. Se a nova release foi assinada por um signatario que **ainda nao esta** em
   `keys/FINGERPRINTS.txt`, o passo (b) falha com "assinante ... NAO esta na
   allowlist" — atualize o keyring (abaixo) **antes**.

## Atualizar o keyring (com seguranca)

As chaves do release team do Node **mudam** (entram e saem signatarios). O
keyring e vendorizado para que a verificacao seja offline e auditavel.

- A allowlist e `keys/FINGERPRINTS.txt` (um fingerprint de 40 hex por linha); o
  material de cada chave e `keys/<FINGERPRINT>.asc`. **Um fingerprint na lista
  sem o `.asc` correspondente nao verifica nada** — e vice-versa.
- Fonte canonica: a secao **"Release keys"** do README do Node.js
  (`github.com/nodejs/node`, bloco principal de signatarios ATIVOS — nao o
  `<details>` de "previous releases"). O material vem de
  `github.com/nodejs/release-keys` (diretorio `keys/`).
- Para cada chave nova:
  1. baixe `keys/<FPR>.asc` do repo `nodejs/release-keys`;
  2. confirme o fingerprint primario:
     `gpg --with-colons --import-options show-only --import <FPR>.asc`
     — a linha `fpr:` tem de bater com o nome do arquivo e com o README;
  3. so entao adicione o `.asc` **e** a linha em `FINGERPRINTS.txt`.
- Nunca remova uma chave que ainda assina uma linha LTS que o desktop empacota.

O estado atual (8 signatarios ativos, procedencia e data de sync) esta no
cabecalho de `keys/FINGERPRINTS.txt`.

## CI e testes

- `.github/workflows/node-runtime.yml` — matriz das 3 plataformas em `pull_request`
  (paths relevantes), `workflow_dispatch` e `workflow_call` (para a fatia C
  reusar). Cada job: monta o bundle (com `<out>` **num caminho com espaco**),
  esconde o Node global do runner, prova que o bundle roda sozinho, e publica
  `node-runtime-<plataforma>` como artifact (retencao curta). Um job Linux roda
  o teste negativo. Sem `continue-on-error`.
- `tests/negative-tamper.sh` — (1) teste unitario de `lib/check-gpg-status.sh`
  com status sinteticos do gpg (aceita GOODSIG+VALIDSIG allowlistado; recusa
  expirada/revogada/ruim/fora-da-allowlist/sem-GOODSIG), sem rede nem chave; e
  (2) fim-a-fim com mirror `file://`, provando que o build **recusa** artefato
  trocado (sha256 nao bate) e `SHASUMS256.txt` alterado (assinatura invalida).
- `tests/prove-standalone.sh` — prova que o bundle roda sem nenhum Node global
  e sem `NODE_PATH` (hoje carrega baileys+pino+sharp; vira `bridge.mjs
  --self-test` quando a fatia A mergear).

## manifest.json

```json
{
  "schema": "garraia.node-runtime-bundle/1",
  "generated_at": "<UTC ISO-8601 Z>",
  "platform": "win-x64 | linux-x64 | darwin-arm64",
  "node": {
    "version": "...", "archive": "...", "archive_sha256": "...",
    "source": "https://nodejs.org/dist/v.../", "signer_fingerprint": "..."
  },
  "bridge": {
    "name": "...", "package_version": "...", "lockfile_version": 3,
    "package_lock_sha256": "...", "baileys_version": "..."
  }
}
```

## Riscos conhecidos

- **Chaves do release team mudam.** Se o Node adicionar um signatario, uma
  release futura pode falhar o passo (b) ate o keyring ser atualizado (falha
  deliberada e ruidosa, nunca silenciosa). Ver "Atualizar o keyring".
- **`sharp`/libvips e por glibc/arch.** O `node_modules` e por
  plataforma/arch/libc; por isso o build roda no runner alvo, sem cross-install.
  O baseline Linux e glibc 2.35 (ubuntu-22.04), coerente com o resto do desktop.
- **Baileys sem release estavel.** A ponte pina `@whiskeysockets/baileys` numa
  `7.0.0-rc*`; subir essa versao e decisao da fatia/ponte, nao deste script.
