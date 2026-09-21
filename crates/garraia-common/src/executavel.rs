//! O nome pelo qual o usuario chamou este programa (#1329).
//!
//! Vive em `garraia-common` porque tres superficies imprimem instrucoes com o
//! nome do binario e todas rodam no MESMO executavel: a CLI (`garraia
//! whatsapp …`), o `next_step` compartilhado de `garraia-channels` (lido pela
//! CLI e pelo `/api/diagnostics`) e o log do gateway quando o canal nao sobe.
//! Uma copia por crate era o caminho para uma delas voltar a dizer `garra`
//! numa maquina que so tem `garraia`.
//!
//! O binario de verdade se chama `garraia`; `garra` e um symlink (ou alias)
//! que o instalador cria. As instrucoes impressas pela CLI ("agora rode
//! `garra start`") eram um literal fixo, entao quem instalou so o `garraia`
//! — imagem Docker, `cargo install`, `install.sh` sem o alias — recebia um
//! comando que nao existia na maquina. A instrucao tem de nomear o
//! executavel que **esta** na maquina, e a unica fonte disso e o proprio
//! `argv[0]` resolvido pelo SO.
//!
//! # Por que so dois nomes sao aceitos
//!
//! `current_exe()` devolve o que quer que esteja rodando: no `cargo test` e
//! `deps/garraia-3f2a…`, num binario renomeado pode ser qualquer coisa, e num
//! erro nao devolve nada. Ecoar um nome de harness de teste numa instrucao
//! para humanos seria pior que o literal antigo. Entao o helper so confia
//! em `garra` e `garraia`, e fora disso volta ao nome canonico do binario,
//! que e o que a documentacao e o `garra update` usam.

use std::path::Path;

/// Nome canonico do binario, e o fallback de tudo que nao e reconhecido.
const CANONICO: &str = "garraia";
/// O alias que o instalador cria.
const ALIAS: &str = "garra";

/// O nome do executavel em execucao: `garra` ou `garraia`.
///
/// Nunca falha e nunca devolve outra coisa — ver o docblock do modulo.
pub fn nome() -> String {
    nome_a_partir_de(std::env::current_exe().ok().as_deref())
}

/// Versao pura de [`nome`]: decide a partir de um caminho ja obtido.
///
/// `None` (erro do SO) e qualquer stem que nao seja exatamente `garra` ou
/// `garraia` — inclusive `garraia-3f2a…` do harness de teste — caem em
/// `garraia`. A extensao `.exe` do Windows e descartada antes da comparacao.
pub fn nome_a_partir_de(exe: Option<&Path>) -> String {
    let stem = exe.and_then(|p| {
        // `file_name` e nao `file_stem`: um nome como `garraia.exe` perde a
        // extensao aqui, mas um hipotetico `garra.ia` nao pode virar `garra`.
        let nome = p.file_name()?.to_str()?;
        Some(nome.strip_suffix(".exe").unwrap_or(nome))
    });
    match stem {
        Some(s) if s == ALIAS || s == CANONICO => s.to_string(),
        _ => CANONICO.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn o_binario_canonico_e_reconhecido() {
        assert_eq!(
            nome_a_partir_de(Some(Path::new("/usr/local/bin/garraia"))),
            "garraia"
        );
    }

    #[test]
    fn o_alias_e_reconhecido() {
        assert_eq!(nome_a_partir_de(Some(Path::new("/opt/x/garra"))), "garra");
    }

    #[test]
    fn a_extensao_do_windows_e_descartada() {
        // No Windows o `file_name` e `garraia.exe` e o strip devolve o nome
        // certo; no Linux `Path` nao separa por `\`, o "nome" e a string
        // inteira e a resposta e o fallback — o mesmo `garraia`, pelos dois
        // caminhos. O strip em si e provado pela segunda assercao, que nao
        // depende do separador.
        let caminho = PathBuf::from(r"C:\x\garraia.exe");
        assert_eq!(nome_a_partir_de(Some(&caminho)), "garraia");
        assert_eq!(nome_a_partir_de(Some(Path::new("garra.exe"))), "garra");
        assert_eq!(nome_a_partir_de(Some(Path::new("garraia.exe"))), "garraia");
    }

    #[test]
    fn o_harness_de_teste_cai_no_canonico() {
        assert_eq!(
            nome_a_partir_de(Some(Path::new("/tmp/deps/whatsapp-abc123"))),
            "garraia"
        );
        assert_eq!(
            nome_a_partir_de(Some(Path::new("/tmp/deps/garraia-3f2a9c"))),
            "garraia"
        );
    }

    #[test]
    fn sem_caminho_cai_no_canonico() {
        assert_eq!(nome_a_partir_de(None), "garraia");
    }

    #[test]
    fn um_nome_parecido_nao_e_aceito() {
        assert_eq!(nome_a_partir_de(Some(Path::new("/x/garra.ia"))), "garraia");
        assert_eq!(nome_a_partir_de(Some(Path::new("/x/garrai"))), "garraia");
    }

    /// A funcao real nunca devolve outra coisa que nao um dos dois nomes —
    /// no harness ela cai no canonico, e e isso que os testes de texto da
    /// CLI assumem.
    #[test]
    fn nome_real_e_sempre_um_dos_dois() {
        let n = nome();
        assert!(n == "garra" || n == "garraia", "{n}");
    }
}
