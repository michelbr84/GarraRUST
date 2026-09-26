//! O que cada principal **pode de fato** — calculado pelo `ToolGate` real, o
//! mesmo do turno (#1400, #1413).
//!
//! Nao existe um segundo modelo: [`efetivo`] monta o portao do turno (piso de
//! modo + teto do principal) e pergunta a ele por sete ferramentas
//! representativas, uma por classe que o operador reconhece. [`matriz`] faz
//! isso para todo principal que a politica descreve; [`diferencas`] compara
//! duas politicas e nomeia o que cada principal ganha e perde — e o preview
//! do `--dry-run` e o corpo do audit.

use garraia_agents::capacidades::{capacidades_da_operacao_mcp, capacidades_nativas};
use garraia_agents::modes::ToolGate;
use garraia_config::ExecutionProfile;

use super::super::{DEFAULT_MODE, LinkedSettings, modo_padrao};
use super::mutacao::mascarar;
use super::{Admission, Alcance, Principal, teto_do_principal};

/// As classes que o operador reconhece, cada uma provada por uma ferramenta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capacidades {
    /// `file_read`
    pub leitura: bool,
    /// `file_write`
    pub escrita: bool,
    /// `bash`
    pub shell: bool,
    /// `device_execute`
    pub dispositivo: bool,
    /// `telegram_send`
    pub mensagem: bool,
    /// `filesystem__read_file`
    pub mcp_leitura: bool,
    /// `filesystem__write_file`
    pub mcp_escrita: bool,
}

impl Capacidades {
    /// Os nomes do que esta ligado, na ordem da tabela.
    pub fn ligadas(&self) -> Vec<&'static str> {
        [
            (self.leitura, "leitura de arquivo"),
            (self.escrita, "escrita de arquivo"),
            (self.shell, "shell"),
            (self.dispositivo, "dispositivo (executar)"),
            (self.mensagem, "enviar mensagem"),
            (self.mcp_leitura, "MCP leitura"),
            (self.mcp_escrita, "MCP escrita"),
        ]
        .into_iter()
        .filter_map(|(ligada, nome)| ligada.then_some(nome))
        .collect()
    }
}

/// O que um principal pode, num perfil de execucao.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Efetivo {
    pub principal: &'static str,
    /// `None` para dono (sem teto) e nao-admitido.
    pub alcance: Option<Alcance>,
    /// O piso de modo do turno (`search`, `code`, ...) ou `—` para quem nao entra.
    pub modo: String,
    pub capacidades: Capacidades,
}

/// Uma linha de [`matriz`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinhaDaMatriz {
    pub principal: &'static str,
    /// `…1234` (numero, id de `@lid` ou JID de grupo), ou `None` para o
    /// pareado, o desconhecido e o default de grupo.
    pub alvo: Option<String>,
    pub efetivo: Efetivo,
}

/// O que um principal ganha e perde entre duas politicas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diferenca {
    pub principal: &'static str,
    pub alvo: Option<String>,
    pub ganha: Vec<&'static str>,
    pub perde: Vec<&'static str>,
}

/// O que aparece na lista quando um principal passa a existir (ou some).
const CONVERSAR: &str = "conversar";

/// O efetivo de um principal: piso de modo (ADR 0024) + teto (ADR 0025),
/// perguntados ao `ToolGate` real.
pub fn efetivo(
    settings: &LinkedSettings,
    perfil: ExecutionProfile,
    principal: Principal,
    is_group: bool,
) -> Efetivo {
    if !principal.admitido() {
        return Efetivo {
            principal: principal.as_str(),
            alcance: None,
            modo: "—".to_string(),
            capacidades: Capacidades::default(),
        };
    }
    // O mesmo criterio de `perfil_do_turno`/`modo_do_piso`: so o dono, em
    // 1:1, num processo em pod, recebe o piso do pod.
    let perfil_do_piso = if principal == Principal::Dono && perfil.is_isolated_pod() && !is_group {
        ExecutionProfile::IsolatedPod
    } else {
        ExecutionProfile::Standard
    };
    let declarado = settings.modo_padrao_efetivo(perfil_do_piso);
    let modo = modo_padrao(&declarado)
        .map_or(DEFAULT_MODE, |m| m.as_str())
        .to_string();
    let gate = ToolGate::for_mode_name(&modo).com_teto(teto_do_principal(&principal));
    let nativa = |nome: &str| gate.permite_com_capacidades(nome, capacidades_nativas(nome));
    let mcp = |nome: &str, operacao: &str| {
        gate.permite_com_capacidades(nome, capacidades_da_operacao_mcp(operacao, None, None))
    };
    Efetivo {
        principal: principal.as_str(),
        alcance: principal.alcance(),
        modo,
        capacidades: Capacidades {
            leitura: nativa("file_read"),
            escrita: nativa("file_write"),
            shell: nativa("bash"),
            dispositivo: nativa("device_execute"),
            mensagem: nativa("telegram_send"),
            mcp_leitura: mcp("filesystem__read_file", "read_file"),
            mcp_escrita: mcp("filesystem__write_file", "write_file"),
        },
    }
}

