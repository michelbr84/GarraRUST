use super::*;

/// #1909: as duas recusas do git que nomeiam o diretorio por extenso. O
/// caminho sai, a causa fica.
#[test]
fn oculta_o_caminho_das_recusas_do_git_e_preserva_a_causa() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    let bare = "fatal: cannot use bare repository '/home/alguem/proj secreto' \
                (safe.bareRepository is 'explicit')";
    assert_eq!(
        oculta_caminhos_absolutos(bare, None),
        format!("fatal: cannot use bare repository '{M}' (safe.bareRepository is 'explicit')")
    );

    let dubia = "fatal: detected dubious ownership in repository at '/srv/garra/wt'\n\
                 To add an exception for this directory, call:\n\n\
                 \tgit config --global --add safe.directory /srv/garra/wt\n";
    let saida = oculta_caminhos_absolutos(dubia, None);
    assert!(!saida.contains("/srv"), "{saida}");
    assert!(saida.contains("dubious ownership"), "{saida}");
    assert!(saida.contains(&format!("safe.directory {M}\n")), "{saida}");

    // Sem aspas, com pontuacao colada: o `:` da mensagem sobrevive.
    assert_eq!(
        oculta_caminhos_absolutos(
            "error: could not lock /a/b/.git/config: Permission denied",
            None
        ),
        format!("error: could not lock {M}: Permission denied")
    );
    // Windows e UNC.
    assert_eq!(
        oculta_caminhos_absolutos(r"fatal: not a git repository: 'C:\Users\dono\x'", None),
        format!("fatal: not a git repository: '{M}'")
    );
    assert_eq!(
        oculta_caminhos_absolutos(r"em \\servidor\share\x agora", None),
        format!("em {M} agora")
    );
}

/// Revisao da #1943: o caminho que o git cita e, no caso real, o proprio
/// `working_dir` da sessao. Ele sai RELATIVO (`.`, `.git/config`) — dizer
/// "fora do diretorio da sessao" seria falso e apagaria o que o modelo
/// precisa; so o que cai fora vira o marcador.
#[cfg(unix)]
#[test]
fn caminho_dentro_do_working_dir_sai_relativo() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    let wd = Path::new("/srv/sessao/proj");
    assert_eq!(
        oculta_caminhos_absolutos(
            "fatal: cannot use bare repository '/srv/sessao/proj' (safe.bareRepository is 'explicit')",
            Some(wd)
        ),
        "fatal: cannot use bare repository '.' (safe.bareRepository is 'explicit')"
    );
    assert_eq!(
        oculta_caminhos_absolutos(
            "error: could not lock /srv/sessao/proj/.git/config: File exists; ver /etc/gitconfig",
            Some(wd)
        ),
        format!("error: could not lock .git/config: File exists; ver {M}")
    );
}

/// Revisao da #1943: formas que o tokenizer deixava passar — `file://`,
/// caminho colado em `:` ou entre colchetes, e apostrofo dentro do caminho.
#[test]
fn formas_coladas_e_apostrofo_tambem_saem() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    for (entrada, esperado) in [
        (
            "fatal: '/home/o'brien/x' is not a repo".to_string(),
            format!("fatal: '{M}' is not a repo"),
        ),
        (
            "remote: file:///home/dono/x.git falhou".to_string(),
            format!("remote: file://{M} falhou"),
        ),
        (
            "hint: [/home/dono/x] e host:/home/dono/y".to_string(),
            format!("hint: [{M}] e host:{M}"),
        ),
    ] {
        let saida = oculta_caminhos_absolutos(&entrada, None);
        assert_eq!(saida, esperado);
        assert!(
            !saida.contains("dono") && !saida.contains("brien"),
            "{saida}"
        );
    }
}

/// Auditoria da #1943: caminho SEM aspas com espaco (formas reais do git
/// 2.43) nao deixa o rabo de fora, e a causa depois da pausa fica.
#[test]
fn caminho_sem_aspas_com_espaco_sai_inteiro() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    for (entrada, esperado) in [
        (
            "fatal: not a git repository: /home/dono/Meus Projetos/garra/.git/worktrees/wt2",
            format!("fatal: not a git repository: {M}"),
        ),
        (
            "fatal: bad config line 3 in file /mnt/My Big Volume/x/.git/config\nhint: veja",
            format!("fatal: bad config line 3 in file {M}\nhint: veja"),
        ),
        (
            "error: could not lock /a b/c/.git/config: File exists; ver /etc/gitconfig",
            format!("error: could not lock {M}: File exists; ver {M}"),
        ),
        (
            "\tgit config --global --add safe.directory /srv/garra/wt\n",
            format!("\tgit config --global --add safe.directory {M}\n"),
        ),
        (
            "fatal: /a/b nao e um repositorio valido",
            format!("fatal: {M} nao e um repositorio valido"),
        ),
    ] {
        assert_eq!(oculta_caminhos_absolutos(entrada, None), esperado);
    }
}

