# 15. Linux packaging toolchain (nfpm + appimagetool)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão 2026-08-31)
- **Date:** 2026-08-31
- **Tags:** release, packaging, deb, rpm, appimage, installers
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Plan: [`plans/0361-linux-packages-and-windows-arm64.md`](../../plans/0361-linux-packages-and-windows-arm64.md)
  - Antecedente: [`plans/0359-windows-installer-and-release-matrix.md`](../../plans/0359-windows-installer-and-release-matrix.md) (§"Fora de escopo": "DMG notarizado, AppImage, `.deb`, `.rpm`; Windows ARM64 nativo")
  - Roadmap: [ROADMAP §4.1 Garra Desktop / Instaladores](../../ROADMAP.md)
  - Runbook: [`docs/releasing.md`](../releasing.md)

---

## Context and Problem Statement

Desde o plan 0359 a release publica binários crus, archives (`.tar.gz`/`.zip`),
instaladores Windows (MSI/NSIS) e os scripts `install.sh`/`install.ps1`. Para
Linux, porém, "instalar" ainda significava `curl | sh` ou baixar o binário na
mão — nada integrava com `apt`/`dnf`/`zypper` nem oferecia o formato portátil
que parte do ecossistema espera (AppImage). O backlog explícito do plan 0359 e
do ROADMAP §4.1 é fechar `.deb`, `.rpm` e AppImage.

Restrição estruturante herdada (CLAUDE.md regra 15): os assets crus
`garraia-<os>-<arch>[.exe]` + seus `.sha256` irmãos são superfície de
compatibilidade do `garra update` — qualquer formato novo entra
**aditivamente**, ao lado, nunca no lugar.

Decisões necessárias: (1) ferramenta para gerar `.deb`/`.rpm`; (2) ferramenta
para gerar AppImage; (3) onde os empacotamentos rodam no pipeline; (4) naming
dos assets.

## Decision Drivers

1. **★★★★★ Não recompilar** — os jobs de build já produzem os binários
   (`ubuntu-22.04` pinado por causa da baseline glibc 2.35). Empacotar deve
   consumir esses artefatos; uma segunda compilação criaria risco de divergir
   da baseline.
2. **★★★★★ Best-effort real** — falha de empacotamento nunca pode bloquear a
   release do CLI (padrão needs-mas-fora-do-`if:` de `release.yml`;
   `continue-on-error` é proibido pelo CLAUDE.md).
3. **★★★★ Reprodutibilidade/segurança do pipeline** — ferramentas pinadas por
   versão **e** SHA-256 no workflow; um "latest"/"continuous" móvel não pode
   quebrar (nem comprometer) uma release futura.
4. **★★★ Um só config para dois formatos** — deb e rpm compartilham 95% dos
   metadados; dois toolchains = dois configs divergindo.
5. **★★★ URLs estáveis** — `/releases/latest/download/<asset>` é o contrato de
   distribuição já usado por MSI/NSIS (nomes sem versão).

## Considered Options

### `.deb` / `.rpm`

| Opção | Avaliação |
|---|---|
| **nfpm (escolhida)** | Um binário Go estático; empacota binário **pré-buildado** em deb+rpm (e apk) a partir de um único `nfpm.yaml` com expansão de env; `version_schema: semver` converte prerelease para as gramáticas deb/rpm (sem o modo de falha do WiX com `-rc`); release upstream com checksums para pinar. |
| `cargo-deb` | Exige contexto cargo/`target/` (o job de release só tem artifacts baixados) e é deb-only — precisaria de um segundo toolchain para rpm. |
| `cargo-generate-rpm` | rpm-only; mesmo problema espelhado. |
| `fpm` | Arrasta runtime Ruby para o runner; projeto em manutenção mínima. |

### AppImage

| Opção | Avaliação |
|---|---|
| **appimagetool 1.9.1, repo `AppImage/appimagetool` (escolhida)** | O sucessor mantido do AppImageKit; publica **tags versionadas** (1.9.x) pináveis por SHA-256; roda em runner sem FUSE via `APPIMAGE_EXTRACT_AND_RUN=1`. |
| AppImageKit tag `13` | Era o pin clássico de CI, mas o upstream marcou a release como **"Obsolete version. DO NOT USE THIS VERSION ANYMORE."** — pinar em artefato desautorizado é dívida no dia 1. |
| linuxdeploy | Só publica tag `continuous` (móvel — inviável para pin por hash); resolve dependências dinâmicas que um binário CLI quase-estático não tem. |

