use super::*;
use crate::tools::ToolOutput;
use crate::tools::file_jail::{DENIAL_MESSAGE, NO_ROOTS_MESSAGE};
use std::time::{Duration, Instant};

fn t0() -> Instant {
    Instant::now()
}

#[test]
fn deterministica_abre_ate_o_fim_do_turno_e_o_turno_seguinte_fecha() {
    let mut b = Breaker::new();
    let agora = t0();
    b.abrir_turno(None);
    assert_eq!(b.estado("file_read", agora), Estado::Fechado);
    b.registrar_falha(
        "file_read",
        Classe::Deterministica(Deterministica::SemRaiz),
        agora,
    );
    match b.estado("file_read", agora) {
        Estado::Aberto {
            motivo: Motivo::Deterministica(Deterministica::SemRaiz),
            ate: Ate::FimDoTurno,
        } => {}
        outro => panic!("{outro:?}"),
    }
    // Outra tool da mesma sessao nao e afetada.
    assert_eq!(b.estado("list_dir", agora), Estado::Fechado);
    // Muito depois continua aberta: e ate o fim do turno, nao por tempo.
    assert!(
        b.estado("file_read", agora + Duration::from_secs(3600))
            .esta_aberto()
    );
    // O turno seguinte, no mesmo contexto, fecha a parte deterministica.
    b.abrir_turno(None);
    assert_eq!(b.estado("file_read", agora), Estado::Fechado);
}

#[test]
fn transitoria_abre_por_cooldown_que_dobra_ate_o_teto() {
    let mut b = Breaker::new();
    let esperados = [15u64, 30, 60, 120, 120];
    let mut agora = t0();
    for (i, secs) in esperados.iter().enumerate() {
        b.registrar_falha("lenta", Classe::Transitoria, agora);
        let ate = match b.estado("lenta", agora) {
            Estado::Aberto {
                motivo: Motivo::Timeouts { seguidos },
                ate: Ate::Instante(ate),
            } => {
                assert_eq!(seguidos, (i as u32) + 1);
                ate
            }
            outro => panic!("falha {i}: {outro:?}"),
        };
        assert_eq!(ate - agora, Duration::from_secs(*secs), "falha {i}");
        // Um instante antes de vencer: aberta. No vencimento: fechada.
        assert!(
            b.estado("lenta", ate - Duration::from_millis(1))
                .esta_aberto()
        );
        assert_eq!(b.estado("lenta", ate), Estado::Fechado);
        agora = ate + Duration::from_secs(1);
    }
}

#[test]
fn sucesso_fecha_e_zera_a_serie_de_timeouts() {
    let mut b = Breaker::new();
    let t = t0();
    b.registrar_falha("lenta", Classe::Transitoria, t);
    b.registrar_falha("lenta", Classe::Transitoria, t);
    b.registrar_sucesso("lenta");
    assert_eq!(b.estado("lenta", t), Estado::Fechado);
    b.registrar_falha("lenta", Classe::Transitoria, t);
    match b.estado("lenta", t) {
        Estado::Aberto {
            motivo: Motivo::Timeouts { seguidos: 1 },
            ate: Ate::Instante(ate),
        } => assert_eq!(ate - t, COOLDOWN_BASE, "a serie recomecou do 1"),
        outro => panic!("{outro:?}"),
    }
    // A sondagem que deu certo no meio do turno tambem fecha a
    // deterministica.
    b.registrar_falha(
        "file_read",
        Classe::Deterministica(Deterministica::ForaDasRaizes),
        t,
    );
    assert!(b.estado("file_read", t).esta_aberto());
    b.registrar_sucesso("file_read");
    assert_eq!(b.estado("file_read", t), Estado::Fechado);
}

