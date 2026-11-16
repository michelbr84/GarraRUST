//! #1912: um pedido de confirmacao emitido num processo nao e aprovavel em
//! outro — reiniciar o gateway faz o humano confirmar de novo.
//!
//! Os modulos `tools::approval`, `tools::pending_approval` e
//! `tools::session_approval` ja diziam isso em prosa ("a chave vive so na
//! memoria do processo", "o registro dura entre turnos, nao entre
//! processos"). O que faltava era a prova: ate aqui todo teste cunhava e
//! conferia a impressao digital no MESMO processo, entao trocar a chave
//! aleatoria por uma constante, ou por uma lida de disco, passaria verde.
//!
//! Este arquivo usa dois processos de verdade. O "gateway A" e o proprio
//! binario de teste reexecutado: ele sorteia a chave dele, cunha o marcador do
//! pedido e o devolve pelo stdout. O "gateway B" e o processo deste teste, que
//! recebe aquele marcador pelas duas vias por onde um pedido antigo poderia
//! voltar depois de um restart — o historico reidratado e o "sim" do humano
//! dentro do TTL — e precisa negar nas duas.
//!
//! O que NAO esta aqui: o percurso por Telegram e por MCP delegado, que a
//! #1912 tambem pede. Isso depende de transporte real e fica na issue.

use super::aprovacao_entre_turnos::*;
use crate::providers::{ChatMessage, ChatRole, ContentBlock, MessagePart};
use crate::tools::approval::{ApprovalFingerprint, MARKER_PREFIX, ToolApproval};
use garraia_common::ApprovalLevel;

/// Liga o papel de "gateway A" no processo filho e carrega o assunto do
/// pedido. Sem ela, [`filho_cunha_o_marcador_de_outro_processo`] nao faz nada.
const ENV_ASSUNTO: &str = "GARRAIA_TESTE_1912_ASSUNTO_DO_FILHO";

/// Prefixo da linha que o filho imprime. O harness do libtest escreve outras
/// linhas no mesmo stdout; esta e a unica que comeca assim.
const LINHA_DO_FILHO: &str = "MARCADOR_DO_FILHO_1912=";

/// A ferramenta de mentira do [`Cenario`] — o nome entra na impressao digital.
const TOOL: &str = "precisa_confirmar";

/// O papel de "gateway A". Rodado pelo `cargo test` comum e um no-op; rodado
/// por [`marcador_de_outro_processo`], cunha o marcador com a chave DESTE
/// processo e o imprime.
#[test]
fn filho_cunha_o_marcador_de_outro_processo() {
    let Ok(assunto) = std::env::var(ENV_ASSUNTO) else {
        return;
    };
    println!(
        "{LINHA_DO_FILHO}{}",
        ApprovalFingerprint::of(TOOL, &assunto).marker()
    );
}

/// Sobe outro processo, pede a ele o marcador de `(TOOL, assunto)` e o devolve
/// como texto — exatamente o que sobraria num historico gravado antes do
/// restart.
fn marcador_de_outro_processo(assunto: &str) -> String {
    let exe = std::env::current_exe().expect("caminho do binario de teste");
    // `module_path!()` comeca pelo nome da crate; o filtro do libtest nao.
    let (_, modulo) = module_path!()
        .split_once("::")
        .expect("modulo dentro da crate");
    let teste = format!("{modulo}::filho_cunha_o_marcador_de_outro_processo");
    let saida = std::process::Command::new(exe)
        .args([teste.as_str(), "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV_ASSUNTO, assunto)
        .output()
        .expect("processo filho");
    let stdout = String::from_utf8_lossy(&saida.stdout);
    assert!(
        saida.status.success(),
        "o processo filho falhou: {stdout}\n{}",
        String::from_utf8_lossy(&saida.stderr)
    );
    // `find`, nao `starts_with`: com `--nocapture` o libtest escreve
    // `test <nome> ... ` na mesma linha antes da saida do teste.
    let marcador = stdout
        .lines()
        .find_map(|l| {
            l.find(LINHA_DO_FILHO)
                .map(|i| &l[i + LINHA_DO_FILHO.len()..])
        })
        .unwrap_or_else(|| panic!("o filho nao imprimiu o marcador: {stdout}"))
        .trim()
        .to_string();
    assert!(
        ApprovalFingerprint::from_marker(&marcador).is_some(),
        "o filho devolveu um marcador bem formado: {marcador}"
    );
    marcador
}

/// O pedido pausado como ele ficaria num historico estruturado gravado pelo
/// processo anterior: resultado de tool, do lado do usuario, com o marcador.
fn historico_com(marcador: &str) -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: ChatRole::User,
        content: MessagePart::Parts(vec![ContentBlock::ToolResult {
            tool_use_id: "t-conf".into(),
            content: format!("{marcador} confirme a acao perigosa em x"),
        }]),
    }]
}

