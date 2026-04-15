use crate::providers::{ChatMessage, ChatRole, MessagePart};
use crate::runtime::AgentRuntime;
use garraia_common::Result;

/// Estrutura de fato estruturado extraído
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StructuredFact {
    pub fact_type: String,
    pub key: String,
    pub value: String,
    pub confidence: f32,
}

/// Extrator de fatos baseado em LLM - muito mais preciso que regex
pub struct LlmMemoryExtractor {
    /// Prompt do sistema para extração de fatos
    system_prompt: String,
}

impl LlmMemoryExtractor {
    pub fn new() -> Self {
        Self {
            system_prompt: r#"Você é um extrator de fatos sobre usuários de conversas.
Analise a mensagem do usuário e extraia fatos relevantes sobre ele.

Tipos de fatos a procurar:
- identity: nome, apelido, idade
- location: cidade, estado, país, endereço
- preference: preferências, gosta de, dislikes
- occupation: trabalho, cargo, empresa
- hobby: hobbies, interesses, atividades
- personal: família, Relationships, eventos importantes
- skill: habilidades, conhecimentos
- fact: fatos importantes ditos pelo usuário

Retorne APENAS um JSON array com os fatos encontrados, ou array vazio se nada relevante.
Cada fato deve ter: fact_type, key, value, confidence (0.0 a 1.0)

Exemplo de saída:
[
  {"fact_type": "preference", "key": "favorite_food", "value": "sushi", "confidence": 0.95},
  {"fact_type": "occupation", "key": "job", "value": "desenvolvedor", "confidence": 0.90}
]

Se não houver fatos relevantes, retorne: []"#
                .to_string(),
        }
    }

    /// Extrai fatos de uma mensagem usando o LLM
    pub async fn extract_facts(
        &self,
        runtime: &AgentRuntime,
        message: &str,
    ) -> Result<Vec<StructuredFact>> {
        // Chamar o LLM para extrair fatos
        let prompt = format!(
            "{}\n\nMensagem do usuário: \"{}\"\n\nExtraia todos os fatos relevantes sobre o usuário desta mensagem.",
            self.system_prompt, message
        );

        let messages = vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(prompt),
        }];

        // Use the runtime's default provider to complete the chat
        let response = runtime
            .chat_completion(messages, None, None, None, None, None)
            .await?;

        // Parsear o JSON de resposta
        self.parse_facts_from_response(&response)
    }

    fn parse_facts_from_response(&self, response: &str) -> Result<Vec<StructuredFact>> {
        // Tentar parsear como array de fatos
        if let Ok(facts) = serde_json::from_str::<Vec<StructuredFact>>(response) {
            // Filtrar fatos com valores vazios
            let valid_facts: Vec<StructuredFact> = facts
                .into_iter()
                .filter(|f| !f.key.trim().is_empty() && !f.value.trim().is_empty())
                .collect();
            return Ok(valid_facts);
        }

        // Tentar extrair JSON do texto de resposta
        if let Some(start) = response.find('[')
            && let Some(end) = response.rfind(']')
        {
            let json_str = &response[start..=end];
            if let Ok(facts) = serde_json::from_str::<Vec<StructuredFact>>(json_str) {
                // Filtrar fatos com valores vazios
                let valid_facts: Vec<StructuredFact> = facts
                    .into_iter()
                    .filter(|f| !f.key.trim().is_empty() && !f.value.trim().is_empty())
                    .collect();
                return Ok(valid_facts);
            }
        }

        Ok(Vec::new())
    }
}

impl Default for LlmMemoryExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extractor_creation() {
        let extractor = LlmMemoryExtractor::new();
        assert!(!extractor.system_prompt.is_empty());
    }
}
