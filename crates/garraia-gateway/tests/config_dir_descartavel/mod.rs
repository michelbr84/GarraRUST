//! #2021: config dir descartavel para os testes que sobem o `GatewayServer`.
//!
//! `GatewayServer::run()` abre o `admin.db`, resolve o `master.key` e grava o
//! `sessions.db` e o `mcp.json` sob o `<config_dir>`. Sem `GARRAIA_CONFIG_DIR`
//! esse diretorio e o de quem roda `cargo test` (`~/.config/garraia`), e uma
//! rodada de teste trabalha sobre os dados reais do operador.
//!
//! Um diretorio por processo, nao por teste: a variavel e do processo, e os
//! testes de um mesmo arquivo rodam em paralelo com servidores que seguem de
//! pe ate o fim do binario. Um diretorio por teste seria trocado ou apagado
//! debaixo do servidor de outro teste.

use std::sync::OnceLock;

/// Aponta `GARRAIA_CONFIG_DIR` para um diretorio temporario que vive ate o
/// fim do processo. Chame antes de construir o `GatewayServer`.
///
/// Um `static` nao roda `Drop`, entao o diretorio fica no temp do sistema
/// depois do binario (prefixo `garraia-teste-config-`). E o preco de nao
/// apagar o config dir debaixo de um servidor ainda de pe.
pub fn isolar() {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("garraia-teste-config-")
            .tempdir()
            .expect("criar o config dir descartavel");
        // SAFETY: `get_or_init` roda isto uma vez so, e os testes que usam
        // este modulo chamam `isolar()` antes de subir o gateway, que e quem
        // le a variavel. Mesmo padrao do harness em `tests/common`.
        unsafe {
            std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        }
        dir
    });
}