/// A base de tudo: a impressao digital cunhada no processo A nao cobre o
/// MESMO `(ferramenta, assunto)` no processo B, em nenhuma das formas que o
/// `ToolContext` carrega. Um segundo processo tampouco repete a do primeiro:
/// a chave e sorteada a cada inicio, nao derivada de algo estavel na maquina.
#[test]
fn impressao_digital_de_outro_processo_nao_cobre_o_mesmo_pedido() {
    let de_a = marcador_de_outro_processo("x");
    let fp_a = ApprovalFingerprint::from_marker(&de_a).expect("marcador de A");
    let fp_b = ApprovalFingerprint::of(TOOL, "x");
    assert_ne!(fp_a, fp_b, "a chave do marcador nao e por processo");

    let uma = ToolApproval::Granted(fp_a.as_str().to_string());
    assert!(!uma.covers(TOOL, "x"));
    let varias = ToolApproval::GrantedMany(vec![fp_a.as_str().to_string()]);
    assert!(!varias.covers(TOOL, "x"));

    // Controle: o que ESTE processo cunha continua valendo aqui. Sem isto, um
    // `covers` que negasse tudo passaria no teste.
    assert!(ToolApproval::Granted(fp_b.as_str().to_string()).covers(TOOL, "x"));

    let de_outro = marcador_de_outro_processo("x");
    assert_ne!(de_a, de_outro, "dois processos cunharam o mesmo marcador");
}

/// Via 1 — historico. O caminho sem escopo aprova pelo `ToolResult` do
/// historico (e o unico que ainda le o marcador de la). Com o marcador de
/// ESTE processo ele roda; com o do processo anterior, o "sim" nao aprova e a
/// ferramenta pergunta de novo, nos tres caminhos do runtime.
#[tokio::test]
async fn sim_sobre_historico_de_outro_processo_nao_executa() {
    let antigo = historico_com(&marcador_de_outro_processo("x"));
    for caminho in CAMINHOS {
        let mut c = Cenario::novo(caminho, &["x"]);
        let r = c.turno_com_historico("s1", "sim", None, &antigo).await;
        assert!(e_pedido(&r), "{caminho:?}: pergunta de novo: {r}");
        assert!(c.rodou().is_empty(), "{caminho:?}: nada roda");

        // Controle: o mesmo historico com o marcador deste processo aprova.
        let atual = historico_com(&ApprovalFingerprint::of(TOOL, "x").marker());
        let mut ok = Cenario::novo(caminho, &["x"]);
        ok.turno_com_historico("s1", "sim", None, &atual).await;
        assert_eq!(ok.rodou(), vec!["x".to_string()], "{caminho:?}");
    }
}

/// O mesmo historico, agora num caminho COM escopo (todo canal com humano
/// verificavel): la o historico ja nao aprova nada, e o marcador antigo nao
/// muda isso.
#[tokio::test]
async fn com_escopo_historico_de_outro_processo_nao_executa() {
    let antigo = historico_com(&marcador_de_outro_processo("x"));
    for caminho in CAMINHOS {
        let mut c = Cenario::novo(caminho, &["x"]);
        let r = c
            .turno_com_historico("s1", "sim", escopo("telegram", "s1", "dono"), &antigo)
            .await;
        assert!(e_pedido(&r), "{caminho:?}: {r}");
        assert!(c.rodou().is_empty(), "{caminho:?}");
    }
}

