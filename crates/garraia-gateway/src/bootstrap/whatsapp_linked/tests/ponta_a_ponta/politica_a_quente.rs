//! #1412 e #1423: a politica de acesso muda a quente e vale na mensagem
//! SEGUINTE — nos dois sentidos, sem janela de portao aberto — e o grupo e
//! uma fronteira propria, que nao herda o alcance de quem fala nele.
//!
//! Mesma fiacao dos outros modulos-filho (ponte falsa → sink → portao →
//! runtime → provider), com a config VIVA num `watch`. O que so estes testes
//! provam:
//!
//! - ligar ou desligar os grupos pela secao `access` a quente muda o
//!   destino da mensagem de grupo seguinte (o filtro de grupo nao pode ficar
//!   preso aos settings do boot);
//! - mensagens concorrentes a trocas de politica sao contadas exatamente uma
//!   vez — turno XOR recusa — e nenhum turno recebe mais do que o maior
//!   alcance configurado;
//! - promover, rebaixar e remover valem na mensagem seguinte;
//! - no grupo manda a politica do GRUPO (por JID ou o default), nunca o nivel
//!   de quem fala; e o remetente desconhecido nao entra por estar num grupo.

use super::*;

const GRUPO_A: &str = "120363000000000071@g.us";
const GRUPO_B: &str = "120363000000000072@g.us";
const ESTRANHO: &str = "5531944443333";
const ESCRITA: &str = "file_write";
const LEITURA: &str = "file_read";

/// A secao viva, so com a politica v2 (sem `allow`/`owners` legados).
fn viva(access: serde_json::Value) -> AppConfig {
    config_com(Some(secao(
        Some(true),
        serde_json::json!({ "allow": [], "owners": [], "access": access }),
    )))
}

/// Sobe com a config viva; `grupos_no_boot` e o `reply_in_groups` legado que
/// o boot leu — o que o filtro de grupo NAO pode continuar usando depois de
/// a secao `access` mudar.
async fn sobe(
    perfil: ExecutionProfile,
    rx: watch::Receiver<AppConfig>,
    grupos_no_boot: bool,
) -> Cenario {
    let c = Montagem {
        roteiro: Roteiro::eco().da_propria_conta(),
        perfil,
        config_viva: Some(rx),
        reply_in_groups: grupos_no_boot,
        ..Montagem::default()
    }
    .sobe(|_| {})
    .await;
    assert!(
        ate(|| c.state.whatsapp_linked.bridge() == BridgeView::Connected).await,
        "a ponte precisa estar de pe"
    );
    c
}

fn no_grupo_de(grupo: &str, remetente: &str, texto: &str) -> InboundMessage {
    let mut m = msg(Some(texto));
    m.chat_jid = Jid::new(grupo);
    m.sender_jid = Jid::new(format!("{remetente}@s.whatsapp.net"));
    m.sender_phone = Some(format!("+{remetente}"));
    m.is_group = true;
    m
}

fn de(remetente: &str, texto: &str) -> InboundMessage {
    let mut m = msg(Some(texto));
    m.chat_jid = Jid::new(format!("{remetente}@s.whatsapp.net"));
    m.sender_jid = Jid::new(format!("{remetente}@s.whatsapp.net"));
    m.sender_phone = Some(format!("+{remetente}"));
    m
}

/// Entrega e espera o espiao registrar — a ausencia de turno depois nao e
/// vacua.
async fn entrega_msg(c: &Cenario, m: InboundMessage) {
    let antes = recebidas(c).len();
    InboundSink::deliver(&*c.espiao, m);
    assert!(
        ate(|| recebidas(c).len() > antes).await,
        "a mensagem precisa chegar ao sink"
    );
}

fn ferramentas(c: &Cenario, i: usize) -> Vec<String> {
    turnos(&c.provider)
        .get(i)
        .map(|t| t.ferramentas.clone())
        .unwrap_or_default()
}

fn bloqueios(c: &Cenario) -> u64 {
    c.state
        .whatsapp_linked
        .rejeicoes()
        .de(rejeicoes::Motivo::Bloqueado)
}

