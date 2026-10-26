# Evidencia de ferramentas entre turnos da CLI

Esta e uma primeira etapa da [issue #1708](https://github.com/michelbr84/GarraRUST/issues/1708), nao a resolucao integral de seus criterios de aceite.

## Funcionamento

O runtime gera eventos de inicio e fim das ferramentas. A CLI associa cada termino ao inicio correspondente e constroi um resumo separado da resposta do modelo. Um inicio isolado nao prova sucesso. O registro descreve status reportado (sucesso ou falha/recusa), nome da ferramenta e, em operacoes de arquivos, somente o nome final do arquivo.

Nao sao copiados para o contexto comandos Bash, URLs, caminhos completos ou texto retornado pelas ferramentas. Ate vinte registros recentes podem ser inspecionados em memoria; perdas por expulsao do buffer sao sinalizadas. Um identificador de turno distingue indices locais reutilizados depois de reiniciar o processo.

Sucesso de escrita reportado pelo executor nao significa que o arquivo ainda existe, que nao foi alterado externamente ou que houve comparacao byte a byte.

## Persistencia

O comportamento sem --persist/--resume continua sem gravar novos dados no disco. Com persistencia habilitada, as evidencias sao gravadas com a direcao especial tool_evidence e metadados de proveniencia source=runtime; o --resume as reidrata no contexto. Elas nao sao texto originalmente produzido pelo LLM.

Nomes de arquivo podem ser sensiveis. O resumo os limita e saneia, mas isso nao equivale a garantia criptografica de sigilo. O banco de sessoes deve continuar sujeito aos controles de acesso locais.

## Limites conhecidos (Refs #1708)

A etapa nao preserva os pares originais estruturados ToolUse/ToolResult com IDs nativos dos providers. Ainda faltam testes E2E com Anthropic e um segundo provider, recuperacao detalhada do /tool apos restart, testes abrangentes de timeout/cancelamento/permissao e avaliacao longitudinal da veracidade das respostas. O alcance inicial e somente a CLI. Por isso esta PR nao deve fechar a issue #1708.

A suite de testes implementada nesta etapa valida geracao sem saidas brutas e reidratacao de dados na SessionStore. So declare aprovado aquilo que efetivamente tiver sido executado em CI.
