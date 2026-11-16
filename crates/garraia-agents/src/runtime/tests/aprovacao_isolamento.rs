//! #1912: regressões do registro de aprovação nos três caminhos do runtime.
//!
//! Usa provider e ferramenta locais, sem Telegram/MCP reais. Não prova vínculo
//! de workspace/id de chamada nem autoriza execução remota. O relógio do registro
//! é retrodatado explicitamente para testar expiração sem sleeps ou corrida no TTL.

use super::aprovacao_entre_turnos::*;
use crate::tools::approval::ApprovalFingerprint;
use crate::tools::pending_approval::PENDING_APPROVAL_TTL;
use std::time::{Duration, Instant};

#[tokio::test]
async fn pedido_expirado_pausa_e_exige_novo_sim_nos_tres_caminhos() {
    for caminho in CAMINHOS {
        let mut c = Cenario::novo(caminho, &["x", "x", "x"]);
        let scope = escopo("telegram", "s1", "dono").expect("escopo");
        assert!(e_pedido(
            &c.turno("s1", "apaga x", Some(scope.clone())).await
        ));
        assert_eq!(c.rt.pending_approvals.len(), 1, "{caminho:?}");
        assert!(c.rodou().is_empty(), "{caminho:?}");

        // Mantém o pedido real e sua impressão digital, mudando só a data de
        // criação para antes do TTL. A resolução continua pelo runtime real.
        c.rt.pending_approvals.register(
            &scope,
            "precisa_confirmar",
            ApprovalFingerprint::of("precisa_confirmar", "x"),
            Instant::now() - PENDING_APPROVAL_TTL - Duration::from_secs(1),
        );
        let r = c.turno("s1", "sim", Some(scope.clone())).await;
        assert!(e_pedido(&r), "{caminho:?}: vencido precisa pausar: {r}");
        assert!(c.rodou().is_empty(), "{caminho:?}: vencido não executa");
        assert_eq!(c.rt.pending_approvals.len(), 1, "novo pedido: {caminho:?}");

        c.turno("s1", "sim", Some(scope)).await;
        assert_eq!(c.rodou(), vec!["x".to_string()], "{caminho:?}");
        assert!(c.rt.pending_approvals.is_empty(), "{caminho:?}");
    }
}

#[tokio::test]
async fn duas_pendencias_coexistem_sem_trocar_alvo_ou_consumir_a_outra() {
    for caminho in CAMINHOS {
        // Dois pedidos vivos no MESMO runtime. Ordem inversa de aprovação
        // impede que uma implementação com um único slot global passe.
        let mut c = Cenario::novo(caminho, &["x", "y", "y", "x"]);
        let a = escopo("telegram", "chat-a", "dono");
        let b = escopo("telegram", "chat-b", "dono");
        assert!(e_pedido(
            &c.turno_com_historico("chat-a", "apaga x", a.clone(), &[])
                .await
        ));
        assert!(e_pedido(
            &c.turno_com_historico("chat-b", "apaga y", b.clone(), &[])
                .await
        ));
        assert!(c.rodou().is_empty(), "{caminho:?}");
        assert_eq!(c.rt.pending_approvals.len(), 2, "{caminho:?}");

        c.turno_com_historico("chat-b", "sim", b, &[]).await;
        assert_eq!(c.rodou(), vec!["y".to_string()], "{caminho:?}");
        assert_eq!(c.rt.pending_approvals.len(), 1, "{caminho:?}");
        c.turno_com_historico("chat-a", "sim", a, &[]).await;
        assert_eq!(
            c.rodou(),
            vec!["y".to_string(), "x".to_string()],
            "{caminho:?}"
        );
        assert!(c.rt.pending_approvals.is_empty(), "{caminho:?}");
    }
}
