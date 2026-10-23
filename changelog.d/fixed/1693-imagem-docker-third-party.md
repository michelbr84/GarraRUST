- **A imagem Docker volta a compilar com os forks de `third_party/` (#1693).** O
  `[patch.crates-io]` que aponta `tauri-plugin-updater` e `utoipa-swagger-ui`
  para forks vendorizados (#1668) entrou depois da v0.4.9, e o Dockerfile nunca
  copiava `third_party/`: o `cargo chef cook` abortava com "patch location
  `/build/third_party/...` does not contain packages" e o Deploy do tag
  v0.4.10-rc1 nao publicou imagem. O Dockerfile agora copia `third_party/` no
  planner e nos dois lados do `cook`; o teste
  `dockerfile_copies_every_patch_path_outside_crates` prende a regra em todo
  PR, e o novo workflow `docker-build.yml` constroi a imagem (sem push) e
  responde `/ping` quando muda o Dockerfile, o `Cargo.toml` da raiz ou
  `third_party/`. O smoke Windows do desktop ganhou tetos internos que cabem no
  teto do job, para um travamento deixar log em vez de virar cancelamento mudo.
