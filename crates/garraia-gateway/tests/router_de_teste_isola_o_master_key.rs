//! #2021: o router de teste nunca le nem escreve o `master.key` do operador.
//!
//! O construtor de router sob `test-helpers` resolve a chave do admin contra
//! um `AdminStore::in_memory()` vazio. Com o store vazio, a resolucao sem
//! frase-senha troca um `master.key` de tamanho errado por uma chave nova
//! (#2013). Se o caminho do arquivo for o `<config_dir>/admin` real, uma
//! rodada de teste move a chave do operador para `master.key.unreadable` e
//! grava outra no lugar, e o gateway de verdade, com segredos no `admin.db`,
//! cai em 503.
//!
//! Um unico teste neste arquivo: `GARRAIA_CONFIG_DIR` e as variaveis da
//! frase-senha sao do processo, e cada arquivo de `tests/` e um binario
//! proprio.

use std::path::Path;

use garraia_gateway::admin::handlers::AdminKey;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::server::admin_key_for_test_router;

/// Tamanho errado de proposito: e o estado que a resolucao substitui.
const CHAVE_DO_OPERADOR: &[u8] = b"chave-do-operador-com-tamanho-errado";

fn arquivos_em(dir: &Path) -> Vec<String> {
    let mut nomes: Vec<String> = std::fs::read_dir(dir)
        .expect("listar o admin dir do operador")
        .map(|e| {
            e.expect("entrada do admin dir")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    nomes.sort();
    nomes
}

#[test]
fn o_router_de_teste_nao_toca_o_master_key_do_operador() {
    let config_dir = tempfile::tempdir().expect("config dir falso do operador");
    let admin_dir = config_dir.path().join("admin");
    std::fs::create_dir_all(&admin_dir).expect("criar o admin dir falso");
    let master_key = admin_dir.join("master.key");
    std::fs::write(&master_key, CHAVE_DO_OPERADOR).expect("gravar o master.key falso");

    // SAFETY: unico teste deste binario, e nenhuma outra thread foi criada.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", config_dir.path());
        // Sem frase-senha, a resolucao vai pelo `master.key`: o caminho do
        // defeito.
        std::env::remove_var("GARRAIA_ADMIN_KEY");
        std::env::remove_var("GARRAIA_VAULT_PASSPHRASE");
    }

    let mut store = AdminStore::in_memory().expect("store in-memory");
    let chave = admin_key_for_test_router(&mut store);

    assert!(
        matches!(chave, AdminKey::Ready(_)),
        "o router de teste precisa de uma chave pronta"
    );
    assert_eq!(
        std::fs::read(&master_key).expect("o master.key do operador continua no lugar"),
        CHAVE_DO_OPERADOR,
        "o master.key do operador foi reescrito por um teste"
    );
    assert_eq!(
        arquivos_em(&admin_dir),
        vec!["master.key".to_string()],
        "um teste criou arquivo no admin dir do operador"
    );
}
