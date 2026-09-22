//! O `garraia config check` no boot (#1247).
//!
//! Todo `garraia start` / `restart` / `start -d` roda o MESMO
//! [`crate::run_check`] do comando, uma vez, e este modulo decide o que fazer
//! com o resultado — de forma pura, sem ler o ambiente nem imprimir nada:
//!
//! - cada `Error` e cada `Warning` e reportado uma vez (a CLI loga), com um
//!   resumo apontando para `garraia config check`;
//! - o boot so e **recusado** por um `Error` cujo campo esta na lista fechada
//!   [`BLOQUEIA_O_BOOT`]: os defeitos que falham ABERTO sem nenhuma camada
//!   depois para pegar. Todo outro `Error` (uma entrada `llm` sem chave, por
//!   exemplo — comum em configs que funcionam hoje) so e dito: recusar boot
//!   neles quebraria instalacoes reais na primeira atualizacao;
//! - a escotilha [`ESCAPE_ENV`] (`GARRAIA_ALLOW_INVALID_CONFIG=1`, exatamente
//!   `"1"`) deixa subir mesmo assim, e os achados bloqueantes continuam sendo
//!   reportados como erro nomeando a env — nada e silenciado.
//!
//! # O que fica de fora no boot
//!
//! Os achados de `gateway.host` / `gateway.port`. O check os calcula pela env
//! e pelo default do clap, sem ver a flag `--host`/`--port` deste mesmo
//! `start`; e as chaves do arquivo estao deprecadas (#1261). No boot quem fala
//! do bind e a recusa do #1261 (`crate::bind::verificar`), sobre o bind real.
//! Duas vozes que podem se contradizer sao piores que uma certa.
//!
//! # Redacao
//!
//! Os textos sao os `message` dos [`Finding`], que o invariante do modulo
//! `check` garante sem segredo (so presenca). Nada aqui formata valor de
//! config.

use crate::check::{ConfigCheck, Finding, Severity};

/// Escotilha: com o valor exato `"1"` o boot sobe mesmo com um achado
/// bloqueante. Qualquer outro valor (`true`, `0`, `" 1"`, vazio) conta como
/// ausente — fail-closed.
pub const ESCAPE_ENV: &str = "GARRAIA_ALLOW_INVALID_CONFIG";

/// sysexits `EX_CONFIG`: o exit code da recusa.
pub const EX_CONFIG: i32 = 78;

/// Os campos cujo `Error` recusa o boot. Lista fechada: cresce item a item,
/// cada um com entrada no changelog.
///
/// v0.4.5: o par de TLS pela metade. O operador pediu TLS e, com so um dos
/// caminhos, o gateway servia HTTP puro em silencio — falha aberta de
/// seguranca sem nenhuma camada depois. Os nomes sao os campos que o check
/// usa (`check.rs`, validacao de TLS); o teste
/// `tls_pela_metade_recusa_pelo_campo_real_do_check` monta o achado pelo
/// check de verdade para que nao divirjam.
pub const BLOQUEIA_O_BOOT: &[&str] = &["gateway.tls_cert_path", "gateway.tls_key_path"];

/// Campos que o boot nao repete (ver o docblock do modulo).
const FORA_DO_BOOT: &[&str] = &["gateway.host", "gateway.port"];

/// O que o boot faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Veredito {
    /// Nenhum achado bloqueante: sobe (e reporta o resto).
    Sobe,
    /// Havia achado bloqueante e a escotilha estava ligada: sobe, e os
    /// bloqueantes sao reportados como erro nomeando [`ESCAPE_ENV`].
    SobeComEscape,
    /// Achado bloqueante sem escotilha: recusa com [`EX_CONFIG`].
    Recusa,
}

/// O relatorio do boot, ja filtrado.
#[derive(Debug, Clone)]
pub struct RelatorioDoBoot {
    pub veredito: Veredito,
    /// Todos os `Error` que o boot reporta (bloqueantes inclusive).
    pub erros: Vec<Finding>,
    /// Todos os `Warning` que o boot reporta.
    pub avisos: Vec<Finding>,
    /// O subconjunto de `erros` que esta em [`BLOQUEIA_O_BOOT`].
    pub bloqueantes: Vec<Finding>,
}

/// Le a escotilha a partir do valor cru da env. So `"1"` liga.
pub fn escape_de(valor: Option<&str>) -> bool {
    valor == Some("1")
}

