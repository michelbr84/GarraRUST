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
//! o runtime publica a lista num `task_local` apenas em volta do `execute` de
//! tools nativas de introspeccao confiaveis; elas leem com
//! [`ferramentas_do_turno`]. As demais tools (incluindo MCP), testes e
//! chamadores fora do runtime leem `None`.
//!
//! Junto com a lista vai um bit: se o portao do turno restringe por
//! whitelist (o piso `search` do WhatsApp e os demais modos somente
//! leitura). O `garra_status` usa esse bit para nao entregar detalhe do
//! operador (diretorio, provedores, versao exata) num turno restrito, cujo
//! remetente nao e necessariamente o operador (#1347, fatia 3).

use std::future::Future;
use std::sync::Arc;

/// O que o runtime publica em volta do `execute` de tools nativas confiaveis
/// de introspeccao.
#[derive(Clone)]
struct Turno {
    ferramentas: Arc<[String]>,
    restrito: bool,
}

tokio::task_local! {
    static TURNO: Turno;
    /// So o bit do portao, publicado em volta do `execute` de QUALQUER tool.
    ///
    /// Revisao da onda C: o [`TURNO`] acima existe so em volta das tools
    /// nativas de introspeccao allowlisted, porque montar a lista de
    /// ferramentas liberadas a cada chamada seria custo sem leitor. Quando o
    /// `repo_search` passou a
    /// consultar [`turno_restrito`] (#1380), ele lia de um escopo que nunca
    /// e aberto para ele — a resposta era `None` SEMPRE, e o ramo do
    /// operador local virou codigo morto sem que nenhum teste percebesse (o
    /// teste montava o escopo a mao).
    ///
    /// Um `bool` copiado num `task_local` nao tem o custo que a lista tem,
    /// entao este vale para o despacho inteiro: o bit de #1347 fica
    /// disponivel para toda tool, hoje e para as proximas.
    static RESTRITO: bool;
    /// #1915: por que o portao barrou cada ferramenta REGISTRADA que ficou
    /// fora de [`TURNO`] — a camada (modo, teto de quem fala) e o motivo.
    /// Aberto pelo runtime junto com o [`TURNO`], para as mesmas tools
    /// nativas de introspeccao: sem isto o `garra_status` sabia QUE a
    /// ferramenta estava negada e nao sabia por qual camada.
    static RECUSAS: Arc<[(String, crate::modes::RecusaDoPortao)]>;
    /// #1985: as camadas de teto do portao deste turno, publicadas em volta
    /// do `execute` das tools de agendamento. E o que a tool grava na tarefa
    /// para ela rodar, depois, com a autoridade de quem a criou.
    static AUTORIDADE: Arc<crate::modes::AutoridadeDaTarefa>;
    /// #1985: o coletor das recusas de TETO, aberto por quem chama o runtime
    /// em volta do turno inteiro (hoje, a execucao de tarefa agendada). O
    /// runtime nao usa `tokio::spawn` no despacho, entao o escopo do chamador
    /// alcanca o ponto da recusa.
    static BLOQUEIOS: Arc<std::sync::Mutex<Vec<BloqueioDeTeto>>>;
}

/// Uma ferramenta que o modelo pediu e uma camada de TETO barrou (#1985).
/// Recusa so de modo nao entra: ali trocar de modo resolve, e nao e disso que
/// o aviso de tarefa bloqueada fala.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BloqueioDeTeto {
    pub ferramenta: String,
    /// O nome do teto que barrou (`TetoDeCapacidades::nome`).
    pub teto: String,
}

/// A autoridade do turno em que esta tool roda, se o runtime a publicou.
/// `None` fora de um despacho do `AgentRuntime` (teste que chama a tool
/// direto, CLI de inspecao): quem agenda sem autoridade grava a tarefa SEM
/// ela, e o executor aplica o fallback restritivo — nunca o portao aberto.
pub fn autoridade_do_turno() -> Option<crate::modes::AutoridadeDaTarefa> {
    AUTORIDADE.try_with(|a| (**a).clone()).ok()
}

/// Roda `fut` com `autoridade` publicada. O runtime chama em volta do
/// `execute` das tools de agendamento; e publico para o teste montar o mesmo
/// escopo.
pub async fn com_autoridade_do_turno<F: Future>(
    autoridade: crate::modes::AutoridadeDaTarefa,
    fut: F,
) -> F::Output {
    AUTORIDADE.scope(Arc::new(autoridade), fut).await
}

/// Roda `fut` coletando as recusas de teto que o runtime registrar dentro
/// dele. Devolve a saida de `fut` e os bloqueios, sem repeticao.
pub async fn coletando_bloqueios_de_teto<F: Future>(fut: F) -> (F::Output, Vec<BloqueioDeTeto>) {
    let coletor = Arc::new(std::sync::Mutex::new(Vec::new()));
    let saida = BLOQUEIOS.scope(Arc::clone(&coletor), fut).await;
    // Lock envenenado so acontece se um `push` entrou em panico; ai o que ha
    // para ler e o que ja estava la.
    let bloqueios = match coletor.lock() {
        Ok(g) => g.clone(),
        Err(envenenado) => envenenado.into_inner().clone(),
    };
    (saida, bloqueios)
}

/// O runtime chama no ponto em que recusa uma ferramenta por TETO. Sem
/// coletor aberto, nao faz nada.
pub(crate) fn registrar_bloqueio_de_teto(ferramenta: &str, teto: &str) {
    let _ = BLOQUEIOS.try_with(|coletor| {
        if let Ok(mut lista) = coletor.lock() {
            let novo = BloqueioDeTeto {
                ferramenta: ferramenta.to_string(),
                teto: teto.to_string(),
            };
            if !lista.contains(&novo) {
                lista.push(novo);
            }
        }
    });
}

/// A recusa do portao deste turno para `nome`, se o runtime a publicou.
/// `None` fora do escopo, e tambem para ferramenta liberada ou desconhecida
/// — quem le nao inventa camada: sem dado, o motivo fica generico.
pub fn recusa_do_turno(nome: &str) -> Option<crate::modes::RecusaDoPortao> {
    RECUSAS
        .try_with(|r| r.iter().find(|(n, _)| n == nome).map(|(_, recusa)| *recusa))
        .ok()
        .flatten()
}

/// Roda `fut` com as recusas do portao deste turno publicadas. O runtime
/// chama em volta do `execute` das tools de introspeccao; e publico para o
/// teste do gateway montar o mesmo escopo.
pub async fn com_recusas_do_turno<F: Future>(
    recusas: Vec<(String, crate::modes::RecusaDoPortao)>,
    fut: F,
) -> F::Output {
    RECUSAS.scope(Arc::from(recusas), fut).await
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

    #[tokio::test]
    async fn recusa_do_turno_so_existe_dentro_do_escopo_e_para_quem_foi_negado() {
        use crate::modes::{MotivoDaPolitica, RecusaDoPortao};
        assert_eq!(recusa_do_turno("bash"), None);
        let recusa = RecusaDoPortao {
            modo: None,
            teto: Some(MotivoDaPolitica::ClasseNegada),
        };
        let dentro = com_recusas_do_turno(vec![("bash".into(), recusa)], async {
            (recusa_do_turno("bash"), recusa_do_turno("file_read"))
        })
        .await;
        assert_eq!(dentro, (Some(recusa), None));
        assert_eq!(recusa_do_turno("bash"), None);
    }
}
