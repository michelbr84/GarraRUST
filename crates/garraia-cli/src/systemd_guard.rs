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

/// Qual gerenciador systemd dona a unit — muda o comando que o operador
/// precisa rodar (`--user` ou nao) e se ele vai precisar de root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gerente {
    /// `systemctl --user` — unit do proprio usuario (`~/.config/systemd/user`).
    Usuario,
    /// `systemctl` — unit do sistema, precisa de privilegio.
    Sistema,
}

/// A unit systemd que dona o processo encontrado na porta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unidade {
    pub nome: String,
    pub gerente: Gerente,
}

impl Unidade {
    /// O comando que de fato reinicia este daemon.
    pub fn comando_restart(&self) -> String {
        match self.gerente {
            Gerente::Usuario => format!("systemctl --user restart {}", self.nome),
            Gerente::Sistema => format!("sudo systemctl restart {}", self.nome),
        }
    }

    /// O comando que tira o daemon do supervisor, para quem realmente quer
    /// rodar `garra restart` com flags proprias.
    pub fn comando_stop(&self) -> String {
        match self.gerente {
            Gerente::Usuario => format!("systemctl --user stop {}", self.nome),
            Gerente::Sistema => format!("sudo systemctl stop {}", self.nome),
        }
    }
}

/// Parser puro de `/proc/<pid>/cgroup` — sem relogio, sem I/O, sem processo.
///
/// Formatos aceitos: cgroup v2 (`0::/caminho`) e a hierarquia `name=systemd`
/// do v1 (`N:name=systemd:/caminho`). As demais linhas do v1 sao de
/// controlador (`cpu`, `memory`, …) e nao descrevem a unit.
///
/// Devolve `None` quando o processo **nao** e de uma unit gerenciada:
///
/// - ultimo componente e um `.scope` — sessao transiente (o terminal de onde
///   alguem rodou `garra start -d`). Reiniciar ai e exatamente o que a CLI
///   sempre fez e continua certo;
/// - ultimo componente e o `user@<uid>.service` — esse e o *gerente* do
///   usuario, nao o daemon; matar ou reiniciar derrubaria a sessao inteira;
/// - nao ha componente de unit nenhum.
pub fn unidade_do_cgroup(conteudo: &str) -> Option<Unidade> {
    for linha in conteudo.lines() {
        let Some((prefixo, caminho)) = linha.rsplit_once(':') else {
            continue;
        };
        // v2: prefixo "0:". v1: prefixo "N:name=systemd".
        if prefixo != "0:" && !prefixo.ends_with("name=systemd") {
            continue;
        }
        let Some(ultimo) = caminho.split('/').rfind(|c| !c.is_empty()) else {
            continue;
        };
        if !ultimo.ends_with(".service") || ultimo.starts_with("user@") {
            continue;
        }
        let gerente = if caminho.contains("/user@") || caminho.contains("/user.slice/") {
            Gerente::Usuario
        } else {
            Gerente::Sistema
        };
        return Some(Unidade {
            nome: ultimo.to_string(),
            gerente,
        });
    }
    None
}

/// Le o cgroup de um PID vivo. Fora do Linux nao ha systemd: `None`.
#[cfg(target_os = "linux")]
pub fn unidade_do_processo(pid: u32) -> Option<Unidade> {
    let conteudo = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    unidade_do_cgroup(&conteudo)
}

#[cfg(not(target_os = "linux"))]
pub fn unidade_do_processo(_pid: u32) -> Option<Unidade> {
    None
}

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

    #[test]
    fn unit_do_usuario_e_reconhecida_com_o_comando_user() {
        let u = unidade_do_cgroup(V2_UNIT_USUARIO).expect("unit");
        assert_eq!(u.nome, "garraia.service");
        assert_eq!(u.gerente, Gerente::Usuario);
        assert_eq!(
            u.comando_restart(),
            "systemctl --user restart garraia.service"
        );
    }

    #[test]
    fn unit_do_sistema_pede_privilegio() {
        let u = unidade_do_cgroup("0::/system.slice/garraia.service\n").expect("unit");
        assert_eq!(u.gerente, Gerente::Sistema);
        assert_eq!(
            u.comando_restart(),
            "sudo systemctl restart garraia.service"
        );
    }

    /// O caso que sempre funcionou e tem de continuar funcionando: `start -d`
    /// disparado de um terminal herda o `.scope` da sessao, que nao e unit.
    #[test]
    fn scope_de_sessao_nao_e_unit() {
        assert_eq!(
            unidade_do_cgroup(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/\
                 app-gnome-terminal-3f2a.scope\n"
            ),
            None
        );
        assert_eq!(unidade_do_cgroup("0::/init.scope\n"), None);
    }

    /// `user@1000.service` e o gerente da sessao. Mandar reiniciar isso
    /// derrubaria tudo do usuario — nao e o daemon.
    #[test]
    fn o_gerente_do_usuario_nao_e_confundido_com_o_daemon() {
        assert_eq!(
            unidade_do_cgroup("0::/user.slice/user-1000.slice/user@1000.service\n"),
            None
        );
    }

    #[test]
    fn cgroup_v1_usa_a_hierarquia_name_systemd_e_ignora_as_de_controlador() {
        let v1 = "12:memory:/user.slice\n\
                  5:cpu,cpuacct:/user.slice\n\
                  1:name=systemd:/user.slice/user-1000.slice/user@1000.service/garraia.service\n";
        let u = unidade_do_cgroup(v1).expect("unit");
        assert_eq!(u.nome, "garraia.service");
        assert_eq!(u.gerente, Gerente::Usuario);
    }

    #[test]
    fn cgroup_sem_nada_reconhecivel_nao_inventa_unit() {
        assert_eq!(unidade_do_cgroup(""), None);
        assert_eq!(unidade_do_cgroup("0::/\n"), None);
        assert_eq!(unidade_do_cgroup("linha sem dois pontos\n"), None);
        // Container sem systemd: caminho de docker, nenhum `.service`.
        assert_eq!(unidade_do_cgroup("0::/docker/9f2c4b1e\n"), None);
    }

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
