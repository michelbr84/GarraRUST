//! Task de vigia do pai para `garra start --exit-with-parent <pid>` (#1636).
//!
//! A logica pura (observar o pai, decidir se ele se foi, pedir o encerramento)
//! vive em [`garraia_common::parent_watch`] e e testada la. Aqui fica so a
//! casca: um tick de ~1 s numa task tokio que, quando o pai some, pede o
//! encerramento gracioso — o mesmo caminho do SIGTERM, que ja derruba a ponte
//! Node e os demais subsistemas.
//!
//! No Windows e no-op: a contencao do par app->sidecar e o Job Object (a casca
//! poe o sidecar nele), entao a `garra` nem liga a task la.

/// Liga a vigia do processo pai de PID `expected_pid` numa task tokio.
///
/// Deve ser chamada de dentro do runtime tokio (o arm `start` em primeiro
/// plano). A task se encerra sozinha assim que detecta o sumico do pai.
#[cfg(unix)]
pub fn spawn(expected_pid: u32) {
    use std::time::Duration;

    use garraia_common::parent_watch::{
        initial_parent, observe_parent, parent_gone, request_self_shutdown,
    };

    let initial = initial_parent();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let obs = observe_parent(expected_pid);
            if parent_gone(&obs, initial) {
                tracing::info!(
                    parent = expected_pid,
                    "processo pai sumiu; encerrando o gateway graciosamente"
                );
                request_self_shutdown();
                break;
            }
        }
    });
}

/// No Windows a vigia por PID e dispensavel: o Job Object da casca
/// (`garraia-desktop`/`garraia-desktop-core`) encerra a arvore do sidecar
/// quando o app morre. Mantem a assinatura para o `start` nao precisar de
/// `cfg` no ponto de chamada.
#[cfg(not(unix))]
pub fn spawn(_expected_pid: u32) {}
