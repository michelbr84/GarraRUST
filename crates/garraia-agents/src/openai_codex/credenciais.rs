//! Credenciais do Sign in with ChatGPT em disco, como a documentacao pede
//! (`sign-in` §5 e `profiles-and-sessions`):
//!
//! ```text
//! <dir>/host.json              {"ext_agent_host_id": "urn:uuid:..."}  — estavel por host
//! <dir>/contas/<client_id>.json  um registro por conta (client id emitido)
//! <dir>/ativa.json             {"client_id": "..."}  — a conta em uso
//! <dir>/renovacao.lock         trava entre processos (gateway, MCP, CLI)
//! ```
//!
//! Tudo escrito de forma atomica com permissao `0600` (Unix), em diretorio
//! `0700`. Tokens nunca vao para log nem para `Debug`.
//!
//! A renovacao e serializada pela trava: gateway, servidor MCP e CLI podem
//! usar a mesma conta ao mesmo tempo, e o refresh token **rotaciona** — dois
//! processos renovando juntos queimariam o token um do outro.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::oauth::{self, ErroOAuth, RespostaDeToken};
use super::{ESCOPO_DO_PLANO, Endpoints};

/// Margem antes do vencimento em que o access token ja e renovado.
const MARGEM_DE_RENOVACAO_S: i64 = 120;
/// Quanto esperar pela trava de renovacao de outro processo.
const ESPERA_MAXIMA_DA_TRAVA: Duration = Duration::from_secs(30);

/// Um registro de conta, no formato do exemplo oficial (`sign-in` §5) mais
/// o nome exibivel e o `earliest_refresh_at` da resposta de token.
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistroDeCredencial {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub ext_agent_host_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default = "bearer")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Quando a resposta de token chegou (UTC, ISO 8601 com `Z`).
    pub saved_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_refresh_at: Option<serde_json::Value>,
}

fn bearer() -> String {
    "Bearer".into()
}

impl std::fmt::Debug for RegistroDeCredencial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = |v: &Option<String>| v.as_ref().map(|_| "<redigido>");
        f.debug_struct("RegistroDeCredencial")
            .field("email", &self.email)
            .field("client_id", &self.client_id)
            .field("subject", &self.subject)
            .field("id_token", &r(&self.id_token))
            .field("access_token", &r(&self.access_token))
            .field("refresh_token", &r(&self.refresh_token))
            .field("expires_in", &self.expires_in)
            .field("scopes", &self.scopes)
            .field("saved_at", &self.saved_at)
            .finish()
    }
}

impl RegistroDeCredencial {
    /// O escopo do plano foi concedido.
    pub fn plano_habilitado(&self) -> bool {
        self.scopes.iter().any(|s| s == ESCOPO_DO_PLANO)
    }

    /// Ha sessao local (access token ou refresh token guardados).
    pub fn conectado(&self) -> bool {
        self.access_token.is_some() || self.refresh_token.is_some()
    }

    /// Instante (Unix, s) em que o access token vence; `None` se nao ha
    /// token ou o `saved_at` e ilegivel.
    pub fn vence_em(&self) -> Option<i64> {
        self.access_token.as_ref()?;
        let salvo = chrono::DateTime::parse_from_rfc3339(&self.saved_at)
            .ok()?
            .timestamp();
        Some(salvo + self.expires_in as i64)
    }

    /// O access token precisa ser renovado em `agora` (vencido, perto de
    /// vencer, ou ausente com refresh token disponivel).
    pub fn precisa_renovar(&self, agora: i64) -> bool {
        match self.vence_em() {
            None => self.refresh_token.is_some(),
            Some(vence) => {
                if agora + MARGEM_DE_RENOVACAO_S < vence
                    && let Some(cedo) = self.earliest_refresh_at.as_ref().and_then(|v| v.as_i64())
                    && agora < cedo
                {
                    return false;
                }
                agora + MARGEM_DE_RENOVACAO_S >= vence
            }
        }
    }

    /// Aplica uma resposta de renovacao: troca juntos access token, validade,
    /// escopos e o refresh token rotativo (quando vier um novo).
    pub fn aplicar_renovacao(&mut self, r: &RespostaDeToken) {
        self.access_token = Some(r.access_token.clone());
        if let Some(rt) = &r.refresh_token {
            self.refresh_token = Some(rt.clone());
        }
        if let Some(id) = &r.id_token {
            self.id_token = Some(id.clone());
        }
        self.token_type = r.token_type.clone();
        self.expires_in = r.expires_in.unwrap_or(3600);
        if r.scope.is_some() {
            self.scopes = r.escopos();
        }
        self.earliest_refresh_at = r.earliest_refresh_at.clone();
        self.saved_at = agora_iso();
    }

