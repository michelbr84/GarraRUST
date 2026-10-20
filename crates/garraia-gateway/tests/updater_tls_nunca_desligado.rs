//! #1671: o fork vendorizado do `tauri-plugin-updater` (PR #1668) **nunca**
//! desliga a verificacao de TLS.
//!
//! O upstream permite `dangerousAcceptInvalidCerts` /
//! `dangerousAcceptInvalidHostnames`, que chamam
//! `danger_accept_invalid_certs(true)` / `danger_accept_invalid_hostnames(true)`
//! no `reqwest::ClientBuilder` do updater — vetor de MITM de update, e os 4
//! alertas `rust/disabled-certificate-check` que o CodeQL apontou no PR #1668.
//! Neste fork os campos continuam sendo aceitos na config (nenhuma instalacao
//! quebra), mas sao ignorados. Este teste prende a escolha: nenhuma linha do
//! fork pode voltar a conter os sinks.

use std::path::{Path, PathBuf};

fn ler(caminho: &Path) -> String {
    match std::fs::read_to_string(caminho) {
        Ok(s) => s,
        Err(e) => panic!("nao consegui ler {}: {e}", caminho.display()),
    }
}

/// Todos os `.rs` do fork do updater, com o caminho relativo para a mensagem
/// de erro apontar o arquivo certo.
fn fontes_do_fork() -> Vec<(PathBuf, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/tauri-plugin-updater/src");
    let mut saida = Vec::new();
    let mut fila = vec![src.clone()];
    while let Some(dir) = fila.pop() {
        let entradas = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => panic!("nao consegui listar {}: {e}", dir.display()),
        };
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            if caminho.is_dir() {
                fila.push(caminho);
            } else if caminho.extension().is_some_and(|e| e == "rs") {
                saida.push((caminho.clone(), ler(&caminho)));
            }
        }
    }
    saida.sort();
    saida
}

/// Os sinks que o CodeQL apontou (`rust/disabled-certificate-check`). Voltar a
/// chamá-los é religar o MITM de update, mesmo que seja "so quando a config
/// pede": o fork nao tem esse modo (#1671).
#[test]
fn nenhum_arquivo_do_fork_desliga_a_verificacao_de_tls() {
    let sinks = [
        "danger_accept_invalid_certs(",
        "danger_accept_invalid_hostnames(",
    ];
    let fontes = fontes_do_fork();
    assert!(
        !fontes.is_empty(),
        "nenhum .rs encontrado no fork do updater: o caminho mudou?"
    );
    for (caminho, fonte) in &fontes {
        for sink in sinks {
            assert!(
                !fonte.contains(sink),
                "{sink} voltou para {}: o updater do fork nao pode desligar a \
                 verificacao de TLS (#1671)",
                caminho.display()
            );
        }
    }
}

/// O contrato de compatibilidade: os campos `dangerous_accept_invalid_certs` e
/// `dangerous_accept_invalid_hostnames` continuam EXISTINDO na config (parse
/// nao quebra instalacao nenhuma), e o updater avisa que os ignora. Sem o
/// aviso, configurar `dangerousAcceptInvalidCerts: true` parece funcionar e
/// nao funciona — a mentira silenciosa e o unico desfecho pior do que o modo.
#[test]
fn os_campos_continuam_aceitos_e_marcados_como_ignorados() {
    let config = ler(&Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/tauri-plugin-updater/src/config.rs"));
    for campo in [
        "pub dangerous_accept_invalid_certs: bool",
        "pub dangerous_accept_invalid_hostnames: bool",
    ] {
        assert!(
            config.contains(campo),
            "o campo `{campo}` sumiu da config do fork: instalacoes com \
             dangerousAcceptInvalidCerts param de fazer parse (#1671)"
        );
    }
    assert!(
        config.contains("Ignored in the GarraRUST fork (#1671)"),
        "a documentacao dos campos deixou de dizer que eles sao ignorados (#1671)"
    );
    let mut com_aviso = 0;
    for (_, fonte) in fontes_do_fork() {
        if fonte.contains("ignorados neste fork (GarraRUST #1671)") {
            com_aviso += 1;
        }
    }
    assert!(
        com_aviso > 0,
        "nenhum aviso de campo ignorado encontrado no fork (#1671)"
    );
}
