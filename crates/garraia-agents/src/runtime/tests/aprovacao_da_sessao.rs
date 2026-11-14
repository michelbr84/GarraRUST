//! #1858 fatia 4: `agent.approval: session` ponta a ponta no runtime.
//!
//! O cenario e o do [`super::aprovacao_entre_turnos`] (historico so em texto,
//! como os canais de producao guardam; registro do servidor por escopo), com
//! o nivel do runtime trocado. O que se prova aqui, nos tres caminhos com
//! pausa: o primeiro pedido continua perguntando, o "sim" passa a valer para
//! o MESMO pedido no resto da sessao, e nada alem disso muda — outro pedido,
//! outra sessao, outro canal, outro remetente, turno sem escopo e o default
//! `always` perguntam como antes.

use super::aprovacao_entre_turnos::*;
use garraia_common::ApprovalLevel;

fn cenario(caminho: Caminho, alvos: &[&str], nivel: ApprovalLevel) -> Cenario {
    let mut c = Cenario::novo(caminho, alvos);
    c.rt.set_approval_level(nivel);
    c
}

/// O coracao da fatia: depois do "sim", o MESMO pedido roda sem perguntar
/// nos turnos seguintes da sessao; um pedido diferente pergunta de novo.
#[tokio::test]
async fn session_sim_vale_para_o_mesmo_pedido_no_resto_da_sessao() {
    for caminho in CAMINHOS {
        let mut c = cenario(caminho, &["x", "x", "x", "y"], ApprovalLevel::Session);
        let e = || escopo("web", "s1", "conn-a");

        let r1 = c.turno("s1", "apaga x", e()).await;
        assert!(
            e_pedido(&r1),
            "{caminho:?}: o primeiro pedido pergunta: {r1}"
        );
        assert!(c.rodou().is_empty(), "{caminho:?}");

        let r2 = c.turno("s1", "sim", e()).await;
        assert_eq!(c.rodou(), vec!["x".to_string()], "{caminho:?}: {r2}");

        // Sem novo "sim": o mesmo pedido roda direto.
        let r3 = c.turno("s1", "apaga x de novo", e()).await;
        assert!(!e_pedido(&r3), "{caminho:?}: x ja estava aprovado: {r3}");
        assert_eq!(
            c.rodou(),
            vec!["x".to_string(), "x".to_string()],
            "{caminho:?}"
        );

        // Outro assunto nunca foi aprovado: pergunta, e nao roda.
        let r4 = c.turno("s1", "apaga y", e()).await;
        assert!(e_pedido(&r4), "{caminho:?}: y nao foi aprovado: {r4}");
        assert_eq!(
            c.rodou(),
            vec!["x".to_string(), "x".to_string()],
            "{caminho:?}"
        );
    }
}

/// Regressao do default: sob `always` o mesmo pedido pergunta de novo depois
/// do "sim" — exatamente o comportamento de antes da fatia.
#[tokio::test]
async fn always_continua_perguntando_o_mesmo_pedido() {
    for caminho in CAMINHOS {
        let mut c = cenario(caminho, &["x", "x", "x"], ApprovalLevel::Always);
        let e = || escopo("web", "s1", "conn-a");
        assert!(
            e_pedido(&c.turno("s1", "apaga x", e()).await),
            "{caminho:?}"
        );
        c.turno("s1", "sim", e()).await;
        assert_eq!(c.rodou(), vec!["x".to_string()], "{caminho:?}");

        let r3 = c.turno("s1", "apaga x de novo", e()).await;
        assert!(e_pedido(&r3), "{caminho:?}: always pergunta sempre: {r3}");
        assert_eq!(c.rodou(), vec!["x".to_string()], "{caminho:?}");
    }
}

/// O runtime nasce `always`: quem nunca chamou `set_approval_level` nao
/// muda de comportamento.
#[tokio::test]
async fn runtime_nasce_always() {
    let c = Cenario::novo(Caminho::AgentConfig, &[]);
    assert_eq!(c.rt.approval_level(), ApprovalLevel::Always);
}

