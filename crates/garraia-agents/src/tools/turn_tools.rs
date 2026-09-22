//! As ferramentas que o portao do turno deixa passar, visiveis a quem
//! executa dentro dele (#1347, revisao da onda A).
//!
//! `garra_status` relatava `AgentRuntime::tool_names()`, que e TODA tool
//! registrada. Nos modos restritos (o piso `search` do WhatsApp inclusive),
//! o modelo e mandado responder a partir desse relatorio, e ele listava
//! `bash`/`file_write` que o `ToolGate` do turno nega — o modelo dizia que
//! podia rodar comando, e a chamada era recusada. O portao do turno nasce do
//! `ExecContext` (piso do canal, modo customizado, `auto` deduzido), que a
//! tool nao enxerga: o modo gravado na sessao nao e o piso do canal.
//!
//! Em vez de mais um campo no `ToolContext` (montado em dezenas de lugares),
//! o runtime publica a lista num `task_local` em volta do `execute` da tool
//! que precisa dela, e a tool le com [`ferramentas_do_turno`]. Fora desse
//! escopo (teste, chamador que nao e o runtime) a leitura devolve `None`.
//!
//! Junto com a lista vai um bit: se o portao do turno restringe por
//! whitelist (o piso `search` do WhatsApp e os demais modos somente
//! leitura). O `garra_status` usa esse bit para nao entregar detalhe do
//! operador (diretorio, provedores, versao exata) num turno restrito, cujo
//! remetente nao e necessariamente o operador (#1347, fatia 3).

use std::future::Future;
use std::sync::Arc;

/// O que o runtime publica em volta do `execute` de `garra_status`.
#[derive(Clone)]
struct Turno {
    ferramentas: Arc<[String]>,
    restrito: bool,
}

tokio::task_local! {
    static TURNO: Turno;
    /// So o bit do portao, publicado em volta do `execute` de QUALQUER tool.
    ///
    /// Revisao da onda C: o [`TURNO`] acima existe so em volta de
    /// `garra_status`, porque montar a lista de ferramentas liberadas a cada
    /// chamada seria custo sem leitor. Quando o `repo_search` passou a
    /// consultar [`turno_restrito`] (#1380), ele lia de um escopo que nunca
    /// e aberto para ele — a resposta era `None` SEMPRE, e o ramo do
    /// operador local virou codigo morto sem que nenhum teste percebesse (o
    /// teste montava o escopo a mao).
    ///
    /// Um `bool` copiado num `task_local` nao tem o custo que a lista tem,
    /// entao este vale para o despacho inteiro: o bit de #1347 fica
    /// disponivel para toda tool, hoje e para as proximas.
    static RESTRITO: bool;
}

/// As ferramentas liberadas no turno em que esta execucao roda, ja na ordem
/// lexica. `None` fora de uma execucao despachada pelo `AgentRuntime`.
pub fn ferramentas_do_turno() -> Option<Vec<String>> {
    TURNO.try_with(|t| t.ferramentas.to_vec()).ok()
}

/// O portao deste turno restringe por whitelist? `None` fora de uma execucao
/// despachada pelo `AgentRuntime` — quem le decide o que isso significa (o
/// `garra_status` trata como nao restrito, porque fora do runtime nao ha
/// remetente remoto; o `repo_search` trata como restrito, porque esconder o
/// caminho do host e o lado seguro; ver o docstring de cada um).
///
/// Le o [`RESTRITO`], que o despacho abre para toda tool, e cai no [`TURNO`]
/// quando so ele existe — o caso do teste que monta o escopo do
/// `garra_status` a mao. Os dois carregam o mesmo
/// `portao.restringe_por_whitelist()` quando estao ambos abertos.
pub fn turno_restrito() -> Option<bool> {
    RESTRITO
        .try_with(|r| *r)
        .ok()
        .or_else(|| TURNO.try_with(|t| t.restrito).ok())
}

/// Roda `fut` com `restrito` publicado como o bit do portao deste turno.
///
/// O runtime chama em volta do `execute` de toda tool; e publico para o teste
/// montar o mesmo escopo sem subir um turno inteiro.
pub async fn com_restricao_do_turno<F: Future>(restrito: bool, fut: F) -> F::Output {
    RESTRITO.scope(restrito, fut).await
}

/// Roda `fut` com `nomes` publicados como as ferramentas do turno e
/// `restrito` como o bit do portao. O runtime chama em volta do `execute`; e
/// publico para o teste do gateway montar o mesmo escopo sem subir um turno
/// inteiro.
pub async fn com_ferramentas_do_turno<F: Future>(
    mut nomes: Vec<String>,
    restrito: bool,
    fut: F,
) -> F::Output {
    nomes.sort();
    nomes.dedup();
    TURNO
        .scope(
            Turno {
                ferramentas: Arc::from(nomes),
                restrito,
            },
            fut,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fora_do_escopo_e_none_dentro_e_a_lista_ordenada() {
        assert_eq!(ferramentas_do_turno(), None);
        let dentro = com_ferramentas_do_turno(
            vec![
                "file_read".into(),
                "garra_status".into(),
                "file_read".into(),
            ],
            true,
            async { (ferramentas_do_turno(), turno_restrito()) },
        )
        .await;
        assert_eq!(
            dentro,
            (
                Some(vec!["file_read".to_string(), "garra_status".to_string()]),
                Some(true)
            )
        );
        assert_eq!(ferramentas_do_turno(), None);
        assert_eq!(turno_restrito(), None);
        let aberto = com_ferramentas_do_turno(vec![], false, async { turno_restrito() }).await;
        assert_eq!(aberto, Some(false));
    }
}
