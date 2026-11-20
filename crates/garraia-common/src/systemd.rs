//! Qual unit systemd e dona de um processo, lido do cgroup (#1542, #1914).
//!
//! Parser puro de `/proc/<pid>/cgroup` mais a leitura do procfs. Mora aqui,
//! e nao na CLI, porque tem dois consumidores: o `systemd_guard` da
//! `garraia-cli` (recusa `garra restart` sobre uma unit) e a entrada
//! `processo.servico` do `garra_diagnostics` no gateway — um parser so. O
//! gateway le o procfs por conta propria porque precisa distinguir "leitura
//! falhou" de "sem unit", que o `Option` de [`unidade_do_processo`] junta.
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
}
