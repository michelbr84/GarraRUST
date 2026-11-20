//! #1542: `restart` nao derruba um daemon que pertence a uma unit systemd.
//!
//! `try_stop_daemon` mata quem estiver segurando a porta e `start_daemon`
//! sobe um processo novo **fora** da unit. Com uma unit `--user` ativa isso
//! produz o pior resultado possivel: o daemon da unit morre, o daemon novo
//! (orfao, sem supervisor) toma a porta, e o `Restart=always` da unit entra
//! em crash-loop contra `Address already in use`. Em producao (0.4.6) foram
//! 4606 reinicios em ~8h, com o log parecendo um MCP reconectando em loop
//! — a cada tentativa o boot conectava o MCP filesystem e morria no bind.
//!
//! A raiz e que `garra restart` e `systemctl restart` sao dois supervisores
//! disputando o mesmo processo. Quem sabe reiniciar uma unit e o systemd,
//! entao aqui a CLI **recusa antes de qualquer efeito colateral** e diz o
//! comando exato — em vez de delegar por conta propria, que silenciaria
//! `--host`/`--port`/`--with-voice` (o `ExecStart` da unit e que manda) e
//! trocaria um estado quebrado por um comportamento errado em silencio.
//!
//! A deteccao le o cgroup do processo, nunca o nome do binario: e o cgroup
//! que diz de quem o processo e filho de verdade.

#[cfg(test)]
use garraia_common::systemd::unidade_do_cgroup;
pub use garraia_common::systemd::{Gerente, Unidade, unidade_do_processo};

/// Escotilha do operador, no formato do `GARRAIA_ALLOW_INVALID_CONFIG`
/// (#1247): so o valor exatamente `1` libera.
pub const ENV_ESCOTILHA: &str = "GARRAIA_ALLOW_SYSTEMD_RESTART";

/// `true` quando o operador pediu explicitamente para a CLI seguir mesmo
/// assim — assumindo o orfao e o crash-loop.
pub fn escotilha_ligada() -> bool {
    valor_libera(std::env::var(ENV_ESCOTILHA).ok().as_deref())
}

/// A regra de valor, separada do ambiente para poder ser testada sem mexer
/// nas variaveis do processo (que sao globais e correm com os outros testes).
fn valor_libera(valor: Option<&str>) -> bool {
    valor == Some("1")
}

/// A recusa, ja com o comando certo. Texto puro para poder ser testado sem
/// processo nenhum no meio.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn recusa(unidade: &Unidade, pid: u32, porta: u16, bin: &str) -> String {
    format!(
        "{bin}: recusando o restart — o daemon na porta {porta} (PID {pid}) pertence a unit \
         systemd `{nome}`.\n\n\
         Parar esse processo por fora deixaria a porta com um daemon orfao e a unit em \
         crash-loop contra `Address already in use` (#1542). Quem reinicia uma unit e o \
         systemd:\n\n    \
         {restart}\n\n\
         Para rodar o `{bin}` com flags proprias, tire o daemon do supervisor antes:\n\n    \
         {stop}\n    \
         {bin} start\n\n\
         Se voce sabe o que esta fazendo e quer o comportamento antigo: {env}=1.",
        nome = unidade.nome,
        restart = unidade.comando_restart(),
        stop = unidade.comando_stop(),
        env = ENV_ESCOTILHA,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2_UNIT_USUARIO: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/garraia.service\n";

    /// A recusa tem de ser acionavel sozinha: o comando certo, o PID, a porta
    /// e a saida de emergencia.
    #[test]
    fn a_recusa_traz_o_comando_o_pid_e_a_escotilha() {
        let u = unidade_do_cgroup(V2_UNIT_USUARIO).expect("unit");
        let texto = recusa(&u, 2245, 3888, "garraia");
        assert!(
            texto.contains("systemctl --user restart garraia.service"),
            "{texto}"
        );
        assert!(texto.contains("PID 2245"), "{texto}");
        assert!(texto.contains("porta 3888"), "{texto}");
        assert!(
            texto.contains("systemctl --user stop garraia.service"),
            "{texto}"
        );
        assert!(texto.contains("GARRAIA_ALLOW_SYSTEMD_RESTART=1"), "{texto}");
        // Nao promete delegar: a CLI nao roda systemctl por ninguem.
        assert!(!texto.contains("reiniciando"), "{texto}");
    }

    /// Mesma regra do `GARRAIA_ALLOW_INVALID_CONFIG` (#1247): `1` e so `1`.
    /// `true`/`yes`/`0`/vazio nao liberam — uma escotilha que aceita qualquer
    /// coisa acaba ligada por acidente.
    #[test]
    fn a_escotilha_so_aceita_exatamente_1() {
        assert!(valor_libera(Some("1")));
        for nao in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("yes"),
            Some(" 1"),
        ] {
            assert!(!valor_libera(nao), "{nao:?} nao devia liberar");
        }
    }
}
