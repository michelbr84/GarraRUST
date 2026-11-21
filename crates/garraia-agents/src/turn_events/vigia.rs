//! O vigia que fecha com `Cancelada` a chamada cujo turno caiu (#1915).

use tokio::sync::mpsc;

#[cfg(doc)]
use super::TurnSink;
use super::{DesfechoDaChamada, TurnEvent};

/// Garante o `ToolFinished` de uma chamada cujo turno foi descartado (#1915).
///
/// # O buraco que ele fecha
///
/// Cancelar um turno (Ctrl+C da `garra chat`, teto de parede, cliente que
/// desistiu) e **dropar o futuro** do turno. Se isso acontece com uma
/// ferramenta em andamento, o `ToolStarted` ja saiu e o `ToolFinished` nunca
/// sai: quem le o fluxo fica com uma chamada aberta para sempre, sem desfecho
/// — exatamente o "timeout/cancelamento por chamada" que o criterio 4 da
/// #1915 pede.
///
/// # Como
///
/// O despacho arma o vigia logo depois do `ToolStarted` e o
/// [`desarmar`](Self::desarmar) depois de cada `tool_finished` concluido. Se o
/// vigia cai armado, o `Drop` envia `ToolFinished` com
/// [`DesfechoDaChamada::Cancelada`] por `try_send` — `Drop` nao pode esperar.
/// E melhor esforco por construcao: canal cheio ou receptor ja fechado perdem
/// o evento, e nenhum dos dois e pior que hoje. A `garra chat` dropa o turno
/// e **depois** drena o canal, entao la o evento chega.
///
/// Desarmar so depois do `.await` do `tool_finished` e de proposito: se o
/// proprio envio do fim for cancelado (canal cheio no momento do drop), o
/// evento nao entrou, e o vigia ainda armado o substitui. Nao ha caminho em
/// que os dois saiam.
///
/// Num `tool_program`, o futuro do passo vive dentro do futuro do envelope, e
/// o drop corre de dentro para fora: o passo fecha antes do envelope, na
/// ordem certa para quem empilha as chamadas.
#[must_use = "um vigia descartado na hora emite um `Cancelada` falso"]
pub struct VigiaDaChamada {
    armado: Option<(mpsc::Sender<TurnEvent>, String, String)>,
    iniciado_em: std::time::Instant,
}

impl VigiaDaChamada {
    pub(super) fn novo(armado: Option<(mpsc::Sender<TurnEvent>, String, String)>) -> Self {
        Self {
            armado,
            iniciado_em: std::time::Instant::now(),
        }
    }

    /// A chamada fechou pelo caminho normal: o `Drop` nao emite nada.
    pub fn desarmar(&mut self) {
        self.armado = None;
    }
}

impl Drop for VigiaDaChamada {
    fn drop(&mut self) {
        let Some((tx, id, name)) = self.armado.take() else {
            return;
        };
        let _ = tx.try_send(TurnEvent::ToolFinished {
            id,
            name,
            duration: self.iniciado_em.elapsed(),
            success: DesfechoDaChamada::Cancelada.e_sucesso(),
            outcome: DesfechoDaChamada::Cancelada,
            summary: "cancelada: o turno foi interrompido antes do fim da chamada".to_string(),
            output: String::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DesfechoDaChamada, TurnEvent, TurnSink};
    use tokio::sync::mpsc;

    /// #1915: vigia que cai armado fecha a chamada com `Cancelada`, com o
    /// mesmo id e nome do inicio.
    #[test]
    fn vigia_armado_no_drop_emite_cancelada() {
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(2);
        let sink = TurnSink::Events(tx);
        drop(sink.vigiar_chamada("c-9", "bash"));
        let Ok(TurnEvent::ToolFinished {
            id,
            name,
            success,
            outcome,
            output,
            ..
        }) = rx.try_recv()
        else {
            panic!("esperava ToolFinished do vigia");
        };
        assert_eq!((id.as_str(), name.as_str()), ("c-9", "bash"));
        assert_eq!(outcome, DesfechoDaChamada::Cancelada);
        assert!(!success);
        assert!(output.is_empty());
        assert!(rx.try_recv().is_err(), "um fim so");
    }

    /// Desarmado, o drop nao emite nada; num sink de texto, nunca emite.
    #[test]
    fn vigia_desarmado_ou_de_texto_fica_em_silencio() {
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(2);
        let sink = TurnSink::Events(tx);
        let mut vigia = sink.vigiar_chamada("c", "bash");
        vigia.desarmar();
        drop(vigia);
        assert!(rx.try_recv().is_err());

        let (tx, mut rx) = mpsc::channel::<String>(2);
        let sink = TurnSink::Text(tx);
        drop(sink.vigiar_chamada("c", "bash"));
        assert!(rx.try_recv().is_err());
    }

    /// Receptor fechado ou canal cheio: o drop nao entra em panico.
    #[test]
    fn vigia_tolera_canal_fechado_e_cheio() {
        let (tx, rx) = mpsc::channel::<TurnEvent>(1);
        let sink = TurnSink::Events(tx.clone());
        tx.try_send(TurnEvent::TextDelta("ocupa".into()))
            .expect("primeiro cabe");
        drop(sink.vigiar_chamada("c", "bash"));
        drop(rx);
        drop(sink.vigiar_chamada("c", "bash"));
    }
}
