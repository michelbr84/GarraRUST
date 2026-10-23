//! Fixture do teste de vigia de pai (#1636): roda o laco de vigia real e sai
//! quando o pai de PID `<expected>` some.
//!
//! Usa exatamente as mesmas funcoes puras que a `garraia-cli` liga em
//! `garra start --exit-with-parent` (`garraia_common::parent_watch`), so que
//! com um laco sincrono (`std::thread::sleep`) em vez do tick tokio — a logica
//! de deteccao, que e o que importa, e a mesma. O teste de integracao
//! `tests/parent_watch.rs` sobe isto, mata o PID vigiado e afirma que esta
//! vigia sai em <= N s.
//!
//! Fora de Unix a vigia e no-op (o pai parece sempre vivo); este bin so e
//! exercitado pelo teste no Unix.

use std::time::Duration;

use garraia_common::parent_watch::{initial_parent, observe_parent, parent_gone};

fn main() {
    let expected: u32 = match std::env::args().nth(1).and_then(|s| s.parse().ok()) {
        Some(pid) => pid,
        None => {
            eprintln!("uso: parent_watch_fixture <pid-esperado>");
            std::process::exit(2);
        }
    };

    let initial = initial_parent();
    loop {
        let obs = observe_parent(expected);
        if parent_gone(&obs, initial) {
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