/// Todo principal que a politica descreve, com o seu efetivo: cada
/// identidade declarada (dono, usuario, bloqueado), o pareado, o
/// desconhecido (so com `open`) e os grupos (so quando respondem).
pub fn matriz(settings: &LinkedSettings, perfil: ExecutionProfile) -> Vec<LinhaDaMatriz> {
    let mut linhas = Vec::new();
    let mut chaves: Vec<String> = settings.chaves_declaradas().into_iter().collect();
    chaves.sort();
    for chave in &chaves {
        let Some(entrada) = settings.entrada_de(chave) else {
            continue;
        };
        let principal = if entrada.bloqueado {
            Principal::Bloqueado
        } else if entrada.dono {
            Principal::Dono
        } else {
            Principal::Usuario(entrada.alcance)
        };
        linhas.push(LinhaDaMatriz {
            principal: principal.as_str(),
            alvo: Some(mascarar(chave)),
            efetivo: efetivo(settings, perfil, principal, false),
        });
    }
    linhas.push(LinhaDaMatriz {
        principal: Principal::Pareado.as_str(),
        alvo: None,
        efetivo: efetivo(settings, perfil, Principal::Pareado, false),
    });
    if settings.access.admission == Admission::Open {
        let p = Principal::Desconhecido(settings.access.default);
        linhas.push(LinhaDaMatriz {
            principal: p.as_str(),
            alvo: None,
            efetivo: efetivo(settings, perfil, p, false),
        });
    }
    if settings.responde_em_grupo() {
        let p = Principal::Grupo(settings.access.groups.default);
        linhas.push(LinhaDaMatriz {
            principal: p.as_str(),
            alvo: None,
            efetivo: efetivo(settings, perfil, p, true),
        });
        for (jid, alcance) in &settings.access.groups.por_grupo {
            let p = Principal::Grupo(*alcance);
            linhas.push(LinhaDaMatriz {
                principal: p.as_str(),
                alvo: Some(mascarar(jid)),
                efetivo: efetivo(settings, perfil, p, true),
            });
        }
    }
    linhas
}

/// O que muda, principal a principal, entre `antes` e `depois`. Vazio quando
/// nada muda de fato — mesmo que a config tenha mudado.
pub fn diferencas(
    antes: &LinkedSettings,
    depois: &LinkedSettings,
    perfil: ExecutionProfile,
) -> Vec<Diferenca> {
    let a = matriz(antes, perfil);
    let d = matriz(depois, perfil);
    let chave = |l: &LinhaDaMatriz| (l.principal, l.alvo.clone());
    let mut chaves: Vec<(&'static str, Option<String>)> = d.iter().map(chave).collect();
    for l in &a {
        let k = chave(l);
        if !chaves.contains(&k) {
            chaves.push(k);
        }
    }
    let ligadas = |linhas: &[LinhaDaMatriz], k: &(&str, Option<String>)| {
        linhas
            .iter()
            .find(|l| l.principal == k.0 && l.alvo == k.1)
            .map(|l| {
                let mut v = Vec::new();
                if l.efetivo.modo != "—" {
                    v.push(CONVERSAR);
                }
                v.extend(l.efetivo.capacidades.ligadas());
                v
            })
    };
    let mut out = Vec::new();
    for k in chaves {
        let ca = ligadas(&a, &k).unwrap_or_default();
        let cd = ligadas(&d, &k).unwrap_or_default();
        let ganha: Vec<&'static str> = cd.iter().copied().filter(|c| !ca.contains(c)).collect();
        let perde: Vec<&'static str> = ca.iter().copied().filter(|c| !cd.contains(c)).collect();
        if ganha.is_empty() && perde.is_empty() {
            continue;
        }
        out.push(Diferenca {
            principal: k.0,
            alvo: k.1,
            ganha,
            perde,
        });
    }
    out
}
