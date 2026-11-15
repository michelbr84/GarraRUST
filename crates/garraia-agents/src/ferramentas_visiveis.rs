//! #1712 — uma visão só das ferramentas do processo, para as duas projeções.
//!
//! O `/status` e o `/tools` da CLI contavam e listavam TODA tool registrada
//! (`AgentRuntime::list_tool_info`) e chamavam isso de "ferramentas que o
//! agente tem". A tool `cli_inspect` já reportava outra coisa: só o que o
//! portão do turno liberou. Eram duas leituras do mesmo inventário, com
//! números que pareciam discordar — e o usuário comparando as duas superfícies
//! via uma divergência que existia na FORMA de ler, não no runtime.
//!
//! Este módulo é o coletor comum: uma leitura do inventário + a decisão de
//! liberação (quando observada), consumida pelas duas projeções. A projeção de
//! interface mostra o que está registrado e diz que a liberação é decidida por
//! turno; a projeção do modelo (`cli_inspect`) omite o que o portão nega. O que
//! muda entre elas é a apresentação — a fonte é esta.
//!
//! Não confundir registro com liberação: `tool_program` é sintética (aparece
//! nas definições sob a condição do portão, sem estar no registro), e uma tool
//! registrada pode estar indisponível agora (`Disponibilidade`).

use crate::runtime::{AgentRuntime, TOOL_PROGRAM_NAME};
use crate::tools::Disponibilidade;

/// Quem disse `sim`/`nao` para cada ferramenta nesta visão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrigemDaPermissao {
    /// A visão foi montada dentro de um turno despachado pelo runtime: a
    /// liberação é a do `ToolGate` daquele turno.
    PortaoDoTurno,
    /// Nenhum portão observado neste ponto (o REPL, antes ou depois do
    /// turno). O portão da CLI depende do TEXTO do turno
    /// (`ToolGate::para_o_turno`), então não existe "liberado" honesto para
    /// mostrar fora dele.
    NaoObservada,
}

impl OrigemDaPermissao {
    /// Rótulo estável para relatório/serialização.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PortaoDoTurno => "turn_gate",
            Self::NaoObservada => "not_observed",
        }
    }
}

/// Uma ferramenta como as duas projeções a veem.
#[derive(Clone, Debug)]
pub struct FerramentaVisivel {
    pub nome: String,
    pub descricao: String,
    /// `"native"` ou `"mcp"`.
    pub origem: String,
    /// Servidor MCP de origem, quando houver.
    pub servidor: Option<String>,
    /// `tool_program` é sintética: entra nas definições sem estar no registro.
    pub sintetica: bool,
    /// `Some(true)` liberada pelo portão do turno, `Some(false)` negada,
    /// `None` quando nenhum portão foi observado.
    pub liberada: Option<bool>,
    pub disponibilidade: Disponibilidade,
}

/// Inventário + liberação numa leitura só.
#[derive(Clone, Debug)]
pub struct VisaoDeFerramentas {
    pub permissao: OrigemDaPermissao,
    pub ferramentas: Vec<FerramentaVisivel>,
}

impl VisaoDeFerramentas {
    /// Tools realmente registradas (a sintética `tool_program` não conta).
    pub fn registradas(&self) -> usize {
        self.ferramentas.iter().filter(|f| !f.sintetica).count()
    }

    /// Quantas o portão liberou — `None` quando nenhum portão foi observado.
    pub fn liberadas(&self) -> Option<usize> {
        match self.permissao {
            OrigemDaPermissao::PortaoDoTurno => Some(
                self.ferramentas
                    .iter()
                    .filter(|f| f.liberada == Some(true))
                    .count(),
            ),
            OrigemDaPermissao::NaoObservada => None,
        }
    }
}