    /// Esquece a sessao (logout ou refresh terminal), mantendo o mapeamento
    /// conta↔cliente e o host para o proximo login.
    pub fn esquecer_sessao(&mut self, manter_id_token: bool) {
        self.access_token = None;
        self.refresh_token = None;
        if !manter_id_token {
            self.id_token = None;
        }
        self.expires_in = 0;
    }
}

/// A visao sem segredos de uma conta, para status, painel e diagnostico.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContaChatGpt {
    pub email: Option<String>,
    pub nome: Option<String>,
    pub client_id: String,
    pub conectado: bool,
    pub plano_habilitado: bool,
    /// Vencimento do access token atual (UTC, ISO 8601 com `Z`).
    pub token_vence_em: Option<String>,
    pub escopos: Vec<String>,
}

impl From<&RegistroDeCredencial> for ContaChatGpt {
    fn from(r: &RegistroDeCredencial) -> Self {
        Self {
            email: r.email.clone(),
            nome: r.name.clone(),
            client_id: r.client_id.clone(),
            conectado: r.conectado(),
            plano_habilitado: r.plano_habilitado(),
            token_vence_em: r
                .vence_em()
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            escopos: r.scopes.clone(),
        }
    }
}

/// Por que nao ha access token utilizavel. As mensagens dizem o que fazer.
#[derive(Debug, thiserror::Error)]
pub enum ErroDeConta {
    #[error(
        "nenhuma conta do ChatGPT conectada ao GarraIA: entre pelo botao \"Continuar com ChatGPT\" na pagina Providers do painel ou com `garra chatgpt login`"
    )]
    NaoConectado,
    #[error(
        "a conta {conta} entrou sem permitir o uso do plano do ChatGPT (escopo chatgpt.tokens.use.direct ausente): rode `garra chatgpt login` de novo e autorize o uso do plano"
    )]
    PlanoNaoHabilitado { conta: String },
    #[error(
        "a sessao do ChatGPT de {conta} expirou ou foi desconectada (refresh recusado: {codigo}): rode `garra chatgpt login`"
    )]
    SessaoExpirada { conta: String, codigo: String },
    #[error("{0}")]
    OAuth(#[from] ErroOAuth),
    #[error("credenciais do ChatGPT em {caminho}: {detalhe}")]
    Armazenamento { caminho: String, detalhe: String },
}

impl From<ErroDeConta> for garraia_common::Error {
    fn from(e: ErroDeConta) -> Self {
        match &e {
            // Rede: o runtime trata como transporte (fallback), nao como credencial.
            ErroDeConta::OAuth(ErroOAuth::Rede { .. }) => {
                garraia_common::Error::Transport(format!("openai-codex: {e}"))
            }
            // Sem conta, sem o escopo do plano ou com a sessao revogada nao ha
            // credencial utilizavel: o runtime classifica pelo texto
            // (`is_credential_error` le `status=401`) e vai direto ao fallback,
            // sem repetir — igual a uma API key recusada.
            ErroDeConta::NaoConectado
            | ErroDeConta::PlanoNaoHabilitado { .. }
            | ErroDeConta::SessaoExpirada { .. } => garraia_common::Error::Agent(format!(
                "openai-codex: {e} (status=401: sem credencial utilizavel)"
            )),
            _ => garraia_common::Error::Agent(format!("openai-codex: {e}")),
        }
    }
}

/// Instante atual em ISO 8601 UTC com `Z` (convencao de timestamps do projeto).
pub fn agora_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Serialize, Deserialize)]
struct ArquivoDoHost {
    ext_agent_host_id: String,
}

#[derive(Serialize, Deserialize)]
struct ArquivoDaAtiva {
    client_id: String,
}

/// Onde e como as credenciais do ChatGPT vivem.
#[derive(Debug, Clone)]
pub struct ArmazemDeCredenciais {
    dir: PathBuf,
    endpoints: Endpoints,
    cliente: reqwest::Client,
}

