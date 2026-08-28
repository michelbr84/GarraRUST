# Runbook de Release

Como cortar uma release `vX.Y.Z` do GarraIA. Tudo depois do tag é automático.

## 1. Preparar (via PR — o `main` é protegido)

1. Bump da versão do workspace em `Cargo.toml` (`[workspace.package] version`).
2. Rodar `cargo check` para o `Cargo.lock` acompanhar (nunca editar o lock à mão).
3. `CHANGELOG.md`: mover o conteúdo de `## [Unreleased]` para uma nova seção
   `## [X.Y.Z] - AAAA-MM-DD` (data narrativa em horário da Flórida,
   ver CLAUDE.md §Convenção de datas). `[Unreleased]` volta vazio.
4. Abrir PR, aguardar CI verde (4 checks obrigatórios da ruleset) e mergear.

## 2. Tag

```bash
git fetch origin main
git tag vX.Y.Z <sha-do-merge>   # ou no HEAD de origin/main
git push origin vX.Y.Z
```

## 3. O que o tag dispara (automático)

| Workflow | Publica |
|---|---|
| `release.yml` | Binários linux-x86_64/arm64, windows, macos-intel/arm64 + `install.sh` + `SHA256SUMS` **e** um `<asset>.sha256` por binário (obrigatório: `garra update` lê o per-asset — `crates/garraia-cli/src/update.rs:127`) numa GitHub Release com notas geradas |
| `deploy.yml` | Imagem `ghcr.io/michelbr84/garraia` multi-arch (amd64+arm64), tags `X.Y.Z`, `X.Y` e sha |

Pré-releases: versões com `alpha`/`beta`/`rc` no nome são marcadas como
prerelease automaticamente.

## 4. Verificar

1. Actions: runs `Release` e `Deploy` verdes para o tag.
2. `https://github.com/michelbr84/GarraRUST/releases/latest` aponta para a
   versão nova, com todos os assets e seus `.sha256`.
3. `garra update` a partir da versão anterior encontra e instala a nova.
4. `docker pull ghcr.io/michelbr84/garraia:X.Y.Z` funciona.

## Rollback

Release ruim: apagar a Release e o tag (`git push origin :refs/tags/vX.Y.Z`),
corrigir via PR e cortar `vX.Y.Z+1`. Nunca reutilizar um tag já publicado —
`garra update` verifica SHA-256 e clientes podem ter cacheado os assets.