## Decision Outcome

**nfpm v2.47.0** (deb+rpm) e **appimagetool 1.9.1** (AppImage), executando num
job dedicado **`package-linux`** do `release.yml`:

- `needs: [build-linux-x86_64, build-linux-arm64]`, gate
  `if: always() && needs.build-linux-x86_64.result == 'success'` — x86_64
  obrigatório, aarch64 best-effort (sem o binário, os pacotes arm64 só não
  saem, com `::warning::`).
- Job **separado** do `release` de propósito: qualquer step que falha dentro
  do `release` derruba a release inteira — o oposto de best-effort (driver 2).
- Ambas as ferramentas baixadas por URL de versão pinada e verificadas contra
  SHA-256 **fixado no workflow** (driver 3).
- Config em `packaging/nfpm.yaml` (env-expandido:
  `GARRAIA_PKG_{VERSION,ARCH,BIN}`), `.desktop` em
  `packaging/appimage/garraia.desktop` (`Terminal=true` — app de terminal),
  ícone reaproveitado de `crates/garraia-desktop/src-tauri/icons/`.
- Pacotes instalam **`/usr/bin/garraia`** — o mesmo nome que `install.sh`
  instala e que os archives carregam. `.deb` declara `libc6 (>= 2.35)`
  (baseline do build); o rpm **não** declara dependência de glibc porque o
  nome do provider varia entre distros rpm — o requisito fica na descrição do
  pacote e em `docs/installation.md`.
- **Naming dos assets: sem versão** — `garraia-linux-{x86_64,aarch64}.{deb,rpm}`
  e `garraia-linux-x86_64.AppImage` — seguindo o precedente do MSI para URLs
  `/releases/latest/download/` permanentes (driver 5). A versão real vive nos
  metadados internos do pacote. A âncora fim-de-linha do
  `select_checksum_line` (install.sh/install.ps1) já garante que os nomes
  crus nunca casem com os sufixos novos (invariante aditivo, regra 15).

### Consequences

- **Positivas:** `sudo apt install ./garraia-….deb` / `sudo rpm -i` /
  AppImage portátil sem tocar na superfície de compatibilidade; um config
  para deb+rpm; pipeline imune a "continuous" móvel.
- **Negativas / assumidas:**
  - Um `.deb`/`.rpm` instala binário root-owned em `/usr/bin`, então o
    `garra update` (troca atômica do próprio arquivo) exige `sudo` — o caminho
    de atualização recomendado para instalações via pacote é baixar o pacote
    novo. Documentado em `docs/installation.md`.
  - AppImage sai só para x86_64 na v0.3.4 (o runtime aarch64 exige
    `--runtime-file` e um segundo pin); registrado como follow-up no ROADMAP.
  - Pacotes **não assinados** (sem chave GPG de repositório apt/dnf) — mesmo
    status do MSI sem code-signing; a integridade vem do `SHA256SUMS` da
    release. Repositórios apt/dnf hospedados ficam fora de escopo.
  - Atualizar o pin de qualquer ferramenta = trocar versão **e** SHA-256 no
    `release.yml` (falha de checksum é o comportamento desejado quando o
    upstream re-publica um artefato).

---

## Amendment 2026-09-01 — pacotes Linux do Garra Desktop

O escopo deste ADR permanece a **CLI**: `package-linux` segue empacotando os
binários crus com nfpm + appimagetool pinados. Os pacotes do **desktop**
(`garraia-desktop-linux-x86_64.deb`/`.AppImage`, job `build-linux-desktop`,
`scripts/build-desktop-linux.sh`) usam o **bundler do próprio Tauri** — o
crate já depende dele, o sidecar/ícones/desktop-entry saem corretos de graça,
e replicar isso em nfpm duplicaria configuração. Consequência assumida: o
linuxdeploy que o Tauri baixa não é pinado por SHA-256 como as ferramentas da
CLI; o job é best-effort e uma quebra dele nunca bloqueia a release. O deb do
desktop declara `Provides/Conflicts/Replaces: garraia` porque instala o
sidecar em `/usr/bin/garraia` — o mesmo path que o deb da CLI possui.