/// Via 2 — o "sim" dentro do TTL. O pedido pausa no gateway A; o gateway B
/// sobe (runtime novo, registro de pendencias vazio) com o mesmo historico em
/// texto que os canais persistem, e o humano responde "sim" na hora. Nada
/// roda: o "sim" vira um pedido NOVO, e so a aprovacao desse pedido novo
/// executa — uma vez.
#[tokio::test]
async fn sim_logo_apos_restart_pede_nova_aprovacao() {
    for caminho in CAMINHOS {
        let dono = || escopo("telegram", "s1", "dono");

        let mut a = Cenario::novo(caminho, &["x"]);
        let r1 = a.turno("s1", "apaga x", dono()).await;
        assert!(e_pedido(&r1), "{caminho:?}: A pausa: {r1}");
        assert_eq!(a.rt.pending_approvals.len(), 1, "{caminho:?}");
        assert!(
            a.historico.iter().all(|m| match &m.content {
                MessagePart::Text(t) => !t.contains(MARKER_PREFIX),
                MessagePart::Parts(_) => false,
            }),
            "{caminho:?}: o que A persiste e so texto, sem marcador"
        );

        let mut b = Cenario::novo(caminho, &["x", "x"]);
        b.historico = a.historico.clone();
        assert!(b.rt.pending_approvals.is_empty(), "{caminho:?}");

        let r2 = b.turno("s1", "sim", dono()).await;
        assert!(e_pedido(&r2), "{caminho:?}: B pergunta de novo: {r2}");
        assert!(b.rodou().is_empty(), "{caminho:?}: o sim antigo nao roda");
        assert!(a.rodou().is_empty(), "{caminho:?}");

        let r3 = b.turno("s1", "sim", dono()).await;
        assert_eq!(b.rodou(), vec!["x".to_string()], "{caminho:?}: {r3}");
    }
}

/// `agent.approval: session` tambem nao atravessa o restart: o que o humano
/// aprovou "para o resto da sessao" no gateway A e perguntado de novo no B.
#[tokio::test]
async fn aprovacao_da_sessao_nao_sobrevive_ao_restart() {
    for caminho in CAMINHOS {
        let dono = || escopo("telegram", "s1", "dono");

        let mut a = Cenario::novo(caminho, &["x", "x", "x"]);
        a.rt.set_approval_level(ApprovalLevel::Session);
        assert!(e_pedido(&a.turno("s1", "apaga x", dono()).await));
        a.turno("s1", "sim", dono()).await;
        let r = a.turno("s1", "apaga x de novo", dono()).await;
        assert!(!e_pedido(&r), "{caminho:?}: em A o pedido ficou lembrado");
        assert_eq!(a.rodou().len(), 2, "{caminho:?}");

        let mut b = Cenario::novo(caminho, &["x"]);
        b.rt.set_approval_level(ApprovalLevel::Session);
        b.historico = a.historico.clone();
        let r = b.turno("s1", "apaga x de novo", dono()).await;
        assert!(e_pedido(&r), "{caminho:?}: B pergunta de novo: {r}");
        assert!(b.rodou().is_empty(), "{caminho:?}");
    }
}

/// Guarda de fonte: a chave do marcador e os dois registros de aprovacao nao
/// tocam disco, banco nem ambiente. E o que sustenta "nao restaurar concessao
/// a partir de persistencia" — quem um dia quiser persistir isto tem de passar
/// por aqui e pelos testes de dois processos acima.
#[test]
fn chave_e_registros_de_aprovacao_ficam_so_em_memoria() {
    let fontes = [
        ("approval.rs", include_str!("../../tools/approval.rs")),
        (
            "pending_approval.rs",
            include_str!("../../tools/pending_approval.rs"),
        ),
        (
            "session_approval.rs",
            include_str!("../../tools/session_approval.rs"),
        ),
    ];
    let proibidos = [
        "std::fs",
        "tokio::fs",
        "File::",
        "rusqlite",
        "sqlx",
        "env::var",
        "include_bytes!",
    ];
    for (arquivo, fonte) in fontes {
        for p in proibidos {
            assert!(!fonte.contains(p), "{arquivo} passou a usar `{p}`");
        }
    }
}