/// **#1412/#1423 — grupos a quente, nos dois sentidos.** O boot tinha os
/// grupos desligados; a secao `access` os liga a quente e a mensagem de grupo
/// SEGUINTE vira turno; desligados de novo, a seguinte nao vira. Depois o
/// contrario: boot com o `reply_in_groups` legado ligado e `access.groups.
/// enabled: false` declarado a quente — o declarado vence (#1501) e nenhuma
/// mensagem de grupo vira turno.
#[tokio::test]
async fn ligar_e_desligar_grupos_a_quente_vale_na_mensagem_seguinte() {
    let usuario = serde_json::json!({ PEER: { "level": "read" } });
    let (tx, rx) = watch::channel(viva(serde_json::json!({
        "users": usuario.clone(),
        "groups": { "enabled": false }
    })));
    let c = sobe(ExecutionProfile::Standard, rx, false).await;

    entrega_msg(&c, no_grupo_de(GRUPO_A, PEER, "grupos desligados")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 1).await,
        0,
        "grupos desligados: sem turno"
    );

    tx.send(viva(serde_json::json!({
        "users": usuario.clone(),
        "groups": { "enabled": true, "default": { "level": "read" } }
    })))
    .expect("watcher");
    entrega_msg(&c, no_grupo_de(GRUPO_A, PEER, "grupos ligados a quente")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 1).await,
        1,
        "ligados pela secao `access` a quente, a mensagem de grupo seguinte vira turno"
    );

    tx.send(viva(serde_json::json!({
        "users": usuario,
        "groups": { "enabled": false }
    })))
    .expect("watcher");
    entrega_msg(&c, no_grupo_de(GRUPO_A, PEER, "desligados de novo")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 2).await,
        1,
        "desligados a quente, a mensagem de grupo seguinte nao vira turno"
    );
    encerra(c).await;

    // O contrario: o boot leu `reply_in_groups: true` (legado) e a secao viva
    // declara `access.groups.enabled: false`.
    let (_tx, rx) = watch::channel(viva(serde_json::json!({
        "users": { PEER: { "level": "read" } },
        "groups": { "enabled": false }
    })));
    let c = sobe(ExecutionProfile::Standard, rx, true).await;
    entrega_msg(
        &c,
        no_grupo_de(GRUPO_A, PEER, "legado ligado, declarado desligado"),
    )
    .await;
    assert_eq!(
        turnos_estaveis_em(&c, 1).await,
        0,
        "o `enabled: false` declarado vence o `reply_in_groups` do boot (#1501)"
    );
    encerra(c).await;
}