#[test]
fn generica_so_abre_depois_de_tres_erros_iguais_no_mesmo_turno() {
    let mut b = Breaker::new();
    let t = t0();
    b.abrir_turno(None);
    let erro = || Classe::Generica("connection refused".into());
    b.registrar_falha("web_fetch", erro(), t);
    b.registrar_falha("web_fetch", erro(), t);
    assert_eq!(b.estado("web_fetch", t), Estado::Fechado, "dois nao bastam");
    // Erro DIFERENTE nao soma na mesma serie.
    b.registrar_falha("web_fetch", Classe::Generica("404".into()), t);
    assert_eq!(b.estado("web_fetch", t), Estado::Fechado);
    b.registrar_falha("web_fetch", erro(), t);
    match b.estado("web_fetch", t) {
        Estado::Aberto {
            motivo: Motivo::Repetida { vezes: 3 },
            ate: Ate::FimDoTurno,
        } => {}
        outro => panic!("{outro:?}"),
    }
    // Turno novo: a contagem recomeca.
    b.abrir_turno(None);
    assert_eq!(b.estado("web_fetch", t), Estado::Fechado);
    b.registrar_falha("web_fetch", erro(), t);
    b.registrar_falha("web_fetch", erro(), t);
    assert_eq!(b.estado("web_fetch", t), Estado::Fechado);
}

#[test]
fn mudanca_de_contexto_limpa_tudo_e_o_mesmo_contexto_preserva_o_cooldown() {
    let mut b = Breaker::new();
    let t = t0();
    b.abrir_turno(Some("/projeto/a"));
    b.registrar_falha("lenta", Classe::Transitoria, t);
    b.registrar_falha(
        "repo_search",
        Classe::Deterministica(Deterministica::SemRepositorio),
        t,
    );
    // Turno novo, mesmo working_dir: o cooldown do timeout segue valendo,
    // a deterministica fecha.
    b.abrir_turno(Some("/projeto/a"));
    assert!(b.estado("lenta", t).esta_aberto());
    assert_eq!(b.estado("repo_search", t), Estado::Fechado);
    // Espacos em volta nao sao contexto diferente.
    b.abrir_turno(Some("  /projeto/a  "));
    assert!(b.estado("lenta", t).esta_aberto());
    // Working dir diferente: tudo limpo, inclusive a serie de timeouts.
    b.abrir_turno(Some("/projeto/b"));
    assert_eq!(b.estado("lenta", t), Estado::Fechado);
    b.registrar_falha("lenta", Classe::Transitoria, t);
    match b.estado("lenta", t) {
        Estado::Aberto {
            motivo: Motivo::Timeouts { seguidos: 1 },
            ..
        } => {}
        outro => panic!("a serie tinha de recomecar do 1: {outro:?}"),
    }
}

#[test]
fn classificar_reconhece_as_frases_das_tools_e_ignora_pedido_de_confirmacao() {
    assert_eq!(classificar(&ToolOutput::success("ok")), Veredito::Sucesso);
    assert_eq!(
        classificar(&ToolOutput::confirmation_request("Confirmar: rm x")),
        Veredito::Neutro,
        "pedido de confirmacao nao e falha"
    );
    assert_eq!(
        classificar(&ToolOutput::error(NO_ROOTS_MESSAGE)),
        Veredito::Falha(Classe::Deterministica(Deterministica::SemRaiz))
    );
    assert_eq!(
        classificar(&ToolOutput::error(format!("file_read: {DENIAL_MESSAGE}"))),
        Veredito::Falha(Classe::Deterministica(Deterministica::ForaDasRaizes))
    );
    assert_eq!(
        classificar(&ToolOutput::error(format!(
            "{} This session has no working directory.",
            crate::tools::repo_search_tool::SEM_REPOSITORIO
        ))),
        Veredito::Falha(Classe::Deterministica(Deterministica::SemRepositorio))
    );
    assert_eq!(
        classificar(&ToolOutput::error(format!("{TIMEOUT_PREFIXO}web_fetch"))),
        Veredito::Falha(Classe::Transitoria)
    );
    match classificar(&ToolOutput::error("  connection refused  ")) {
        Veredito::Falha(Classe::Generica(assinatura)) => {
            assert_eq!(assinatura, "connection refused");
        }
        outro => panic!("{outro:?}"),
    }
}

