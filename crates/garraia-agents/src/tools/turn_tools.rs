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

use std::future::Future;
use std::sync::Arc;

tokio::task_local! {
    static FERRAMENTAS_DO_TURNO: Arc<[String]>;
}

/// As ferramentas liberadas no turno em que esta execucao roda, ja na ordem
/// lexica. `None` fora de uma execucao despachada pelo `AgentRuntime`.
pub fn ferramentas_do_turno() -> Option<Vec<String>> {
    FERRAMENTAS_DO_TURNO.try_with(|v| v.to_vec()).ok()
}

/// Roda `fut` com `nomes` publicados como as ferramentas do turno. O runtime
/// chama em volta do `execute`; e publico para o teste do gateway montar o
/// mesmo escopo sem subir um turno inteiro.
pub async fn com_ferramentas_do_turno<F: Future>(mut nomes: Vec<String>, fut: F) -> F::Output {
    nomes.sort();
    nomes.dedup();
    FERRAMENTAS_DO_TURNO.scope(Arc::from(nomes), fut).await
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
            async { ferramentas_do_turno() },
        )
        .await;
        assert_eq!(
            dentro,
            Some(vec!["file_read".to_string(), "garra_status".to_string()])
        );
        assert_eq!(ferramentas_do_turno(), None);
    }
}
