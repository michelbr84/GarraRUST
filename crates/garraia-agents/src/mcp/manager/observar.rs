//! #1438: o que o [`McpManager`] relata ao registro de confiabilidade
//! (`crate::observabilidade`): o transporte de cada servidor visto vivo ou
//! morto a cada tick do health monitor (o registro conta UMA queda por
//! transicao vivo -> morto) e o desfecho de cada tentativa de reconexao.
//!
//! Filho de `manager.rs` para ler o campo privado sem crescer aquele arquivo
//! (mais de 2 000 linhas). So nome de servidor (config do operador) e
//! booleano chegam ao registro — nunca `last_error`, comando ou caminho.

use std::sync::Arc;

use super::McpManager;
use crate::observabilidade::Observabilidade;

impl McpManager {
    /// Entrega o registro de confiabilidade. O gateway chama uma vez no boot,
    /// com o do `AgentRuntime`, antes de o health monitor subir; sem isto
    /// (a CLI local) nada e contado.
    pub fn set_observabilidade(&self, obs: Arc<Observabilidade>) {
        match self.observabilidade.write() {
            Ok(mut slot) => *slot = Some(obs),
            Err(envenenado) => *envenenado.into_inner() = Some(obs),
        }
    }

    /// O registro entregue por [`Self::set_observabilidade`], se houver.
    pub fn observabilidade(&self) -> Option<Arc<Observabilidade>> {
        match self.observabilidade.read() {
            Ok(slot) => slot.clone(),
            Err(envenenado) => envenenado.into_inner().clone(),
        }
    }

    /// Um tick do health monitor: cada transporte em `connections`, vivo ou
    /// morto.
    pub(super) fn observar_transportes<'a>(
        &self,
        transportes: impl Iterator<Item = (&'a str, bool)>,
    ) {
        if let Some(obs) = self.observabilidade() {
            for (servidor, vivo) in transportes {
                obs.registrar_transporte_mcp(servidor, vivo);
            }
        }
    }

    /// Uma tentativa de reconexao automatica terminou.
    pub(super) fn observar_reconexao(&self, servidor: &str, ok: bool) {
        if let Some(obs) = self.observabilidade() {
            obs.registrar_reconexao_mcp(servidor, ok, std::time::Instant::now());
        }
    }
}