/// A lembranca e do trio `(canal, sessao, remetente)`: outra sessao, outro
/// canal e outro remetente (o grupo) nao herdam o "sim" do dono — e o dono
/// continua com o dele.
#[tokio::test]
async fn session_nao_atravessa_sessao_canal_nem_remetente() {
    let mut c = cenario(
        Caminho::AgentConfig,
        &["x", "x", "x", "x", "x", "x"],
        ApprovalLevel::Session,
    );
    assert!(e_pedido(
        &c.turno("s1", "apaga x", escopo("telegram", "s1", "dono"))
            .await
    ));
    c.turno("s1", "sim", escopo("telegram", "s1", "dono")).await;
    assert_eq!(c.rodou(), vec!["x".to_string()]);

    let r = c
        .turno("s2", "apaga x", escopo("telegram", "s2", "dono"))
        .await;
    assert!(e_pedido(&r), "outra sessao: {r}");
    let r = c.turno("s1", "apaga x", escopo("web", "s1", "dono")).await;
    assert!(e_pedido(&r), "outro canal: {r}");
    let r = c
        .turno("s1", "apaga x", escopo("telegram", "s1", "outra-pessoa"))
        .await;
    assert!(e_pedido(&r), "outro remetente do mesmo grupo: {r}");
    assert_eq!(c.rodou(), vec!["x".to_string()], "nenhum deles rodou");

    // O dono, na sessao dele, segue com a aprovacao.
    let r = c
        .turno("s1", "apaga x", escopo("telegram", "s1", "dono"))
        .await;
    assert!(!e_pedido(&r), "{r}");
    assert_eq!(c.rodou(), vec!["x".to_string(), "x".to_string()]);
}

/// Sem escopo nao ha quem tenha aprovado: nada e lembrado, e o caminho
/// legado (historico em texto nunca aprova) segue igual.
#[tokio::test]
async fn session_sem_escopo_nao_lembra_nada() {
    for caminho in CAMINHOS {
        let mut c = cenario(caminho, &["x", "x", "x"], ApprovalLevel::Session);
        assert!(
            e_pedido(&c.turno("s1", "apaga x", None).await),
            "{caminho:?}"
        );
        assert!(e_pedido(&c.turno("s1", "sim", None).await), "{caminho:?}");
        assert!(
            e_pedido(&c.turno("s1", "apaga x", None).await),
            "{caminho:?}"
        );
        assert!(c.rodou().is_empty(), "{caminho:?}");
        assert!(c.rt.session_approvals.is_empty(), "{caminho:?}");
    }
}

/// Escopo de outra sessao que nao a do turno: fail-closed de antes — nada
/// aprovado, nada lembrado.
#[tokio::test]
async fn session_escopo_de_outra_sessao_nao_lembra() {
    let mut c = cenario(Caminho::AgentConfig, &["x", "x"], ApprovalLevel::Session);
    assert!(e_pedido(
        &c.turno("s1", "apaga x", escopo("web", "outra", "a")).await
    ));
    assert!(e_pedido(
        &c.turno("s1", "sim", escopo("web", "outra", "a")).await
    ));
    assert!(c.rodou().is_empty());
    assert!(c.rt.session_approvals.is_empty());
}

/// "nao" encerra o pedido (#1340) e nao vira lembranca: o mesmo pedido
/// depois pergunta de novo.
#[tokio::test]
async fn session_recusa_nao_vira_lembranca() {
    let mut c = cenario(
        Caminho::AgentConfig,
        &["x", "", "x"],
        ApprovalLevel::Session,
    );
    let e = || escopo("web", "s1", "a");
    assert!(e_pedido(&c.turno("s1", "apaga x", e()).await));
    let r = c.turno("s1", "nao", e()).await;
    assert!(!e_pedido(&r), "{r}");
    let r = c.turno("s1", "apaga x", e()).await;
    assert!(e_pedido(&r), "recusado nao fica aprovado: {r}");
    assert!(c.rodou().is_empty());
    assert!(c.rt.session_approvals.is_empty());
}