impl ArmazemDeCredenciais {
    /// `dir` e `<config_dir>/credentials/openai-codex`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let cliente = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            dir: dir.into(),
            endpoints: Endpoints::default(),
            cliente,
        }
    }

    pub fn with_endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn diretorio(&self) -> &Path {
        &self.dir
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    pub(crate) fn cliente(&self) -> &reqwest::Client {
        &self.cliente
    }

    fn erro_io(&self, caminho: &Path, e: impl std::fmt::Display) -> ErroDeConta {
        ErroDeConta::Armazenamento {
            caminho: caminho.display().to_string(),
            detalhe: e.to_string(),
        }
    }

    fn garantir_dirs(&self) -> Result<(), ErroDeConta> {
        let contas = self.dir.join("contas");
        for d in [&self.dir, &contas] {
            if !d.exists() {
                garraia_common::fs_perms::create_secret_dir(d).map_err(|e| self.erro_io(d, e))?;
            }
        }
        Ok(())
    }

    /// O `ext_agent_host_id` deste host: criado uma vez (`urn:uuid:` + UUIDv4,
    /// opaco, sem dado do usuario) e reutilizado em todo login.
    pub fn host_id(&self) -> Result<String, ErroDeConta> {
        let caminho = self.dir.join("host.json");
        if let Ok(bytes) = std::fs::read(&caminho)
            && let Ok(a) = serde_json::from_slice::<ArquivoDoHost>(&bytes)
            && !a.ext_agent_host_id.trim().is_empty()
        {
            return Ok(a.ext_agent_host_id);
        }
        self.garantir_dirs()?;
        let id = format!("urn:uuid:{}", uuid::Uuid::new_v4().hyphenated());
        let json = serde_json::to_vec_pretty(&ArquivoDoHost {
            ext_agent_host_id: id.clone(),
        })
        .map_err(|e| self.erro_io(&caminho, e))?;
        escrever_segredo(&caminho, &json).map_err(|e| self.erro_io(&caminho, e))?;
        Ok(id)
    }

    fn caminho_da_conta(&self, client_id: &str) -> PathBuf {
        // O client id emitido e `oaiapp_...`; qualquer caractere fora de
        // [A-Za-z0-9_-] vira `_` para nunca escapar do diretorio.
        let seguro: String = client_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join("contas").join(format!("{seguro}.json"))
    }

    /// O registro de uma conta pelo client id emitido.
    pub fn registro(&self, client_id: &str) -> Result<Option<RegistroDeCredencial>, ErroDeConta> {
        let caminho = self.caminho_da_conta(client_id);
        match std::fs::read(&caminho) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| self.erro_io(&caminho, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.erro_io(&caminho, e)),
        }
    }

    fn client_id_ativo(&self) -> Option<String> {
        let bytes = std::fs::read(self.dir.join("ativa.json")).ok()?;
        serde_json::from_slice::<ArquivoDaAtiva>(&bytes)
            .ok()
            .map(|a| a.client_id)
    }

    /// O registro da conta em uso.
    pub fn registro_ativo(&self) -> Result<Option<RegistroDeCredencial>, ErroDeConta> {
        match self.client_id_ativo() {
            Some(id) => self.registro(&id),
            None => Ok(None),
        }
    }

    /// A conta em uso, sem segredos.
    pub fn conta_ativa(&self) -> Option<ContaChatGpt> {
        self.registro_ativo()
            .ok()
            .flatten()
            .as_ref()
            .map(ContaChatGpt::from)
    }

    /// Todas as contas registradas neste host, sem segredos.
    pub fn contas(&self) -> Vec<ContaChatGpt> {
        let Ok(entradas) = std::fs::read_dir(self.dir.join("contas")) else {
            return Vec::new();
        };
        let mut v: Vec<ContaChatGpt> = entradas
            .flatten()
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice::<RegistroDeCredencial>(&b).ok())
            .map(|r| ContaChatGpt::from(&r))
            .collect();
        v.sort_by(|a, b| a.email.cmp(&b.email));
        v
    }

    /// Grava o registro (atomico, 0600) e o marca como a conta em uso.
    pub fn salvar_e_ativar(&self, registro: &RegistroDeCredencial) -> Result<(), ErroDeConta> {
        self.salvar(registro)?;
        self.ativar(&registro.client_id)
    }

    fn salvar(&self, registro: &RegistroDeCredencial) -> Result<(), ErroDeConta> {
        self.garantir_dirs()?;
        let caminho = self.caminho_da_conta(&registro.client_id);
        let json = serde_json::to_vec_pretty(registro).map_err(|e| self.erro_io(&caminho, e))?;
        escrever_segredo(&caminho, &json).map_err(|e| self.erro_io(&caminho, e))
    }

    /// Troca a conta em uso (ja registrada neste host).
    pub fn ativar(&self, client_id: &str) -> Result<(), ErroDeConta> {
        if self.registro(client_id)?.is_none() {
            return Err(ErroDeConta::NaoConectado);
        }
        let caminho = self.dir.join("ativa.json");
        let json = serde_json::to_vec_pretty(&ArquivoDaAtiva {
            client_id: client_id.to_string(),
        })
        .map_err(|e| self.erro_io(&caminho, e))?;
        escrever_segredo(&caminho, &json).map_err(|e| self.erro_io(&caminho, e))
    }

    /// Um access token utilizavel da conta em uso, renovando antes se ele
    /// vence em menos de 2 minutos. Sem plano habilitado, nao ha token.
    pub async fn access_token(&self) -> Result<String, ErroDeConta> {
        let reg = self.registro_ativo()?.ok_or(ErroDeConta::NaoConectado)?;
        exigir_plano(&reg)?;
        let agora = chrono::Utc::now().timestamp();
        if !reg.precisa_renovar(agora)
            && let Some(at) = reg.access_token.clone()
        {
            return Ok(at);
        }
        self.renovar_sob_trava(&reg.client_id, None).await
    }

    /// Renova depois de um 401 do Responses API. Se outro processo ja
    /// renovou (o token salvo nao e mais o recusado), usa o dele.
    pub async fn renovar_apos_recusa(&self, token_recusado: &str) -> Result<String, ErroDeConta> {
        let reg = self.registro_ativo()?.ok_or(ErroDeConta::NaoConectado)?;
        self.renovar_sob_trava(&reg.client_id, Some(token_recusado))
            .await
    }

    async fn renovar_sob_trava(
        &self,
        client_id: &str,
        token_recusado: Option<&str>,
    ) -> Result<String, ErroDeConta> {
        self.garantir_dirs()?;
        let caminho_trava = self.dir.join("renovacao.lock");
        let _trava = Trava::adquirir(&caminho_trava)
            .await
            .map_err(|e| self.erro_io(&caminho_trava, e))?;

        // Reler sob a trava: outro processo pode ter acabado de renovar.
        let mut reg = self.registro(client_id)?.ok_or(ErroDeConta::NaoConectado)?;
        exigir_plano(&reg)?;
        let agora = chrono::Utc::now().timestamp();
        let ja_renovado = match token_recusado {
            Some(recusado) => {
                reg.access_token.as_deref().is_some_and(|at| at != recusado)
                    && !reg.precisa_renovar(agora)
            }
            None => !reg.precisa_renovar(agora),
        };
        if ja_renovado && let Some(at) = reg.access_token.clone() {
            return Ok(at);
        }

        let conta = reg.email.clone().unwrap_or_else(|| reg.client_id.clone());
        let Some(refresh) = reg.refresh_token.clone() else {
            return Err(ErroDeConta::SessaoExpirada {
                conta,
                codigo: "sem_refresh_token".into(),
            });
        };
        match oauth::renovar(&self.cliente, &self.endpoints, &reg.client_id, &refresh).await {
            Ok(resposta) => {
                reg.aplicar_renovacao(&resposta);
                self.salvar(&reg)?;
                Ok(resposta.access_token)
            }
            Err(e) if e.exige_novo_login() => {
                let codigo = match &e {
                    ErroOAuth::Recusado { codigo, .. } => codigo.clone(),
                    _ => "desconhecido".into(),
                };
                // Refresh inutilizavel: limpar os tokens (mantendo o ID token
                // como dica do proximo login) e pedir login de novo.
                reg.esquecer_sessao(true);
                self.salvar(&reg)?;
                tracing::warn!(conta = %conta, codigo = %codigo, "openai-codex: refresh token recusado; login necessario");
                Err(ErroDeConta::SessaoExpirada { conta, codigo })
            }
            // Rede e 5xx: manter as credenciais e devolver o erro.
            Err(e) => Err(ErroDeConta::OAuth(e)),
        }
    }

    /// Sai da conta em uso: revoga a sessao no servidor e apaga os tokens
    /// locais, mantendo o mapeamento conta↔cliente e o host. Devolve se a
    /// revogacao remota foi confirmada.
    pub async fn sair(&self) -> Result<bool, ErroDeConta> {
        let mut reg = self.registro_ativo()?.ok_or(ErroDeConta::NaoConectado)?;
        let confirmada = match reg.refresh_token.clone() {
            Some(rt) => oauth::revogar(&self.cliente, &self.endpoints, &reg.client_id, &rt)
                .await
                .is_ok(),
            None => true,
        };
        reg.esquecer_sessao(false);
        self.salvar(&reg)?;
        Ok(confirmada)
    }
}

