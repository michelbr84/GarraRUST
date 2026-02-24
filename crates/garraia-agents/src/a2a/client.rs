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

impl A2AClient {
    /// Cria uma nova instância do cliente A2A.
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }

    /// Busca o Agent Card no endpoint well-known do agente remoto.
    ///
    /// Espera que o agente exponha:
    /// `{base_url}/.well-known/agent.json`
    pub async fn fetch_agent_card(&self, base_url: &str) -> Result<AgentCard> {
        let url = format!(
            "{}/.well-known/agent.json",
            base_url.trim_end_matches('/')
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                Error::Agent(format!(
                    "falha ao buscar agent card em {url}: {e}"
                ))
            })?;

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
            .map_err(|e| {
                Error::Agent(format!(
                    "falha ao criar tarefa em {url}: {e}"
                ))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Agent(format!(
                "criação de tarefa falhou com status {status}: {body}"
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
        let url = format!(
            "{}/a2a/tasks/{}",
            base_url.trim_end_matches('/'),
            task_id
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                Error::Agent(format!(
                    "falha ao consultar tarefa em {url}: {e}"
                ))
            })?;

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
            .map_err(|e| {
                Error::Agent(format!(
                    "falha ao cancelar tarefa em {url}: {e}"
                ))
            })?;

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