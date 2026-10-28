//! #1725: a data e hora do turno entram no system prompt.
//!
//! O modelo nao sabe que dia e; sem a linha ele inventa datas e nao tem base
//! para "amanha" ou "ate sexta". A composicao e pura e testavel com um instante
//! fixo; o `Local::now()` so entra em `com_contexto_temporal`.

use super::*;
use crate::persona::Lang;
use chrono::{DateTime, FixedOffset};

fn instante(rfc3339: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(rfc3339).expect("instante valido")
}

/// A frase em PT: dia da semana por extenso, data ISO, hora local e o
/// deslocamento do fuso — o que o modelo precisa para converter.
#[test]
fn linha_de_data_hora_em_pt() {
    let linha = linha_de_data_hora(instante("2026-10-27T23:55:00-04:00"), Lang::Pt);
    assert_eq!(
        linha,
        "Data e hora atuais: terça-feira, 2026-10-27 23:55 (UTC-04:00)."
    );
}

/// A mesma frase em EN, noutro fuso e noutro dia da semana.
#[test]
fn linha_de_data_hora_em_en() {
    let linha = linha_de_data_hora(instante("2026-11-01T09:05:00+00:00"), Lang::En);
    assert_eq!(
        linha,
        "Current date and time: Sunday, 2026-11-01 09:05 (UTC+00:00)."
    );
}

/// Os sete dias saem por extenso nos dois idiomas, sem depender do locale
/// do SO.
#[test]
fn todos_os_dias_da_semana_tem_nome() {
    let esperado_pt = [
        "segunda-feira",
        "terça-feira",
        "quarta-feira",
        "quinta-feira",
        "sexta-feira",
        "sábado",
        "domingo",
    ];
    let esperado_en = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    // 2026-10-26 e segunda-feira; os seis dias seguintes viram o mes.
    let segunda = instante("2026-10-26T12:00:00+00:00");
    for (i, (pt, en)) in esperado_pt.iter().zip(esperado_en).enumerate() {
        let dia = segunda + chrono::Duration::days(i as i64);
        assert!(
            linha_de_data_hora(dia, Lang::Pt).contains(pt),
            "{pt}: {}",
            linha_de_data_hora(dia, Lang::Pt)
        );
        assert!(
            linha_de_data_hora(dia, Lang::En).contains(en),
            "{en}: {}",
            linha_de_data_hora(dia, Lang::En)
        );
    }
}

/// A linha vai ao FIM do prompt existente, separada por linha em branco, e
/// uma vez so.
#[test]
fn com_contexto_temporal_anexa_ao_fim_uma_vez() {
    let composto = com_contexto_temporal(Some("Voce e Garra.".to_string()), Lang::Pt)
        .expect("prompt existente ganha a linha");
    assert!(composto.starts_with("Voce e Garra.\n\n"), "{composto}");
    assert_eq!(
        composto.matches("Data e hora atuais:").count(),
        1,
        "{composto}"
    );
    // O ano corrente aparece: e a data de agora, nao um placeholder.
    let ano = chrono::Local::now().format("%Y").to_string();
    assert!(composto.contains(&ano), "{composto}");
}

/// `PersonaMode::Neutral` devolve `None` para "nenhum system prompt"; uma
/// linha de data sozinha quebraria esse contrato pre-0250.
#[test]
fn com_contexto_temporal_preserva_o_none_do_modo_neutral() {
    assert_eq!(com_contexto_temporal(None, Lang::Pt), None);
    assert_eq!(com_contexto_temporal(None, Lang::En), None);
}

/// Com objetivo (#983) e data (#1725), a ordem e: prompt, objetivo, data.
#[test]
fn objetivo_vem_antes_da_data() {
    let composto = com_contexto_temporal(
        com_objetivo(Some("base".to_string()), Some("achar o bug")),
        Lang::Pt,
    )
    .expect("composto");
    let objetivo = composto.find("Objetivo declarado").expect("objetivo");
    let data = composto.find("Data e hora atuais").expect("data");
    assert!(objetivo < data, "{composto}");
}