/// [`escape_de`] sobre a env real do processo.
pub fn escape_do_ambiente() -> bool {
    escape_de(std::env::var(ESCAPE_ENV).ok().as_deref())
}

/// A decisao, pura: dado o resultado do check e a escotilha, o que o boot faz
/// e o que ele reporta.
pub fn avaliar(check: &ConfigCheck, escape: bool) -> RelatorioDoBoot {
    let no_boot = |f: &&Finding| !FORA_DO_BOOT.contains(&f.field.as_str());
    let erros: Vec<Finding> = check
        .findings
        .iter()
        .filter(no_boot)
        .filter(|f| f.severity == Severity::Error)
        .cloned()
        .collect();
    let avisos: Vec<Finding> = check
        .findings
        .iter()
        .filter(no_boot)
        .filter(|f| f.severity == Severity::Warning)
        .cloned()
        .collect();
    let bloqueantes: Vec<Finding> = erros
        .iter()
        .filter(|f| BLOQUEIA_O_BOOT.contains(&f.field.as_str()))
        .cloned()
        .collect();
    let veredito = match (bloqueantes.is_empty(), escape) {
        (true, _) => Veredito::Sobe,
        (false, true) => Veredito::SobeComEscape,
        (false, false) => Veredito::Recusa,
    };
    RelatorioDoBoot {
        veredito,
        erros,
        avisos,
        bloqueantes,
    }
}

impl RelatorioDoBoot {
    /// `true` quando nao ha nada a dizer no boot.
    pub fn vazio(&self) -> bool {
        self.erros.is_empty() && self.avisos.is_empty()
    }

    /// Uma linha por achado, com a severidade: o que a CLI loga (`error!` /
    /// `warn!`) e, no `start -d`, imprime em stderr antes do fork.
    pub fn linhas(&self, bin: &str) -> Vec<(Severity, String)> {
        let mut linhas: Vec<(Severity, String)> = Vec::new();
        for f in &self.erros {
            let bloqueante = self.bloqueantes.iter().any(|b| b.field == f.field);
            let sufixo = if bloqueante && self.veredito == Veredito::SobeComEscape {
                format!(" (boot allowed anyway by {ESCAPE_ENV}=1)")
            } else {
                String::new()
            };
            linhas.push((
                Severity::Error,
                format!("config error [{}]: {}{sufixo}", f.field, f.message),
            ));
        }
        for f in &self.avisos {
            linhas.push((
                Severity::Warning,
                format!("config warning [{}]: {}", f.field, f.message),
            ));
        }
        if !self.vazio() {
            linhas.push((
                if self.erros.is_empty() {
                    Severity::Warning
                } else {
                    Severity::Error
                },
                self.resumo(bin),
            ));
        }
        linhas
    }

    /// A linha de resumo.
    pub fn resumo(&self, bin: &str) -> String {
        format!(
            "{} config error(s), {} warning(s) — run `{bin} config check` for details",
            self.erros.len(),
            self.avisos.len()
        )
    }

