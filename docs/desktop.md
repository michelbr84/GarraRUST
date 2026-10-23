# Garra Desktop — instalador e runtime Node privado

O Garra Desktop (crate `garraia-desktop`, Tauri v2) empacota, num único
instalador por plataforma, o app (bandeja + overlay do papagaio + Chat Bar) e o
**gateway** como sidecar `garraia`. A partir da issue **#1636**, o instalador
traz também um **runtime Node.js privado** e as dependências de execução da
ponte do WhatsApp, para que o usuário **não precise instalar Node, npm ou usar
terminal** — nem baixar nada na primeira execução.

> O **contrato de resolução** do Node (ordem `env` → privado → `PATH`, e os
> caminhos por plataforma) está em [whatsapp.md §Node privado](./whatsapp.md#node-privado-e-ordem-de-resolucao).
> Este documento cobre o lado do **instalador**: o que ele traz, onde põe, o que
> a desinstalação remove e o que fica.

## O que o instalador traz

Além do app e do sidecar `garraia`, o pacote inclui, no diretório de resources
do Tauri (ver layout abaixo):

| Item | Caminho (relativo à raiz de resources) | Origem |
|------|----------------------------------------|--------|
| Runtime Node oficial | `node/node.exe` (Windows) · `node/bin/node` (Unix) | nodejs.org, verificado por GPG + SHA-256 |
| `LICENSE` do Node | `node/LICENSE` | tarball oficial do Node |
| Ponte pré-construída | `bridge-whatsapp/` (com `node_modules/` + `.package-lock.json`) | `bridge/whatsapp` + `npm ci --omit=dev --ignore-scripts` |
| Avisos de terceiros | `THIRD_PARTY_NOTICES.txt` | Node + todos os pacotes de `node_modules` |
| Manifesto do bundle | `manifest.json` | versão, sha256, fingerprint do assinante, estado do lock |

O `node/` e o `bridge-whatsapp/` são sempre **irmãos** — o resolver conta com
isso. A ponte é montada nativamente no runner de cada plataforma (sem
cross-install), então o `sharp` (peer do Baileys, addon nativo) vem com o
binário pré-compilado correto da plataforma.

## Onde o Tauri v2 põe o Node (layout por formato)

O diretório de resources do Tauri (`resource_dir()`) e a posição do sidecar
`externalBin` foram verificados **empiricamente** (`msiexec /a`, `dpkg-deb -c`,
`--appimage-extract`, inspeção do `.app`) e batem exatamente com um dos caminhos
do contrato da fatia A, **relativo ao sidecar `garraia`**:

| Formato | Sidecar `garraia` | Resources (= `node/`, `bridge-whatsapp/`) | Node privado resolvido |
|---------|-------------------|-------------------------------------------|------------------------|
| **Windows** MSI / NSIS | `<instal>\garraia.exe` | `<instal>\` (mesmo dir do exe) | `<instal>\node\node.exe` |
| **macOS** `.app` / `.dmg` | `Contents/MacOS/garraia` | `Contents/Resources/` | `Contents/Resources/node/bin/node` |
| **Linux** `.deb` / `.rpm` | `/usr/bin/garraia` | `/usr/lib/garraia-desktop/` | `/usr/lib/garraia-desktop/node/bin/node` |
| **Linux** AppImage | `$APPDIR/usr/bin/garraia` | `$APPDIR/usr/lib/garraia-desktop/` | `$APPDIR/usr/lib/garraia-desktop/node/bin/node` |

Em todos os casos o gateway (que roda como o processo do sidecar `garraia`)
resolve o Node privado por `current_exe()` + caminho relativo fixo, **sem** tocar
no `PATH` global nem substituir qualquer Node já instalado na máquina.

### Como é montado

Os scripts fonte-única de build do desktop
(`scripts/build-installer.ps1`, `scripts/build-desktop-linux.sh`,
`scripts/build-desktop-macos.sh`) chamam
`scripts/desktop/node-runtime/build-node-runtime.sh --platform <p> --out
crates/garraia-desktop/src-tauri/node-runtime` **antes** do `cargo tauri build`
(o `build.rs` do `tauri-build` copia os resources no compile do crate). O
`tauri.conf.json` mapeia esse diretório para dentro dos resources:

```jsonc
"resources": {
  "resources/config.default.yml": "resources/config.default.yml",
  "node-runtime/node": "node",
  "node-runtime/bridge-whatsapp": "bridge-whatsapp",
  "node-runtime/THIRD_PARTY_NOTICES.txt": "THIRD_PARTY_NOTICES.txt",
  "node-runtime/manifest.json": "manifest.json"
}
```

> `node-runtime/` é artefato de build (Node oficial + `node_modules`): está no
> `.gitignore`, nunca é versionado, e é montado do zero a cada build.

## O que a desinstalação remove e o que fica

- **Removido** (tudo o que o pacote instala): o app, o sidecar `garraia`, o
  `node/` privado, o `bridge-whatsapp/` (com o `node_modules`), o
  `THIRD_PARTY_NOTICES.txt` e o `manifest.json`. No Windows o MSI/NSIS tira a
  pasta de instalação; no Linux o `dpkg -r` tira `/usr/lib/garraia-desktop/` e
  `/usr/bin/garraia{,-desktop}`; no macOS some ao apagar o `.app`.
- **Preservado** (dado do usuário, nunca tocado pelo instalador):
  `~/.garraia/whatsapp/` — a sessão vinculada e a cópia **semeada** da ponte em
  `~/.garraia/whatsapp/bridge/`. Uma reinstalação ou atualização reaproveita a
  sessão; o pareamento do WhatsApp não precisa ser refeito.

A atualização (instalar uma versão nova por cima) troca o `node/` e o
`bridge-whatsapp/` do pacote e mantém `~/.garraia/whatsapp/` intacto. Quando a
ponte embutida muda, o gateway regrava no próximo boot só os arquivos que
diferem (ver [whatsapp.md](./whatsapp.md#garra-whatsapp-self-test---json)).

## Tamanho

Empacotar o runtime Node + `node_modules` aumenta cada instalador. Os tamanhos
exatos (antes, sem o bundle — baseline da release v0.4.9 — e depois, com o
bundle) são medidos e reportados por formato a cada build do desktop no CI
(`desktop.yml`), e no corpo da release.

## Validação no CI

O workflow `desktop.yml` **não** se contenta com "compilou". Por plataforma, os
jobs `smoke-*`:

1. pegam o instalador que o build gerou (artifact);
2. extraem a árvore de instalação num **caminho com espaço**;
3. **escondem todo Node global do `PATH`** (e confirmam `node` fora do `PATH`);
4. rodam o sidecar instalado `garraia whatsapp self-test --json` e exigem
   `node_origin == "privado"`, `self_test == "ok"` e o carregamento de
   `@whiskeysockets/baileys`, `pino` e `sharp` (addon nativo);
5. provam que `node/` e `bridge-whatsapp/node_modules` são irmãos do sidecar no
   caminho do contrato.

No Windows o smoke faz uma **instalação real silenciosa** (`msiexec /i /qn`, que
cai no default `C:\Program Files\...` — caminho com espaço), prova o layout e o
self-test, depois **desinstala** (`msiexec /x /qn`) e checa que `node/` e
`bridge-whatsapp/` somem e que `~/.garraia/whatsapp` fica.

Os jobs `update-linux` e `update-macos` cobrem **atualização + desinstalação**:
instalam o instalador publicado da **v0.4.9** (sem o bundle), criam um marcador
em `~/.garraia/whatsapp`, instalam o novo por cima, rodam o self-test de novo
(tem de virar `origin=privado`) e provam que o marcador **sobrevive**; depois
desinstalam e provam que o pacote some e o dado do usuário fica.

### Fora do escopo automatizado (hoje)

- **Upgrade-sobre-o-anterior do MSI (Windows)**: a `main` ainda está na versão
  0.4.9, e um major-upgrade real do WiX exige `ProductVersion` maior (bump de
  versão). O `smoke-windows` já cobre instalar-limpo + self-test + desinstalar +
  dado do usuário preservado; o upgrade-sobre-o-anterior volta ao CI quando a
  versão subir. Em Linux (`dpkg -i`) e macOS (cópia do `.app`) o upgrade é
  determinístico mesmo com versão igual e **é** testado.
- **Inicialização pela GUI do desktop** (o app Tauri sobe, lança o sidecar, o
  gateway responde `/api/health` e o sidecar morre ao fechar o app): depende de
  sessão gráfica; nos runners headless é instável e não é rodado como gate.
- **Pareamento real do WhatsApp**: exige um telefone e leitura de QR —
  intervenção externa. O `self-test` prova que a ponte **carrega**, não que
  pareia; os dois nunca são confundidos.
- **macOS**: o app não é assinado nem notarizado hoje (igual ao Windows). O
  binário `node` dentro do `.app` mantém a assinatura Developer ID da Node
  Foundation; o CI confirmou que rodá-lo por caminho a partir de um `.app` não
  assinado, em ambiente sem quarentena, **funciona** (o self-test passou).
  Assinatura/notarização é decisão do dono.
