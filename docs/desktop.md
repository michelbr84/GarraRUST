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

**Atualização + desinstalação** (`update-linux`, `update-macos`, `update-windows`):
instalam o instalador **publicado da v0.4.9** (sem o bundle), criam um marcador
em `~/.garraia/whatsapp`, instalam o novo por cima, rodam o self-test de novo
(tem de virar `origin=privado`) e provam que o marcador **sobrevive**; depois
desinstalam e provam que o pacote some (`node/`, `bridge-whatsapp/`) e o dado do
usuário fica. No Windows, como a `main` está em 0.4.9 e um major-upgrade do WiX
exige `ProductVersion` maior, o job faz um bump só no CI (0.4.9 → 0.4.10, sem
commit), compila o MSI e testa o upgrade real por cima da v0.4.9 publicada.

**Inicialização pelo desktop** (`desktop-launch-{linux,windows,macos}`): lançam o
app instalado (Linux sob `xvfb`+`dbus`, Windows headed, macOS `open -a`), esperam
o sidecar `garraia start` responder `GET http://127.0.0.1:3888/api/health`,
**matam o app de forma abrupta** e provam que o sidecar morre em ≤ 15 s — pela
rede da fatia D (Job Object `KILL_ON_JOB_CLOSE` no Windows, `--exit-with-parent`
/ parent-watch no Linux/macOS). Se um runner não subir a GUI, o job falha com a
evidência (`app.log` + `ps`) para o roteiro manual.

O job `onboarding-ui` (Playwright, Ubuntu, **sem gateway**) abre
`ui/onboarding.html` por `file://` com um `window.__TAURI__` falso e dirige o
wizard emitindo os eventos NDJSON gravados da fatia 1 — inclusive um `qr` **real**
(`crates/garraia-desktop/tests/fixtures/qr-line.json`, 33×33, gerado pelo mesmo
`qr::modules`). Prova a navegação das telas, o canvas do QR **desenhado** (pixels
opacos escuros ≠ 0), a escolha de modo e a persona chegando aos comandos, e o
caminho de erro (QR expirado → mensagem acionável + "gerar um QR novo"). O job
`assert-ui-assets` tem um tripwire de grep que garante que a janela, os sete
comandos e a entrada em `capabilities` continuam ligados.

### Fora do escopo automatizado (hoje)

- **Pareamento real do WhatsApp**: exige um telefone e leitura de QR —
  intervenção externa. O `self-test` prova que a ponte **carrega**, não que
  pareia; os dois nunca são confundidos. O `onboarding-ui` prova a UI com um QR
  gravado, não um pareamento vivo.
- **macOS**: o app não é assinado nem notarizado hoje (igual ao Windows). O
  binário `node` dentro do `.app` mantém a assinatura Developer ID da Node
  Foundation; o CI confirmou que rodá-lo por caminho a partir de um `.app` não
  assinado, em ambiente sem quarentena, **funciona** (o self-test passou).
  Assinatura/notarização é decisão do dono.

## Assistente de Primeiro Uso (#1636)

Na primeira execução — sem `onboarding.json` com `concluido: true` em
`app_config_dir()` — o app abre o wizard `ui/onboarding.html`. São cinco telas:
**Boas-vindas → Modo → QR → Personalidade → Pronto**, em pt-BR por padrão. O
usuário não-dev instala o `.msi`, abre, escaneia o QR e troca mensagens com o
Garra **sem abrir terminal nenhuma vez**.

**Onde mora a lógica.** A casca Tauri (`src-tauri/src/onboarding.rs`) é fina: a
decisão vive em `garraia_desktop_core::onboarding` (puro, testado no CI — ADR
0021). A casca só lança o sidecar, transmite os eventos e grava config/estado.

