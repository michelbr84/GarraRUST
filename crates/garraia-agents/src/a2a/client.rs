use garraia_common::{Error, Result};
use reqwest::Client;

use super::model::{A2ATask, AgentCard, CreateTaskRequest};

/// Cliente para comunicação com agentes remotos compatíveis com A2A.
pub struct A2AClient {
    http: Client,
}

impl Default for A2AClient {
    fn default() -> Self {
        Self::new()
    }
}

/// #1792: o que um 3xx do agente remoto manda corrigir. O 3xx vira erro que
/// cita o `Location` (`providers::recusar_redirecionamento`), nunca um redirect
/// seguido.
const ALVO_DO_REDIRECT: &str = "a base_url do agente remoto";

impl A2AClient {
    /// Cria uma nova instância do cliente A2A.
    ///
    /// #1792: era `Client::new()` — seguia ate 10 redirects e uma tarefa num
    /// peer travado esperava para sempre. Agora a politica de `cliente_llm`
    /// (sem redirect, teto de conexao) e a janela padrao do LLM: nao ha config
    /// que traga outra.
    pub fn new() -> Self {
        Self {
            http: crate::cliente_llm::cliente_padrao(Some(crate::cliente_llm::JANELA_PADRAO)),
        }
    }

    /// Busca o Agent Card no endpoint well-known do agente remoto.
    ///
    /// Espera que o agente exponha:
    /// `{base_url}/.well-known/agent.json`
    pub async fn fetch_agent_card(&self, base_url: &str) -> Result<AgentCard> {
        let url = format!("{}/.well-known/agent.json", base_url.trim_end_matches('/'));

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha ao buscar agent card em {url}: {e}")))?;

        crate::providers::recusar_redirecionamento(
            "requisição do agent card falhou",
            &resp,
            ALVO_DO_REDIRECT,
        )?;
        if !resp.status().is_success() {
            return Err(Error::Agent(format!(
                "requisição do agent card falhou com status {}",
                resp.status()
            )));
        }

        resp.json::<AgentCard>()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar agent card: {e}")))
    }

    /// Cria uma tarefa em um agente remoto A2A.
    ///
    /// Endpoint esperado:
    /// `{base_url}/a2a/tasks`
    pub async fn create_task(
        &self,
        base_url: &str,
        request: &CreateTaskRequest,
    ) -> Result<A2ATask> {
        let url = format!("{}/a2a/tasks", base_url.trim_end_matches('/'));

        let resp = self
            .http
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha ao criar tarefa em {url}: {e}")))?;

        crate::providers::recusar_redirecionamento(
            "criação de tarefa falhou",
            &resp,
            ALVO_DO_REDIRECT,
        )?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // #1737 F2: so a frase do peer, nunca o corpo inteiro.
            return Err(Error::Agent(format!(
                "criação de tarefa falhou com status {status}: {}",
                crate::providers::corpo_de_erro_sanitizado(&body)
            )));
        }

        resp.json::<A2ATask>()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da tarefa: {e}")))
    }

    /// Consulta o status de uma tarefa em um agente remoto A2A.
    ///
    /// Endpoint esperado:
    /// `{base_url}/a2a/tasks/{task_id}`
    pub async fn get_task(&self, base_url: &str, task_id: &str) -> Result<A2ATask> {
        let url = format!("{}/a2a/tasks/{}", base_url.trim_end_matches('/'), task_id);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha ao consultar tarefa em {url}: {e}")))?;

        crate::providers::recusar_redirecionamento(
            "consulta de tarefa falhou",
            &resp,
            ALVO_DO_REDIRECT,
        )?;
        if !resp.status().is_success() {
            return Err(Error::Agent(format!(
                "consulta de tarefa falhou com status {}",
                resp.status()
            )));
        }

        resp.json::<A2ATask>()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da tarefa: {e}")))
    }

    /// Cancela uma tarefa em um agente remoto A2A.
    ///
    /// Endpoint esperado:
    /// `{base_url}/a2a/tasks/{task_id}/cancel`
    pub async fn cancel_task(&self, base_url: &str, task_id: &str) -> Result<A2ATask> {
        let url = format!(
            "{}/a2a/tasks/{}/cancel",
            base_url.trim_end_matches('/'),
            task_id
        );

        let resp = self
            .http
            .post(&url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha ao cancelar tarefa em {url}: {e}")))?;

        crate::providers::recusar_redirecionamento(
            "cancelamento de tarefa falhou",
            &resp,
            ALVO_DO_REDIRECT,
        )?;
        if !resp.status().is_success() {
            return Err(Error::Agent(format!(
                "cancelamento de tarefa falhou com status {}",
                resp.status()
            )));
        }

        resp.json::<A2ATask>()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da tarefa: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::model::{A2AMessage, A2APart};
    use crate::cliente_llm::servidor_que_redireciona as srv;

    /// #1792: o cliente era `Client::new()` — seguia ate 10 redirects e nao
    /// tinha teto nenhum. Agora as quatro chamadas devolvem o 3xx como erro
    /// que cita o `Location` (redigido) e nunca o seguem.
    #[tokio::test]
    async fn redirect_nao_e_seguido_e_o_erro_cita_o_location() {
        let s = srv::subir(302).await;
        let cliente = A2AClient::new();
        let pedido = CreateTaskRequest {
            id: None,
            message: A2AMessage {
                role: "user".into(),
                parts: vec![A2APart::Text { text: "oi".into() }],
            },
            metadata: Default::default(),
            target: None,
        };

        let erros = [
            (
                "fetch_agent_card",
                cliente
                    .fetch_agent_card(&s.base)
                    .await
                    .expect_err("3xx tem de virar erro"),
            ),
            (
                "create_task",
                cliente
                    .create_task(&s.base, &pedido)
                    .await
                    .expect_err("3xx tem de virar erro"),
            ),
            (
                "get_task",
                cliente
                    .get_task(&s.base, "t1")
                    .await
                    .expect_err("3xx tem de virar erro"),
            ),
            (
                "cancel_task",
                cliente
                    .cancel_task(&s.base, "t1")
                    .await
                    .expect_err("3xx tem de virar erro"),
            ),
        ];

        assert_eq!(s.pedidos(), erros.len(), "algum redirect foi seguido");
        for (chamada, erro) in erros {
            let msg = erro.to_string();
            assert!(msg.contains("status=302 Found"), "{chamada}: {msg}");
            assert!(
                msg.contains(&format!("location={}", s.location_redigido())),
                "{chamada}: o Location tem de ser citado, redigido: {msg}"
            );
            assert!(!msg.contains(srv::SEGREDO), "{chamada}: {msg}");
            assert!(
                msg.contains("corrija a base_url do agente remoto"),
                "{chamada}: {msg}"
            );
        }
    }
}
