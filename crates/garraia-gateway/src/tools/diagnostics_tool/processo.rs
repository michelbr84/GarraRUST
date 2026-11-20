//! `processo.servico` — qual processo responde e quem o supervisiona? (#1914)
//!
//! `versao.instalada` da a versao e o uptime, `versao.executavel` prova qual
//! build esta carregado; faltava dizer QUAL processo e esse e se ha um
//! supervisor que o reinicia. Tudo por interface nativa e somente leitura:
//!
//! - **PID**: `std::process::id()`.
//! - **inicio**: o relogio de parede de agora menos o uptime do `AppState`,
//!   em UTC ISO 8601 com `Z` (convencao de timestamp de API do projeto),
//!   truncado no segundo. E o inicio do GATEWAY neste processo — um restart
//!   da unit gera PID e inicio novos, e e assim que se compara antes/depois.
//! - **supervisor**: a unit systemd dona do processo, lida do proprio cgroup
//!   (`/proc/self/cgroup`) pelo mesmo parser que o `garra restart` usa
//!   (`garraia_common::systemd`) — escopo `usuario` (`systemctl --user`) ou
//!   `sistema`. Nunca roda `systemctl`, nunca aceita argumento.
//!
//! ## O que conta como prova
//!
//! cgroup lido e sem unit = `nenhum` (processo de terminal / `start -d`,
//! container sem systemd): sem reinicio automatico, e isso e um fato
//! observado. cgroup ilegivel e `nao_verificado` com motivo e sai `erro`,
//! como toda leitura que falhou no relatorio — nunca "sem supervisor". Fora
//! do Linux nao ha systemd: `nao_suportado`. Contagem de reinicios da unit
//! (`NRestarts`) so sai do `systemctl`/D-Bus e fica fora desta entrada.

use chrono::{DateTime, SecondsFormat, Utc};

use super::{EstadoDiag, Verificacao};

/// Id estavel da entrada.
pub(crate) const ID: &str = "processo.servico";

/// O que se conseguiu ler do cgroup deste processo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cgroup {
    /// Conteudo de `/proc/self/cgroup`.
    Lido(String),
    /// Leitura falhou (procfs bloqueado, permissao).
    NaoVerificado { motivo: &'static str },
    /// Plataforma sem cgroup/systemd.
    NaoSuportado,
}

impl Cgroup {
    /// O cgroup deste processo, lido agora.
    pub(crate) fn deste_processo() -> Self {
        if !cfg!(target_os = "linux") {
            return Self::NaoSuportado;
        }
        match std::fs::read_to_string("/proc/self/cgroup") {
            Ok(c) => Self::Lido(c),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Self::NaoVerificado {
                motivo: "acesso_negado",
            },
            Err(_) => Self::NaoVerificado {
                motivo: "falha_de_leitura",
            },
        }
    }
}