- **QR**: a casca lança o sidecar `garraia whatsapp link --machine` (fatia 1,
  mesmo resolver de Node privado) e repassa cada linha NDJSON do stdout à webview
  pelo evento Tauri `wa://ndjson`. A webview rasteriza a **matriz** (`modules`,
  linhas de `0`/`1`) num `<canvas>`. O payload cru do QR (credencial) nunca chega
  à webview nem ao `/api/*`.
- **Cancelar** mata o processo do pareamento; no Windows ele entra no mesmo Job
  Object do gateway (`KILL_ON_JOB_CLOSE`), então uma morte abrupta do app durante
  o pareamento derruba a árvore Node (o `link` não aceita `--exit-with-parent`).
- **Modo → config** (confirmado em `docs/whatsapp.md` §Comandos, ADR 0025): o
  pareamento é igual nos dois modos; a política de acesso entra **depois** do
  `done`:
  - **Meu próprio número** → `garraia whatsapp access open --yes`: o Garra responde
    **qualquer** pessoa que mandar mensagem. A tela de Modo avisa isso em letras
    grandes — `open` afrouxa a admissão, e nenhum default de segurança é afrouxado
    em silêncio.
  - **Número dedicado** → `garraia whatsapp access restricted`: só quem está
    declarado/pareou fala com o Garra. É o **default fail-closed**; reaplicá-lo é
    o que reverte um `open` anterior quando se troca de volta.
- **Personalidade** → `agent.system_prompt`, montado por
  `garraia_desktop_core::montar_system_prompt` (nome, estilo, idioma) e gravado por
  `ConfigLoader::set_agent_system_prompt` (o mesmo writer de config do projeto, com
  modo `0600` no Unix).
- **Pronto** aplica a política do modo, persiste `onboarding.json`
  (`concluido: true`, `modo`) em `app_config_dir()` e **reinicia o gateway** para o
  canal recém-vinculado entrar no ar.

A escolha de modo **persiste e é configurável depois** em `ui/settings.html` →
seção **WhatsApp** (trocar modo e "Refazer pareamento").

### Validar o assistente (roteiro manual do dono)

O pareamento real precisa de um celular, então não é automatizável — este é o
passo manual. O artefato é o **MSI do run de CI** (`desktop.yml` →
`build-installer` → artifact `garraia-desktop-windows-pr`), ou o `.deb`/`.dmg` dos
jobs irmãos.

1. **Instale num Windows limpo** (idealmente sem Node global). Baixe o MSI do run
   de CI, rode o instalador e abra **Garra** pelo menu Iniciar. **Não abra
   terminal nenhum.**
2. **Boas-vindas**: a janela do assistente abre sozinha. Clique **Começar**.
   (Prova sem celular: o app grava `app_config_dir()/onboarding-shown.marker` ao
   exibir a tela.)
3. **Modo**: escolha **Meu próprio número** (o Garra responde todo mundo) ou
   **Número dedicado** (só você). Confira que o aviso amarelo explica a diferença.
4. **QR**: o código aparece no canvas com o status pulsante "Aponte a câmera…".
   No celular: **WhatsApp → Aparelhos conectados → Conectar um aparelho** e aponte
   para a tela. Se o QR expirar, use **Gerar um QR novo**. Esperado: o status passa
   por "Conectando a sua conta…" e some para a próxima tela.
5. **Personalidade**: dê um nome, escolha o estilo e deixe **Português (Brasil)**.
   Clique **Continuar**.
6. **Pronto**: o status deve virar **● Garra conectado**. Do celular, mande um
   **"oi"** para o número vinculado e confirme que o Garra responde. Clique
   **Abrir o Garra**.
7. **Settings**: abra Ajustes → **WhatsApp**, confirme que o modo escolhido está
   lá, troque-o e veja o status "o modo foi atualizado". "Refazer pareamento"
   reabre o wizard.

Em nenhum passo foi preciso abrir terminal. Se quiser conferir o
"sem-terminal" por fora: `onboarding-shown.marker` em `app_config_dir()` e a seção
`channels.whatsapp_linked` no `config.yml` provam que o fluxo todo passou pela UI.