impl AgentRuntime {
    /// #1712: o inventário com a liberação do turno corrente, numa leitura só.
    ///
    /// `liberadas` é a lista que o despacho publica em
    /// `tools::turn_tools::ferramentas_do_turno()`: `Some` dentro de um turno,
    /// `None` para quem pergunta fora dele (o REPL dos slash commands).
    ///
    /// A visão NÃO filtra nada — filtrar é decisão da projeção, e a projeção
    /// do modelo filtra fail-closed (omite negadas e, sem portão, omite tudo).
    pub fn visao_das_ferramentas(&self, liberadas: Option<&[String]>) -> VisaoDeFerramentas {
        let permissao = if liberadas.is_some() {
            OrigemDaPermissao::PortaoDoTurno
        } else {
            OrigemDaPermissao::NaoObservada
        };
        let mut ferramentas: Vec<FerramentaVisivel> = self
            .tool_inventory()
            .into_iter()
            .map(|entry| FerramentaVisivel {
                liberada: liberadas.map(|lista| lista.iter().any(|nome| nome == &entry.name)),
                disponibilidade: self.disponibilidade_de(&entry.name),
                nome: entry.name,
                descricao: entry.description,
                origem: entry.source,
                servidor: entry.server,
                sintetica: false,
            })
            .collect();
        // `tool_program` nunca está no registro: entra pela MESMA condição do
        // despacho (o nome presente na lista liberada).
        if liberadas.is_some_and(|lista| lista.iter().any(|nome| nome == TOOL_PROGRAM_NAME)) {
            ferramentas.push(FerramentaVisivel {
                nome: TOOL_PROGRAM_NAME.to_string(),
                descricao: String::new(),
                origem: "native_runtime".to_string(),
                servidor: None,
                sintetica: true,
                liberada: Some(true),
                disponibilidade: self.disponibilidade_de(TOOL_PROGRAM_NAME),
            });
        }
        ferramentas.sort_by(|a, b| a.nome.cmp(&b.nome));
        VisaoDeFerramentas {
            permissao,
            ferramentas,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolContext, ToolOutput};
    use async_trait::async_trait;
    use serde_json::{Value, json};

    struct Fake {
        nome: &'static str,
        disponibilidade: Disponibilidade,
    }

    #[async_trait]
    impl Tool for Fake {
        fn name(&self) -> &str {
            self.nome
        }

        fn description(&self) -> &str {
            "fixture do coletor"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _ctx: &ToolContext,
            _input: Value,
        ) -> garraia_common::Result<ToolOutput> {
            Ok(ToolOutput::success("ok"))
        }

        fn disponibilidade(&self) -> Disponibilidade {
            self.disponibilidade.clone()
        }
    }

    fn runtime() -> AgentRuntime {
        let runtime = AgentRuntime::new();
        runtime.register_tool(Box::new(Fake {
            nome: "file_read",
            disponibilidade: Disponibilidade::Disponivel,
        }));
        runtime.register_tool(Box::new(Fake {
            nome: "bash",
            disponibilidade: Disponibilidade::indisponivel("not_configured", "sem config", None),
        }));
        runtime
    }

    #[test]
    fn sem_portao_a_visao_declara_registradas_e_nao_inventa_liberacao() {
        let visao = runtime().visao_das_ferramentas(None);
        assert_eq!(visao.permissao, OrigemDaPermissao::NaoObservada);
        assert_eq!(visao.permissao.as_str(), "not_observed");
        assert_eq!(visao.registradas(), 2);
        assert_eq!(visao.liberadas(), None);
        assert!(
            visao
                .ferramentas
                .iter()
                .all(|f| f.liberada.is_none() && !f.sintetica)
        );
        // A disponibilidade operacional vem do inventário, não da contagem.
        let bash = visao
            .ferramentas
            .iter()
            .find(|f| f.nome == "bash")
            .expect("bash registrada");
        assert!(matches!(
            bash.disponibilidade,
            Disponibilidade::Indisponivel { .. }
        ));
    }

    #[test]
    fn com_portao_a_visao_marca_liberada_e_negada_sem_prometer_tool_program() {
        let liberadas = vec!["file_read".to_string(), TOOL_PROGRAM_NAME.to_string()];
        let visao = runtime().visao_das_ferramentas(Some(&liberadas));
        assert_eq!(visao.permissao, OrigemDaPermissao::PortaoDoTurno);
        assert_eq!(visao.permissao.as_str(), "turn_gate");
        // Registradas são as duas reais; a sintética fica fora da contagem.
        assert_eq!(visao.registradas(), 2);
        assert_eq!(visao.liberadas(), Some(2));
        let bash = visao
            .ferramentas
            .iter()
            .find(|f| f.nome == "bash")
            .expect("bash registrada");
        assert_eq!(bash.liberada, Some(false));
        let programa = visao
            .ferramentas
            .iter()
            .find(|f| f.nome == TOOL_PROGRAM_NAME)
            .expect("tool_program sintética");
        assert!(programa.sintetica);
    }

    #[test]
    fn tool_program_sintetica_so_entra_com_o_portao_liberando() {
        let liberadas = vec!["file_read".to_string()];
        let visao = runtime().visao_das_ferramentas(Some(&liberadas));
        assert!(
            visao
                .ferramentas
                .iter()
                .all(|f| f.nome != TOOL_PROGRAM_NAME),
            "tool_program apareceu sem o portão liberá-la"
        );
        assert_eq!(visao.liberadas(), Some(1));
        assert!(visao.ferramentas.iter().all(|f| !f.sintetica));
    }
}