/// **#1412 — mensagens concorrentes a trocas de politica.** A politica
/// alterna entre "usuario `read`" e "usuario bloqueado" enquanto mensagens
/// chegam sem esperar umas pelas outras. Nenhuma se perde nem conta duas
/// vezes (turnos + recusas por bloqueio == mensagens) e nenhum turno recebe
/// alem do alcance `read` (nada de escrita nem shell) — nao existe janela
/// de portao aberto no meio da troca. No fim, com a politica parada, a
/// mensagem seguinte segue exatamente a ultima troca.
#[tokio::test]
async fn mensagens_concorrentes_a_trocas_de_politica_nunca_abrem_o_portao() {
    let leitura = viva(serde_json::json!({ "users": { PEER: { "level": "read" } } }));
    let bloqueado = viva(serde_json::json!({
        "users": { PEER: { "level": "read", "blocked": true } }
    }));
    let (tx, rx) = watch::channel(leitura.clone());
    let c = sobe(ExecutionProfile::Standard, rx, false).await;

    const MENSAGENS: usize = 40;
    let alternador = {
        let tx = tx.clone();
        let (leitura, bloqueado) = (leitura.clone(), bloqueado.clone());
        tokio::spawn(async move {
            for i in 0..(MENSAGENS * 2) {
                let proxima = if i % 2 == 0 { &bloqueado } else { &leitura };
                if tx.send(proxima.clone()).is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
    };
    for i in 0..MENSAGENS {
        InboundSink::deliver(&*c.espiao, de(PEER, &format!("concorrente {i}")));
        tokio::task::yield_now().await;
    }
    alternador.await.expect("alternador");

    let contabilizadas = |c: &Cenario| turnos(&c.provider).len() as u64 + bloqueios(c);
    assert!(
        ate(|| contabilizadas(&c) >= MENSAGENS as u64).await,
        "toda mensagem vira turno ou recusa: {} de {MENSAGENS}",
        contabilizadas(&c)
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        contabilizadas(&c),
        MENSAGENS as u64,
        "nenhuma mensagem contada duas vezes (turnos {} + bloqueios {})",
        turnos(&c.provider).len(),
        bloqueios(&c)
    );
    for (i, t) in turnos(&c.provider).iter().enumerate() {
        for acima in [ESCRITA, "bash"] {
            assert!(
                !t.ferramentas.iter().any(|f| f == acima),
                "turno {i} recebeu `{acima}` — acima de qualquer politica configurada: {:?}",
                t.ferramentas
            );
        }
    }

    // A politica parada: a mensagem seguinte segue a ultima troca, e so ela.
    let turnos_antes = turnos(&c.provider).len();
    let bloqueios_antes = bloqueios(&c);
    tx.send(bloqueado).expect("watcher");
    entrega_msg(&c, de(PEER, "bloqueado e parado")).await;
    assert!(ate(|| bloqueios(&c) == bloqueios_antes + 1).await);
    assert_eq!(turnos_estaveis_em(&c, turnos_antes).await, turnos_antes);
    tx.send(leitura).expect("watcher");
    entrega_msg(&c, de(PEER, "liberado e parado")).await;
    assert_eq!(
        turnos_estaveis_em(&c, turnos_antes + 1).await,
        turnos_antes + 1
    );
    assert!(
        ferramentas(&c, turnos_antes).iter().any(|f| f == LEITURA),
        "{:?}",
        ferramentas(&c, turnos_antes)
    );
    encerra(c).await;
}

/// **#1412 — promover, rebaixar e remover valem na mensagem seguinte.** No
/// pod, o mesmo numero: usuario `read` (sem shell) → promovido a dono (piso
/// do pod, com shell) → rebaixado (sem shell de novo) → removido (nem vira
/// turno; a recusa e contada como politica restrita).
#[tokio::test]
async fn promover_rebaixar_e_remover_valem_na_mensagem_seguinte() {
    let usuario = viva(serde_json::json!({ "users": { PEER: { "level": "read" } } }));
    let (tx, rx) = watch::channel(usuario.clone());
    let c = sobe(ExecutionProfile::IsolatedPod, rx, false).await;

    entrega(&c, "como usuario").await;
    assert_eq!(turnos_estaveis_em(&c, 1).await, 1);
    assert!(
        !ferramentas(&c, 0).iter().any(|f| f == "bash"),
        "{:?}",
        ferramentas(&c, 0)
    );

    tx.send(viva(
        serde_json::json!({ "users": { PEER: { "role": "owner" } } }),
    ))
    .expect("watcher");
    entrega(&c, "como dono").await;
    assert_eq!(turnos_estaveis_em(&c, 2).await, 2);
    assert!(
        ferramentas(&c, 1).iter().any(|f| f == "bash"),
        "promovido a dono no pod, o piso sobe na mensagem seguinte: {:?}",
        ferramentas(&c, 1)
    );

    tx.send(usuario).expect("watcher");
    entrega(&c, "rebaixado").await;
    assert_eq!(turnos_estaveis_em(&c, 3).await, 3);
    assert!(
        !ferramentas(&c, 2).iter().any(|f| f == "bash"),
        "rebaixado, o shell sai na mensagem seguinte: {:?}",
        ferramentas(&c, 2)
    );

    tx.send(viva(serde_json::json!({ "users": {} })))
        .expect("watcher");
    entrega(&c, "removido").await;
    assert_eq!(turnos_estaveis_em(&c, 4).await, 3, "removido: sem turno");
    assert_eq!(
        c.state
            .whatsapp_linked
            .rejeicoes()
            .de(rejeicoes::Motivo::Restrita),
        1
    );
    encerra(c).await;
}

/// **#1423 — no grupo manda a politica do grupo.** Grupos ligados, default
/// `chat` e o GRUPO_A com politica propria `read`. Um usuario `full` em
/// conversa direta fala nos dois grupos: no A recebe leitura (o teto do
/// grupo, nao o dele — nada de escrita); no B, sem politica propria, o default
/// `chat` (nenhuma ferramenta). E um remetente desconhecido num grupo ligado
/// nao entra: a admissao continua `restricted`.
#[tokio::test]
async fn no_grupo_manda_a_politica_do_grupo_e_nao_a_de_quem_fala() {
    let (_tx, rx) = watch::channel(viva(serde_json::json!({
        "users": { PEER: { "level": "full", "write": true } },
        "groups": {
            "enabled": true,
            "default": { "level": "chat" },
            GRUPO_A: { "level": "read" }
        }
    })));
    let c = sobe(ExecutionProfile::Standard, rx, false).await;

    entrega_msg(&c, no_grupo_de(GRUPO_A, PEER, "no grupo A")).await;
    assert_eq!(turnos_estaveis_em(&c, 1).await, 1);
    let a = ferramentas(&c, 0);
    assert!(a.iter().any(|f| f == LEITURA), "grupo A e `read`: {a:?}");
    assert!(
        !a.iter().any(|f| f == ESCRITA || f == "bash"),
        "o `full` de quem fala nao entra no grupo: {a:?}"
    );

    entrega_msg(&c, no_grupo_de(GRUPO_B, PEER, "no grupo B")).await;
    assert_eq!(turnos_estaveis_em(&c, 2).await, 2);
    assert!(
        ferramentas(&c, 1).is_empty(),
        "grupo sem politica propria recebe o default `chat`: {:?}",
        ferramentas(&c, 1)
    );

    entrega_msg(&c, no_grupo_de(GRUPO_A, ESTRANHO, "estranho no grupo A")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 3).await,
        2,
        "remetente desconhecido nao entra por estar num grupo ligado"
    );
    encerra(c).await;
}
