# GarraIA — Política de Privacidade — RASCUNHO

> **Status: RASCUNHO — NÃO PUBLICAR até revisão jurídica e definição de
> encarregado (DPO).** Artefato de engenharia para a issue de GA
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)); base factual:
> `docs/compliance/dpia.md`. Esta política descreve o produto **como ele
> opera hoje**, com a oferta cloud explicitamente rotulada como planejada.
> Itens entre `[PENDENTE]` exigem decisão ou revisão humana.

## 1. Quem trata os dados

- **Instalação self-host (default, vigente):** você instala o GarraIA na sua
  própria máquina/servidor. Os dados processados pelo agente **ficam na sua
  infraestrutura**; o projeto GarraIA não tem acesso a eles. Nesse modelo,
  **você é o controlador** dos dados dos seus usuários finais, e esta
  política descreve o que o software faz — a responsabilidade pelo
  tratamento é sua, incluindo as suas próprias obrigações legais.
- **Garra Cloud (PLANEJADO — não ofertada):** quando existir, o projeto
  operará a infraestrutura e tratará dados em nome dos operadores de conta.
  Esta seção só passa a valer com a publicação dos termos da oferta.
- **Integrações externas:** provedores LLM e plataformas de canais que
  **você** conecta são terceiros regidos pelos termos de cada um; em
  self-host, a relação é direta entre você e eles.

## 2. Dados que o agente processa

| Categoria | Exemplos | Origem |
|---|---|---|
| Conteúdo de mensagens | texto, comandos, anexos citados pelo agente | canais conectados (Telegram, Discord, Slack, WhatsApp etc.) |
| Dados de conta | e-mail de login, display_name | cadastro no gateway |
| Metadados técnicos | `ip_inet`, `user_agent` | acesso ao gateway |
| Sessões e chaves | device de sessão, API keys (somente metadado; o hash nunca é exposto) | uso do produto |
| Registros de auditoria | eventos de conta e de ferramenta | motor de auditoria |

Base legal para `ip_inet`/`user_agent` (segurança e prevenção a fraude):
**legítimo interesse**, com o assessment formal em
`docs/compliance/lia.md` — a decisão de governança é condicionada àquele
documento, e este LIA **não estende** a publicidade, tracking ou tratamento
de dados sensíveis.

**Não vendemos dados pessoais.** Conversas não são usadas para publicidade
nem para treinamento de modelos do projeto.

## 3. Como usamos e protegemos

- Sandbox de bash com allowlist por comando (`agent.bash_allowlist`) e
  denylist de segurança sempre à frente (v0.4.8).
- Vault criptografado para segredos (chaves de canal e de LLM).
- Sessões revogáveis; anonimização e exclusão de conta disponíveis nos
  endpoints de direitos do titular.
- LLM: o produto é **traga a sua chave (BYOK)** — em self-host, as mensagens
  vão ao provedor LLM que **você** configurar, regido pelos termos daquele
  provedor. A afirmação de que "sua chave não sai da sua máquina" descreve
  **apenas o fluxo self-host**. `[PENDENTE: listar provedores padrão
  (OpenRouter, Anthropic, OpenAI) com links das políticas de cada um]`

## 4. Seus direitos (LGPD / GDPR)

O gateway implementa, para a conta autenticada:

- **Exportação** dos dados da conta: `GET /v1/me/export` (perfil, sessões,
  chaves de API, auditoria, grupos).
- **Anonimização**: `POST /v1/me/anonymize` (substitui identificadores por
  token, revoga sessões).
- **Exclusão**: `DELETE /v1/me` (soft-delete com tombstone; exclusão alcança
  dados derivados — memória e embeddings — quando o purge roda).
- **Correção de perfil**: `PATCH /v1/me`.

**Decisão registrada (2026-10-09):** o escopo da exportação será estendido
para incluir o arquivo de mensagens — engenharia em curso em branch separada;
a mudança documental do DPIA/desta política acompanha o código, não a
intenção. Limitação vigente: a exportação cobre dados de **conta**; o arquivo
completo de mensagens ainda não é incluído.

Restauração de backup reaplica exclusões/anonimizações já executadas (sem
reativar dados apagados). `[PENDENTE: verificação operacional dessa
garantia em restore]`

## 5. Encarregado (DPO) e contato

`[PENDENTE: designar encarregado (LGPD art. 41) e publicar canal de contato —
canal atual de segurança/vulnerabilidades: SECURITY.md (advisory privado) e
security@garraia.org; pedidos de titulares: canal privado de suporte,
conforme tabela em SUPPORT.md]`

## 6. Suboperadores

Self-host: provedores conectados por você (LLM, canais) são regidos pelos
termos de cada um. Garra Cloud (futuro): inventário por fluxo em
`docs/legal/dpa-template.md` §5, com processo de atualização.
`[PENDENTE]`

## 7. Retenção

### Self-host (vigente)

A retenção é definida pelo operador da instalação: limpeza de sessões após
90 dias de fechamento (default do produto), políticas de memória
(`memory.retention`, default 90 dias) e do ledger de runs (`runs.retention`,
default desligado). Registros de acesso que o operador self-host precise
guardar por obrigação legal (Marco Civil art. 15 — registros de acesso à
aplicação, 6 meses) são responsabilidade do operador.

### Garra Cloud (PROPOSTO — política de produto a implementar e verificar)

Tabela a seguir é **política proposta**, não comportamento implementado:
a oferta cloud não existe, e nenhuma dessas retenções está em operação.
Antes de anunciar, cada linha precisa de implementação **e** verificação.

| Dado | Retenção proposta | Observação |
|---|---|---|
| Mensagens, anexos e resultados de ferramentas | **90 dias (default)**; opções de 7 ou 30 dias por conta | |
| Memórias derivadas de mensagens | Seguem a política da fonte | Apagada a fonte, apaga o derivado |
| Memórias preservadas pelo usuário | Persistem até o usuário apagar | Visualização, edição e exclusão próprias |
| Logs diagnósticos | 30 dias | Segredos removidos antes do armazenamento |
| Registros de segurança/auditoria | 90 dias | Distintos dos registros legais abaixo |
| Backups | Rotação ≤ 30 dias | Tratamento de exclusão: restauração não reativa dado já excluído |
| Dados de cadastro e faturamento | Durante a relação + prazos legalmente exigidos | |

**Divergência registrada (doc ↔ código):** o DPIA (`dpia.md` §2.5) cita
retenção default de 730 dias para mensagens via
`groups.settings_jsonb.retention_days`; essa coluna/chave **não foi
encontrada nas migrations do workspace** — a retenção efetiva hoje é a
descrita no bloco self-host acima. A divergência é pendência de
implementação/verificação, não de texto.

Mudanças de retenção futuras serão comunicadas com antecedência, com
oportunidade de exportação antes de qualquer redução.

**Biometria:** não tratamos biometria hoje; se introduzida, será dado
sensível (LGPD art. 5, II). Vetor/embedding derivado de dado pessoal **não é
anonimização** — segue a política do dado de origem.

## 8. Alterações desta política

Versões desta política serão registradas no repositório; mudanças materiais
serão anunciadas no CHANGELOG/docs com aviso razoável. `[PENDENTE: definir
canal de anúncio]`