/// A entrada, pura: sem relogio nem I/O proprios.
pub(crate) fn verificacao(
    pid: u32,
    uptime_secs: u64,
    agora: DateTime<Utc>,
    cgroup: &Cgroup,
) -> Verificacao {
    let inicio = i64::try_from(uptime_secs)
        .ok()
        .and_then(chrono::Duration::try_seconds)
        .and_then(|d| agora.checked_sub_signed(d))
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true));

    let (estado, supervisor, detalhe) = match cgroup {
        Cgroup::Lido(conteudo) => match garraia_common::systemd::unidade_do_cgroup(conteudo) {
            Some(u) => {
                let escopo = match u.gerente {
                    garraia_common::systemd::Gerente::Usuario => "usuario",
                    garraia_common::systemd::Gerente::Sistema => "sistema",
                };
                let detalhe = format!(
                    "PID {pid}, supervisionado pela unit systemd `{}` (escopo {escopo}); \
                     reinicio e com `{}`.",
                    u.nome,
                    u.comando_restart()
                );
                (
                    EstadoDiag::Ok,
                    serde_json::json!({
                        "tipo": "systemd",
                        "escopo": escopo,
                        "unidade": u.nome,
                    }),
                    detalhe,
                )
            }
            None => (
                EstadoDiag::Ok,
                serde_json::json!({ "tipo": "nenhum" }),
                format!(
                    "PID {pid}, fora de qualquer unit systemd (terminal, `start -d` ou \
                     container): nada o reinicia se cair."
                ),
            ),
        },
        Cgroup::NaoVerificado { motivo } => (
            EstadoDiag::Erro,
            serde_json::json!({ "tipo": "nao_verificado", "motivo": motivo }),
            format!(
                "PID {pid}; supervisor nao verificado: o cgroup do processo nao pode ser lido \
                 (veja `motivo`). Leitura negada nao prova que nao ha supervisor."
            ),
        ),
        Cgroup::NaoSuportado => (
            EstadoDiag::NaoSuportado,
            serde_json::json!({ "tipo": "nao_suportado" }),
            format!("PID {pid}; esta plataforma nao tem systemd para supervisionar o processo."),
        ),
    };

    Verificacao::nova(
        ID,
        estado,
        serde_json::json!({
            "pid": pid,
            "iniciado_em": inicio,
            "uptime_secs": uptime_secs,
            "supervisor": supervisor,
        }),
        detalhe,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agora() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-20T15:00:00Z")
            .expect("data fixa")
            .with_timezone(&Utc)
    }

    const UNIT_USUARIO: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/garraia.service\n";

    #[test]
    fn unit_do_usuario_sai_com_escopo_e_comando_user() {
        let v = verificacao(4242, 90, agora(), &Cgroup::Lido(UNIT_USUARIO.to_string()));
        assert_eq!(v.id, ID);
        assert_eq!(v.estado, EstadoDiag::Ok);
        assert_eq!(v.evidencia["pid"], 4242);
        assert_eq!(v.evidencia["iniciado_em"], "2026-11-20T14:58:30Z");
        assert_eq!(v.evidencia["uptime_secs"], 90);
        assert_eq!(v.evidencia["supervisor"]["tipo"], "systemd");
        assert_eq!(v.evidencia["supervisor"]["escopo"], "usuario");
        assert_eq!(v.evidencia["supervisor"]["unidade"], "garraia.service");
        assert!(
            v.detalhe
                .contains("systemctl --user restart garraia.service"),
            "{}",
            v.detalhe
        );
    }

    #[test]
    fn unit_do_sistema_sai_com_escopo_sistema() {
        let v = verificacao(
            7,
            0,
            agora(),
            &Cgroup::Lido("0::/system.slice/garraia.service\n".to_string()),
        );
        assert_eq!(v.estado, EstadoDiag::Ok);
        assert_eq!(v.evidencia["supervisor"]["escopo"], "sistema");
        assert_eq!(v.evidencia["iniciado_em"], "2026-11-20T15:00:00Z");
        assert!(
            v.detalhe.contains("sudo systemctl restart"),
            "{}",
            v.detalhe
        );
    }

    /// Terminal / `start -d`: um `.scope` nao e unit, e isso e fato lido.
    #[test]
    fn scope_de_terminal_e_sem_supervisor_e_nao_erro() {
        let v = verificacao(
            1,
            5,
            agora(),
            &Cgroup::Lido("0::/user.slice/user-1000.slice/session-3.scope\n".to_string()),
        );
        assert_eq!(v.estado, EstadoDiag::Ok);
        assert_eq!(v.evidencia["supervisor"]["tipo"], "nenhum");
        assert!(v.detalhe.contains("nada o reinicia"), "{}", v.detalhe);
    }

    /// Leitura negada nunca vira "sem supervisor".
    #[test]
    fn cgroup_ilegivel_e_nao_verificado_e_erro() {
        let v = verificacao(
            1,
            5,
            agora(),
            &Cgroup::NaoVerificado {
                motivo: "acesso_negado",
            },
        );
        assert_eq!(v.estado, EstadoDiag::Erro);
        assert_eq!(v.evidencia["supervisor"]["tipo"], "nao_verificado");
        assert_eq!(v.evidencia["supervisor"]["motivo"], "acesso_negado");
        assert_ne!(v.evidencia["supervisor"]["tipo"], "nenhum");
        // O PID e o inicio continuam provados.
        assert_eq!(v.evidencia["pid"], 1);
        assert_eq!(v.evidencia["iniciado_em"], "2026-11-20T14:59:55Z");
    }

    #[test]
    fn fora_do_linux_e_nao_suportado() {
        let v = verificacao(1, 5, agora(), &Cgroup::NaoSuportado);
        assert_eq!(v.estado, EstadoDiag::NaoSuportado);
        assert_eq!(v.evidencia["supervisor"]["tipo"], "nao_suportado");
    }

    /// Antes/depois de um restart: PID e inicio mudam, e e isso que o
    /// operador compara.
    #[test]
    fn restart_muda_pid_e_inicio() {
        let antes = verificacao(100, 3600, agora(), &Cgroup::Lido(UNIT_USUARIO.into()));
        let depois = verificacao(200, 10, agora(), &Cgroup::Lido(UNIT_USUARIO.into()));
        assert_ne!(antes.evidencia["pid"], depois.evidencia["pid"]);
        assert_ne!(
            antes.evidencia["iniciado_em"],
            depois.evidencia["iniciado_em"]
        );
    }

    /// Uptime absurdo nao derruba a entrada: o inicio fica `null`.
    #[test]
    fn uptime_fora_da_faixa_nao_entra_em_panico() {
        let v = verificacao(1, u64::MAX, agora(), &Cgroup::NaoSuportado);
        assert!(v.evidencia["iniciado_em"].is_null());
    }

    /// Nenhum caminho de cgroup sai no relatorio — so o nome da unit.
    #[test]
    fn caminho_do_cgroup_nao_vaza() {
        let v = verificacao(1, 5, agora(), &Cgroup::Lido(UNIT_USUARIO.into()));
        let texto = format!("{} {}", v.evidencia, v.detalhe);
        assert!(!texto.contains("user.slice"), "{texto}");
        assert!(!texto.contains("user@1000"), "{texto}");
    }

    /// No host de teste (Linux) a leitura real funciona e o parser roda.
    #[cfg(target_os = "linux")]
    #[test]
    fn leitura_real_do_proprio_cgroup() {
        let c = Cgroup::deste_processo();
        assert!(
            matches!(c, Cgroup::Lido(_) | Cgroup::NaoVerificado { .. }),
            "{c:?}"
        );
    }
}
