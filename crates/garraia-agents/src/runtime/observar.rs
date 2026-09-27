//! #1438: a ponte entre o despacho de ferramentas e o registro de
//! confiabilidade (`crate::observabilidade`).
//!
//! Mora fora de `runtime.rs` (que ja passa de 4 000 linhas) e e filho dele, o
//! que da acesso aos campos privados do [`AgentRuntime`] sem expor nada: no
//! despacho ficam so as chamadas — uma por recusa antes de executar (portao,
//! indisponibilidade, breaker aberto) e uma para a saida de toda ferramenta
//! executada, logo depois de alimentar o breaker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{AgentRuntime, TOOL_PROGRAM_NAME};
use crate::observabilidade::{Chamada, Desfecho, Observabilidade};
use crate::tools::ToolOutput;

impl AgentRuntime {
    /// O registro de confiabilidade deste runtime. O gateway o le para o
    /// `GET /admin/api/reliability`, o `/metrics` e a linha agregada do
    /// `/api/diagnostics`, e o entrega (clonando o `Arc`) ao `McpManager` e
    /// ao supervisor da ponte de canal.
    pub fn observabilidade(&self) -> &Arc<Observabilidade> {
        &self.observabilidade
    }

    /// Uma recusa ANTES de executar. Nada rodou, entao nao ha latencia.
    pub(super) fn observar_recusa(&self, name: &str, desfecho: Desfecho) {
        self.observabilidade.registrar(
            Chamada {
                nome: name,
                conhecida: self.e_ferramenta_conhecida(name),
                desfecho,
                duracao: Duration::ZERO,
                abriu_breaker: false,
            },
            Instant::now(),
        );
    }

    /// A saida de uma ferramenta executada: o desfecho sai da MESMA
    /// classificacao do breaker (timeout pelo prefixo do despacho, pedido de
    /// confirmacao neutro), e so ele — o texto da saida nao vai adiante.
    pub(super) fn observar_saida(
        &self,
        name: &str,
        saida: &ToolOutput,
        duracao: Duration,
        abriu_breaker: bool,
    ) {
        self.observabilidade.registrar(
            Chamada {
                nome: name,
                conhecida: self.e_ferramenta_conhecida(name),
                desfecho: Desfecho::da_saida(saida),
                duracao,
                abriu_breaker,
            },
            Instant::now(),
        );
    }

    /// So nome de ferramenta registrada (ou a intrinseca `tool_program`)
    /// vira chave. O nome vem do MODELO, que pode por ali texto do prompt; o
    /// que nao e de ferramenta registrada cai no balde constante do registro.
    fn e_ferramenta_conhecida(&self, name: &str) -> bool {
        name == TOOL_PROGRAM_NAME || self.find_tool(name).is_some()
    }
}