/// #1909 (residuo da auditoria da #1943): sem aspas, o ULTIMO componente
/// com espaco (`/home/dono/Meus Projetos`) deixava `Projetos` solto. Ele
/// sai inteiro quando e o `working_dir` da sessao ou um ancestral dele; a
/// causa depois da pausa continua dita, e o disco nao e consultado.
#[cfg(unix)]
#[test]
fn ultimo_componente_com_espaco_sai_inteiro_na_linhagem_da_sessao() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    let wd = Path::new("/home/dono/Meus Projetos/garra wt");
    for (entrada, esperado) in [
        // O proprio working_dir: sai relativo.
        (
            "fatal: not a git repository: /home/dono/Meus Projetos/garra wt",
            "fatal: not a git repository: .".to_string(),
        ),
        // Um ancestral, com pausa e causa depois: marcador inteiro.
        (
            "fatal: bad config in /home/dono/Meus Projetos: Permission denied",
            format!("fatal: bad config in {M}: Permission denied"),
        ),
        // Pontuacao final colada continua fora do caminho.
        (
            "hint: veja /home/dono/Meus Projetos. Depois tente",
            format!("hint: veja {M}. Depois tente"),
        ),
        // Prefixo da linhagem nao engole a palavra seguinte que nao e dela.
        (
            "fatal: /home/dono nao e um repositorio",
            format!("fatal: {M} nao e um repositorio"),
        ),
    ] {
        let saida = oculta_caminhos_absolutos(entrada, Some(wd));
        assert_eq!(saida, esperado);
        assert!(!saida.contains("Projetos"), "{saida}");
    }
}

/// Sem consulta ao disco: um caminho FORA da linhagem da sessao que
/// existe sai igual a um que nao existe — o nome solto e o limite
/// documentado, e e o mesmo nos dois casos (sem oraculo de existencia).
#[cfg(unix)]
#[test]
fn existencia_no_disco_nao_muda_a_saida() {
    use crate::tools::tool_context::CAMINHO_FORA_DO_TURNO as M;
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tmp.path().join("Meus Projetos")).expect("mkdir");
    let existe = format!("fatal: em {}/Meus Projetos", tmp.path().display());
    let nao_existe = format!("fatal: em {}/Meus Outros", tmp.path().display());
    assert_eq!(
        oculta_caminhos_absolutos(&existe, None),
        format!("fatal: em {M} Projetos")
    );
    assert_eq!(
        oculta_caminhos_absolutos(&nao_existe, None),
        format!("fatal: em {M} Outros")
    );
}

/// O que NAO e caminho absoluto do host fica como esta: relativo, URL,
/// barra solta, nome de ref.
#[test]
fn nao_toca_no_que_nao_e_caminho_absoluto() {
    for texto in [
        "fatal: Needed a single revision",
        "fatal: ambiguous argument 'origin/main': unknown revision",
        "warning: in the working copy of 'src/a.rs', LF will be replaced",
        "fatal: unable to access 'https://example.com/a/b.git/': timeout",
        "usage: git diff [<options>] [<commit>] [--] [<path>...]",
        "a / b // c",
        "",
    ] {
        assert_eq!(oculta_caminhos_absolutos(texto, None), texto);
    }
}

/// O portao: turno restrito oculta, turno aberto mostra, e fora de um
/// turno (sem escopo) e fail-closed.
#[tokio::test]
async fn stderr_para_o_turno_segue_o_portao() {
    use crate::tools::turn_tools::com_restricao_do_turno;
    let texto = "fatal: cannot use bare repository '/home/alguem/x'";
    let aberto = com_restricao_do_turno(false, async { stderr_para_o_turno(texto, None) }).await;
    assert_eq!(aberto, texto);
    let restrito = com_restricao_do_turno(true, async { stderr_para_o_turno(texto, None) }).await;
    assert!(!restrito.contains("/home/alguem"), "{restrito}");
    assert!(!stderr_para_o_turno(texto, None).contains("/home/alguem"));
}

#[test]
fn neutraliza_cada_driver_uma_vez() {
    let saida = b"filter.lfs.clean\0filter.lfs.smudge\0filter.My.Drv.process\0";
    let args = neutraliza_filtros(saida).expect("ok");
    assert!(args.contains(&"filter.lfs.clean=".to_string()));
    assert!(args.contains(&"filter.lfs.required=false".to_string()));
    assert!(args.contains(&"filter.My.Drv.process=".to_string()));
    assert_eq!(args.iter().filter(|a| *a == "filter.lfs.clean=").count(), 1);
    assert_eq!(args.len(), 2 * 4 * 2);
}

#[test]
fn nome_com_igual_recusa() {
    assert!(neutraliza_filtros(b"filter.a=b.clean\0").is_err());
}

#[test]
fn saida_vazia_nao_gera_nada() {
    assert!(neutraliza_filtros(b"").expect("ok").is_empty());
}
