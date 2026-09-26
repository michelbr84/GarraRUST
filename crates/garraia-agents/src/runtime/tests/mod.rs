//! Testes do [`AgentRuntime`], um arquivo por tema.
//!
//! Ate 2026-09-25 tudo isto morava num unico `mod tests {}` dentro de
//! `runtime.rs`, que chegou a 10 881 linhas (6 763 de teste) — o maior `.rs`
//! do repositorio e a regressao numero um do Quality Ratchet (`.quality/`,
//! plan 0064). O ratchet conta TODO `.rs` rastreado, teste inclusive, e
//! tambem quantos passam de 700/1500/2500 linhas: por isso a divisao e em
//! arquivos de ate ~650 linhas, e nao num `runtime_tests.rs` unico.
//!
//! ## Como funciona
//!
//! - `use super::*` abaixo traz os itens (privados inclusive) de `runtime.rs`;
//!   cada arquivo faz `use super::*` e enxerga tudo por aqui.
//! - Helpers (providers de mentira, tools de mentira, `turno_de_streaming`…)
//!   ficam `pub(super)` no arquivo do tema que os criou e chegam aos outros
//!   pelos globs `use <tema>::*` desta lista.
//! - Os submodulos que ja eram aninhados (`aprovacao_entre_turnos`,
//!   `aviso_de_loop`, …) viraram arquivos irmaos sem mudar de caminho:
//!   `super::super::X` continua sendo o `runtime.rs`.
//! - As tres guardas que varrem o fonte (`include_str!("../../runtime.rs")`)
//!   agora leem um arquivo que e SO producao.
//!
//! ## Para um teste novo
//!
//! Escolha o tema; se nenhum serve, crie `runtime/tests/<tema>.rs`, declare-o
//! aqui (`mod` + `use <tema>::*` se ele exportar helper) e mantenha cada
//! arquivo abaixo de 700 linhas — o ratchet (`bash
//! scripts/quality/collect-metrics.sh`) acusa o contrario no PR.

use super::*;
use crate::memory_extractor::StructuredFact;
use crate::tools::{ToolContext, ToolOutput};
use async_trait::async_trait;
use garraia_common::Error;

mod aprovacao_entre_turnos;
/// Continuacao de [`aprovacao_entre_turnos`] (dividido pelo teto de 700 linhas do ratchet):
/// a aprovacao retomada e o pedido do `bash` sem marcador.
mod aprovacao_retomada;
mod aprovacao_vinculada;
mod aviso_de_loop;
mod disponibilidade;
/// T6 (`tool_program_com_passo_negado_casa_todo_inicio_com_fim_no_streaming`)
/// e T10 (`tool_program_que_esgota_a_tarefa_casa_todo_inicio_com_fim_no_streaming`)
/// usam o `RodaPrograma`, que so implementa `complete()`: o
/// `stream_complete` padrao devolve `Err`, e aqueles testes rodam o ramo
/// de FALLBACK em batch do turno de streaming. Estes cobrem o outro
/// ramo — o `tool_program` chega em `ToolUseStart` + `InputJsonDelta`
/// (partido em dois pedacos, como um provider real manda) e o input so
/// existe depois de o runtime juntar os deltas.
mod eventos_por_passo_no_streaming;
mod fallback_local;
mod mcp_no_runtime;
mod metricas_de_memoria;
mod nota_garra_status;
mod portao_e_fixtures;
mod repo_search_git_e_recall;
mod roteamento_e_fatos;
mod streaming_quebra_no_meio;
mod streaming_redo;
mod telemetria_e_ruido;
mod teto_por_capacidade;
mod tool_program_basico;
mod tool_program_gate_e_orcamento;
mod tool_program_loop_e_streaming;
mod tool_program_pausa;
mod workspace_por_sessao;

// So os temas que exportam helper para os outros; um tema novo entra aqui
// quando outro arquivo passar a usar algo dele (o clippy acusa o glob
// sem uso, e o compilador acusa o helper que falta).
use self::{
    mcp_no_runtime::*, metricas_de_memoria::*, portao_e_fixtures::*, streaming_redo::*,
    telemetria_e_ruido::*, tool_program_pausa::*,
};