fn exigir_plano(reg: &RegistroDeCredencial) -> Result<(), ErroDeConta> {
    if !reg.conectado() {
        return Err(ErroDeConta::NaoConectado);
    }
    if !reg.plano_habilitado() {
        return Err(ErroDeConta::PlanoNaoHabilitado {
            conta: reg.email.clone().unwrap_or_else(|| reg.client_id.clone()),
        });
    }
    Ok(())
}

/// Trava exclusiva entre processos sobre um arquivo (liberada no `drop`).
struct Trava(std::fs::File);

impl Trava {
    async fn adquirir(caminho: &Path) -> std::io::Result<Self> {
        let arquivo = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(caminho)?;
        let inicio = std::time::Instant::now();
        loop {
            match arquivo.try_lock() {
                Ok(()) => return Ok(Self(arquivo)),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if inicio.elapsed() > ESPERA_MAXIMA_DA_TRAVA {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "outro processo segura a renovacao do token ha mais de 30 s",
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e),
            }
        }
    }
}

impl Drop for Trava {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Escreve `bytes` em `caminho` de forma atomica (temporario + rename) com
/// permissao `0600` desde a criacao (Unix).
pub(crate) fn escrever_segredo(caminho: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = caminho.parent().unwrap_or_else(|| Path::new("."));
    let nome = caminho
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("credencial");
    let temporario = dir.join(format!(".{nome}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut opcoes = std::fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opcoes.mode(0o600);
    }
    let resultado = (|| {
        let mut f = opcoes.open(&temporario)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&temporario, caminho)
    })();
    if resultado.is_err() {
        let _ = std::fs::remove_file(&temporario);
    }
    resultado?;
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn registro(
        client_id: &str,
        salvo_ha_s: i64,
        escopos: &[&str],
    ) -> RegistroDeCredencial {
        let salvo = chrono::Utc::now() - chrono::Duration::seconds(salvo_ha_s);
        RegistroDeCredencial {
            email: Some("a@b.c".into()),
            name: Some("Ana".into()),
            issuer: "https://auth.openai.com".into(),
            subject: "user-1".into(),
            client_id: client_id.into(),
            ext_agent_host_id: "urn:uuid:x".into(),
            id_token: Some("ID-secreto".into()),
            access_token: Some("AT-secreto".into()),
            refresh_token: Some("RT-secreto".into()),
            token_type: "Bearer".into(),
            expires_in: 3600,
            scopes: escopos.iter().map(|s| s.to_string()).collect(),
            saved_at: salvo.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            earliest_refresh_at: None,
        }
    }

    #[test]
    fn decide_quando_renovar() {
        let agora = chrono::Utc::now().timestamp();
        assert!(!registro("c", 60, &[ESCOPO_DO_PLANO]).precisa_renovar(agora));
        // Faltando menos de 2 minutos.
        assert!(registro("c", 3500, &[ESCOPO_DO_PLANO]).precisa_renovar(agora));
        // Vencido.
        assert!(registro("c", 7200, &[ESCOPO_DO_PLANO]).precisa_renovar(agora));
        // Sem access token mas com refresh: renovar.
        let mut r = registro("c", 0, &[ESCOPO_DO_PLANO]);
        r.access_token = None;
        assert!(r.precisa_renovar(agora));
    }

    #[test]
    fn debug_e_visao_publica_nao_vazam_tokens() {
        let r = registro("oaiapp_1", 0, &[ESCOPO_DO_PLANO, "email"]);
        let d = format!("{r:?}");
        for segredo in ["AT-secreto", "RT-secreto", "ID-secreto"] {
            assert!(!d.contains(segredo), "{segredo} no Debug");
        }
        let conta = ContaChatGpt::from(&r);
        let json = serde_json::to_string(&conta).expect("json");
        assert!(!json.contains("secreto"));
        assert!(conta.plano_habilitado && conta.conectado);
    }

    #[test]
    fn renovacao_troca_tudo_junto_e_mantem_refresh_se_nao_vier_outro() {
        let mut r = registro("c", 4000, &[ESCOPO_DO_PLANO]);
        let resp: RespostaDeToken =
            serde_json::from_str(r#"{"access_token":"AT2","expires_in":3600,"scope":"openid chatgpt.tokens.use.direct"}"#)
                .expect("json");
        r.aplicar_renovacao(&resp);
        assert_eq!(r.access_token.as_deref(), Some("AT2"));
        assert_eq!(r.refresh_token.as_deref(), Some("RT-secreto"));
        assert!(!r.precisa_renovar(chrono::Utc::now().timestamp()));
        let resp2: RespostaDeToken = serde_json::from_str(
            r#"{"access_token":"AT3","refresh_token":"RT3","expires_in":3600}"#,
        )
        .expect("json");
        r.aplicar_renovacao(&resp2);
        assert_eq!(r.refresh_token.as_deref(), Some("RT3"));
        // Sem `scope` na resposta, os escopos anteriores ficam.
        assert!(r.plano_habilitado());
    }

    #[test]
    fn host_id_e_estavel_opaco_e_o_arquivo_e_0600() {
        let tmp = tempfile::tempdir().expect("tmp");
        let a = ArmazemDeCredenciais::new(tmp.path().join("openai-codex"));
        let id1 = a.host_id().expect("host");
        let id2 = a.host_id().expect("host");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("urn:uuid:"));
        #[cfg(unix)]
        {
            let modo =
                garraia_common::fs_perms::mode_of(&a.diretorio().join("host.json")).expect("modo");
            assert_eq!(modo.map(|m| m & 0o777), Some(0o600));
        }
    }

    #[test]
    fn salvar_ativar_e_listar_contas() {
        let tmp = tempfile::tempdir().expect("tmp");
        let a = ArmazemDeCredenciais::new(tmp.path().join("oc"));
        assert!(a.conta_ativa().is_none());
        a.salvar_e_ativar(&registro("oaiapp_1", 0, &[ESCOPO_DO_PLANO]))
            .expect("salva");
        let mut outra = registro("oaiapp_2", 0, &["openid"]);
        outra.email = Some("z@b.c".into());
        a.salvar_e_ativar(&outra).expect("salva");
        assert_eq!(
            a.conta_ativa().map(|c| c.client_id),
            Some("oaiapp_2".into())
        );
        assert_eq!(a.contas().len(), 2);
        a.ativar("oaiapp_1").expect("ativa");
        assert_eq!(
            a.conta_ativa().map(|c| c.client_id),
            Some("oaiapp_1".into())
        );
        assert!(a.ativar("oaiapp_inexistente").is_err());
        // Nome de arquivo nunca escapa do diretorio.
        assert!(
            a.caminho_da_conta("../../etc/x")
                .starts_with(a.diretorio().join("contas"))
        );
    }

    #[tokio::test]
    async fn sem_plano_nao_ha_token_e_sem_conta_tambem_nao() {
        let tmp = tempfile::tempdir().expect("tmp");
        let a = ArmazemDeCredenciais::new(tmp.path().join("oc"));
        assert!(matches!(
            a.access_token().await,
            Err(ErroDeConta::NaoConectado)
        ));
        a.salvar_e_ativar(&registro("oaiapp_1", 0, &["openid", "email"]))
            .expect("salva");
        assert!(matches!(
            a.access_token().await,
            Err(ErroDeConta::PlanoNaoHabilitado { .. })
        ));
    }

    #[tokio::test]
    async fn token_valido_e_devolvido_sem_rede() {
        let tmp = tempfile::tempdir().expect("tmp");
        // Endpoints inalcancaveis: se tentasse renovar, falharia.
        let e = Endpoints {
            token: "http://127.0.0.1:9/token".into(),
            ..Endpoints::default()
        };
        let a = ArmazemDeCredenciais::new(tmp.path().join("oc")).with_endpoints(e);
        a.salvar_e_ativar(&registro("oaiapp_1", 10, &[ESCOPO_DO_PLANO]))
            .expect("salva");
        assert_eq!(a.access_token().await.expect("token"), "AT-secreto");
    }

    #[tokio::test]
    async fn trava_serializa_e_libera_no_drop() {
        let tmp = tempfile::tempdir().expect("tmp");
        let caminho = tmp.path().join("t.lock");
        let t1 = Trava::adquirir(&caminho).await.expect("trava 1");
        let outro = std::fs::OpenOptions::new()
            .write(true)
            .open(&caminho)
            .expect("abre");
        assert!(matches!(
            outro.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(t1);
        assert!(outro.try_lock().is_ok());
    }
}