    /// A mensagem da recusa: os achados bloqueantes, a correcao e a
    /// escotilha.
    pub fn mensagem_de_recusa(&self, bin: &str) -> String {
        let mut s = String::from(
            "refusing to start: the config has errors that would fail open at boot:\n",
        );
        for f in &self.bloqueantes {
            s.push_str(&format!("  - [{}] {}\n", f.field, f.message));
        }
        s.push_str(&format!(
            "Fix them (run `{bin} config check` for the full report), or set \
             {ESCAPE_ENV}=1 to boot anyway — the findings are still logged as errors."
        ));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn achado(sev: Severity, campo: &str) -> Finding {
        Finding {
            severity: sev,
            field: campo.into(),
            message: format!("mensagem de {campo}"),
        }
    }

    fn check_com(findings: Vec<Finding>) -> ConfigCheck {
        // `run_check` le env; os testes de `check`/`auth` a mutam sob este lock.
        let _g = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let loader = crate::ConfigLoader::with_dir(std::env::temp_dir().join("garraia-bg-nada"));
        let mut c = crate::run_check(&loader, &crate::AppConfig::default());
        c.findings = findings;
        c
    }

    #[test]
    fn check_vazio_sobe_sem_nada_a_dizer() {
        let r = avaliar(&check_com(vec![]), false);
        assert_eq!(r.veredito, Veredito::Sobe);
        assert!(r.vazio());
        assert!(r.linhas("garraia").is_empty());
    }

    #[test]
    fn error_fora_da_allowlist_sobe_e_e_reportado() {
        let r = avaliar(
            &check_com(vec![achado(Severity::Error, "llm.main.api_key")]),
            false,
        );
        assert_eq!(r.veredito, Veredito::Sobe);
        assert_eq!(r.erros.len(), 1);
        assert!(r.bloqueantes.is_empty());
        let linhas = r.linhas("garraia");
        assert!(linhas[0].1.contains("llm.main.api_key"));
        assert!(
            linhas
                .last()
                .is_some_and(|(_, l)| l.contains("`garraia config check`"))
        );
    }

    /// O achado vem do check DE VERDADE, para que o nome do campo nao possa
    /// divergir de `BLOQUEIA_O_BOOT` sem este teste quebrar.
    #[test]
    fn tls_pela_metade_recusa_pelo_campo_real_do_check() {
        // `run_check` le env; os testes de `check`/`auth` a mutam sob este lock.
        let _g = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (cert, key) in [(Some("/c.pem"), None), (None, Some("/k.pem"))] {
            let mut cfg = crate::AppConfig::default();
            cfg.gateway.tls_cert_path = cert.map(str::to_string);
            cfg.gateway.tls_key_path = key.map(str::to_string);
            let loader = crate::ConfigLoader::with_dir(std::env::temp_dir().join("garraia-bg-tls"));
            let check = crate::run_check(&loader, &cfg);
            let r = avaliar(&check, false);
            assert_eq!(r.veredito, Veredito::Recusa, "{:?}", check.findings);
            let msg = r.mensagem_de_recusa("garraia");
            assert!(msg.contains(ESCAPE_ENV), "{msg}");
            assert!(msg.contains("`garraia config check`"), "{msg}");
            assert!(
                msg.contains("gateway.tls_key_path") || msg.contains("gateway.tls_cert_path"),
                "{msg}"
            );

            // Com a escotilha sobe, e o bloqueante segue dito como erro.
            let r = avaliar(&check, true);
            assert_eq!(r.veredito, Veredito::SobeComEscape);
            assert!(!r.bloqueantes.is_empty());
            assert!(
                r.linhas("garraia")
                    .iter()
                    .any(|(s, l)| *s == Severity::Error && l.contains(ESCAPE_ENV))
            );
        }
    }

    #[test]
    fn bind_fica_de_fora_do_boot() {
        let r = avaliar(
            &check_com(vec![
                achado(Severity::Warning, "gateway.host"),
                achado(Severity::Error, "gateway.host"),
                achado(Severity::Warning, "gateway.port"),
                achado(Severity::Error, "gateway.port"),
            ]),
            false,
        );
        assert!(r.vazio(), "{r:?}");
        assert_eq!(r.veredito, Veredito::Sobe);
    }

    #[test]
    fn escotilha_so_liga_com_um_exato() {
        assert!(escape_de(Some("1")));
        for v in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some(" 1"),
            Some("1 "),
            Some("yes"),
        ] {
            assert!(!escape_de(v), "{v:?}");
        }
    }

    /// Nenhum segredo chega as linhas do boot, mesmo com chaves preenchidas.
    #[test]
    fn linhas_do_boot_nunca_carregam_segredo() {
        // `run_check` le env; os testes de `check`/`auth` a mutam sob este lock.
        let _g = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut cfg = crate::AppConfig::default();
        cfg.gateway.api_key = Some("segredo-do-gateway-1247".into());
        cfg.gateway.tls_cert_path = Some("/c.pem".into());
        cfg.llm.insert(
            "main".into(),
            crate::model::LlmProviderConfig {
                provider: "anthropic".into(),
                model: None,
                api_key: Some("sk-ant-segredo-1247".into()),
                base_url: None,
                extra: Default::default(),
            },
        );
        let loader = crate::ConfigLoader::with_dir(std::env::temp_dir().join("garraia-bg-red"));
        let check = crate::run_check(&loader, &cfg);
        for escape in [false, true] {
            let r = avaliar(&check, escape);
            let tudo = format!(
                "{:?}{}",
                r.linhas("garraia"),
                r.mensagem_de_recusa("garraia")
            );
            assert!(!tudo.contains("segredo-do-gateway-1247"), "{tudo}");
            assert!(!tudo.contains("sk-ant-segredo-1247"), "{tudo}");
        }
    }
}