#[test]
fn a_assinatura_generica_e_truncada_para_nao_guardar_a_saida_inteira() {
    let longa = "x".repeat(10_000);
    match classificar(&ToolOutput::error(longa)) {
        Veredito::Falha(Classe::Generica(a)) => assert_eq!(a.len(), ASSINATURA_MAX),
        outro => panic!("{outro:?}"),
    }
}

#[test]
fn explicacao_nomeia_a_tool_o_codigo_e_pede_para_nao_repetir() {
    let t = t0();
    assert_eq!(Estado::Fechado.explicacao("x", t), None);
    let mut b = Breaker::new();
    b.registrar_falha(
        "repo_search",
        Classe::Deterministica(Deterministica::SemRepositorio),
        t,
    );
    let texto = b
        .estado("repo_search", t)
        .explicacao("repo_search", t)
        .expect("aberta");
    assert!(texto.contains("`repo_search`"), "{texto}");
    assert!(
        texto.contains("temporariamente indisponivel nesta conversa (no_repository)"),
        "{texto}"
    );
    assert!(
        texto.contains("Nao repita a chamada neste turno"),
        "{texto}"
    );

    b.registrar_falha("lenta", Classe::Transitoria, t);
    let depois = t + Duration::from_secs(5);
    let texto = b
        .estado("lenta", depois)
        .explicacao("lenta", depois)
        .expect("aberta");
    assert!(texto.contains("(timeout)"), "{texto}");
    assert!(texto.contains("10s"), "o que falta do cooldown: {texto}");
}

#[test]
fn abertas_lista_so_o_que_esta_aberto_agora_com_codigo_e_motivo() {
    let mut b = Breaker::new();
    let t = t0();
    b.registrar_falha("lenta", Classe::Transitoria, t);
    b.registrar_falha(
        "file_read",
        Classe::Deterministica(Deterministica::SemRaiz),
        t,
    );
    // Um erro generico so nao abre.
    b.registrar_falha("web_fetch", Classe::Generica("x".into()), t);
    let abertas = b.abertas(t);
    assert_eq!(abertas.len(), 2, "{abertas:?}");
    assert_eq!(abertas[0].tool, "file_read", "ordem lexica: {abertas:?}");
    assert_eq!(abertas[0].codigo, "no_roots");
    assert!(!abertas[0].motivo.is_empty());
    assert_eq!(abertas[1].tool, "lenta");
    assert_eq!(abertas[1].codigo, "timeout");
    // Vencido o cooldown, o timeout sai da lista sozinho.
    let depois = b.abertas(t + COOLDOWN_BASE);
    assert_eq!(depois.len(), 1);
    assert_eq!(depois[0].tool, "file_read");
}

#[test]
fn registro_por_sessao_isola_sessoes_e_expulsa_a_mais_antiga_no_teto() {
    let r = Breakers::com_capacidade(2);
    let t = t0();
    r.abrir_turno("a", None);
    r.registrar_falha(
        "a",
        "file_read",
        Classe::Deterministica(Deterministica::SemRaiz),
        t,
    );
    assert!(r.estado("a", "file_read", t).esta_aberto());
    assert_eq!(
        r.estado("b", "file_read", t),
        Estado::Fechado,
        "outra sessao nao ve"
    );
    assert!(r.abertas("b", t).is_empty());
    // Leitura de sessao desconhecida nao a cria.
    assert_eq!(r.sessoes(), 1);
    r.abrir_turno("b", None);
    r.abrir_turno("c", None);
    assert_eq!(r.sessoes(), 2, "teto");
    assert_eq!(
        r.estado("a", "file_read", t),
        Estado::Fechado,
        "a mais antiga saiu"
    );
    // `registrar` classifica pela saida da tool: falha abre, sucesso fecha.
    r.registrar(
        "c",
        "repo_search",
        &ToolOutput::error(format!(
            "{} nada aqui.",
            crate::tools::repo_search_tool::SEM_REPOSITORIO
        )),
        t,
    );
    assert_eq!(r.abertas("c", t)[0].codigo, "no_repository");
    r.registrar("c", "repo_search", &ToolOutput::success("achei"), t);
    assert!(r.abertas("c", t).is_empty());
}
