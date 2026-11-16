//! #1912: o mesmo pedido relativo não herda aprovação de outro workspace.
use super::aprovacao_entre_turnos::{CAMINHOS, Cenario, e_pedido, escopo};
use crate::exec_context::ExecContext;
use garraia_common::ApprovalLevel;

fn exec(workspace: Option<&str>) -> ExecContext {
    ExecContext {
        approval_scope: escopo("web", "s1", "conn-a"),
        working_dir: workspace.map(str::to_string),
        ..ExecContext::default()
    }
}

#[tokio::test]
async fn pendencia_nao_atravessa_troca_de_workspace() {
    for caminho in CAMINHOS {
        for (antes, depois) in [
            (Some("workspace-a"), Some("workspace-b")),
            (Some("workspace-a"), None),
            (None, Some("workspace-b")),
        ] {
            let mut c = Cenario::novo(caminho, &["x", "x", "x"]);
            let r = c.turno_com_exec("s1", "apaga x", &[], &exec(antes)).await;
            assert!(e_pedido(&r), "{caminho:?}");
            let r = c.turno_com_exec("s1", "sim", &[], &exec(depois)).await;
            assert!(
                e_pedido(&r),
                "{caminho:?}: workspace mudou, deve perguntar: {r}"
            );
            assert!(
                c.rodou().is_empty(),
                "{caminho:?}: não executa em outro workspace"
            );
            let r = c.turno_com_exec("s1", "sim", &[], &exec(depois)).await;
            assert!(
                r.contains("concluido"),
                "{caminho:?}: nova aprovação vale: {r}"
            );
            assert_eq!(c.rodou(), vec!["x"]);
        }
    }
}

#[tokio::test]
async fn concessao_da_sessao_nao_atravessa_workspace() {
    for caminho in CAMINHOS {
        for (antes, depois) in [
            (Some("workspace-a"), Some("workspace-b")),
            (Some("workspace-a"), None),
            (None, Some("workspace-b")),
        ] {
            let mut c = Cenario::novo(caminho, &["x", "x", "x", "x", "x"]);
            c.rt.set_approval_level(ApprovalLevel::Session);
            let a = exec(antes);
            let b = exec(depois);
            assert!(e_pedido(&c.turno_com_exec("s1", "apaga x", &[], &a).await));
            c.turno_com_exec("s1", "sim", &[], &a).await;
            assert_eq!(c.rodou(), vec!["x"], "{caminho:?}");
            c.turno_com_exec("s1", "de novo", &[], &a).await;
            assert_eq!(
                c.rodou(),
                vec!["x", "x"],
                "{caminho:?}: mesmo workspace lembra"
            );
            let r = c.turno_com_exec("s1", "de novo", &[], &b).await;
            assert!(
                e_pedido(&r),
                "{caminho:?}: outro workspace exige aprovação: {r}"
            );
            assert_eq!(c.rodou(), vec!["x", "x"], "{caminho:?}");
            c.turno_com_exec("s1", "sim", &[], &b).await;
            assert_eq!(c.rodou(), vec!["x", "x", "x"], "{caminho:?}");
        }
    }
}

#[tokio::test]
async fn aprovacao_usa_workspace_efetivo_nao_so_o_declarado() {
    for caminho in CAMINHOS {
        let raiz = tempfile::tempdir().expect("workspace isolado");
        let mut c = Cenario::novo(caminho, &["x", "x", "x"]);
        c.rt.set_workspace_padrao(Some(crate::tools::SessionWorkspace::nova(
            raiz.path().to_path_buf(),
        )));
        let automatico = exec(None);
        let efetivo =
            c.rt.working_dir_efetivo(&automatico, "s1")
                .expect("raiz efetiva");
        let explicito = exec(Some(&efetivo));
        assert!(e_pedido(
            &c.turno_com_exec("s1", "apaga x", &[], &automatico).await
        ));
        c.turno_com_exec("s1", "sim", &[], &explicito).await;
        assert_eq!(
            c.rodou(),
            vec!["x"],
            "{caminho:?}: mesmo destino efetivo aprova"
        );
        let outro = exec(Some("outro-workspace"));
        assert!(e_pedido(&c.turno_com_exec("s1", "sim", &[], &outro).await));
        assert_eq!(c.rodou(), vec!["x"], "{caminho:?}");
    }
}
