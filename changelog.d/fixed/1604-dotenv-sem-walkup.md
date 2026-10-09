**`.env` e lido apenas do diretorio atual, sem walk-up pelos pais (#1604).**

`dotenvy::dotenv()` e `dotenvy::from_filename(".env")` sobem os diretorios
pais ate achar um `.env`; um arquivo na raiz do repo injetava
`GARRAIA_CONFIG_DIR`, `GARRAIA_EXECUTION_PROFILE` e credenciais em todo
comando `garra` e em todo boot do gateway rodado de dentro de uma
subpasta. CLI e gateway agora usam `dotenvy::from_path(".env")`, que e
`File::open` puro: o `.env` do proprio diretorio continua valendo, o do
pai deixa de valer.
