# GarraIA — Política de Privacidade — RASCUNHO

> **Status: RASCUNHO — NÃO PUBLICAR até revisão jurídica e definição de
> encarregado (DPO).** Artefato de engenharia para a issue de GA
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)); base factual:
> `docs/compliance/dpia.md`. Itens entre `[PENDENTE]` exigem decisão ou
> revisão humana.

## 1. Quem trata os dados

- **Instalação self-host (default):** você instala o GarraIA na sua própria
  máquina/servidor. Os dados processados pelo agente **ficam na sua
  infraestrutura**; o projeto GarraIA não tem acesso a eles. Nesse modelo,
  **você é o controlador** dos dados dos seus usuários finais.
- **Oferta cloud operada pelo projeto (quando existir):** o projeto passa a
  processar dados em nome do operador. `[PENDENTE: publicar esta seção apenas
  quando a oferta cloud existir; hoje o produto é self-host]`

## 2. Dados que o agente processa

| Categoria | Exemplos | Origem |
|---|---|---|
| Conteúdo de mensagens | texto, comandos, anexos citados pelo agente | canais conectados (Telegram, Discord, Slack, WhatsApp etc.) |
| Dados de conta | e-mail de login, display_name | cadastro no gateway |
| Metadados técnicos | `ip_inet`, `user_agent` | acesso ao gateway |
| Sessões e chaves | device de sessão, API keys (somente metadado; o hash nunca é exposto) | uso do produto |
| Registros de auditoria | eventos de conta e de ferramenta | motor de auditoria |

Base legal (self-host, para o operador): `[PENDENTE: contrato/legítimo
interesse do operador; o LIA para ip_inet/user_agent segue pendente no DPIA]`.

## 3. Como usamos e protegemos

- Sandbox de bash com allowlist por comando (`agent.bash_allowlist`) e
  denylist de segurança sempre à frente (v0.4.8).
- Vault criptografado para segredos (chaves de canal e de LLM).
- Sessões revogáveis; anonimização e exclusão de conta disponíveis nos
  endpoints de direitos do titular.
- LLM: o produto é **traga a sua chave (BYOK)** — em self-host, as mensagens
  vão ao provedor LLM que **você** configurar, regido pelos termos daquele
  provedor. `[PENDENTE: listar provedores padrão (OpenRouter, Anthropic,
  OpenAI) com links das políticas de cada um]`

## 4. Seus direitos (LGPD / GDPR)

O gateway implementa, para a conta autenticada:

- **Exportação** dos dados da conta: `GET /v1/me/export` (perfil, sessões,
  chaves de API, auditoria, grupos).
- **Anonimização**: `POST /v1/me/anonymize` (substitui identificadores por
  token, revoga sessões).
- **Exclusão**: `DELETE /v1/me` (soft-delete com tombstone).
- **Correção de perfil**: `PATCH /v1/me`.

Limitação conhecida: a exportação cobre dados de **conta**; o arquivo
completo de mensagens não é incluído hoje. `[PENDENTE: produto decidir se
extende o escopo]`

## 5. Encarregado (DPO) e contato

`[PENDENTE: designar encarregado (LGPD art. 41) e publicar canal de contato —
hoje o canal é o security/governance do projeto via GitHub e
security@garraia.org para vulnerabilidades]`

## 6. Suboperadores

Self-host: provedores conectados por você (LLM, canais) são regidos pelos
termos de cada um. Cloud operada (futuro): lista em
`docs/legal/dpa-template.md` §5, com processo de atualização.
`[PENDENTE]`

## 7. Retenção

Na instalação self-host, retenção é definida pelo operador (ex.: política de
limpeza de sessões: 90 dias após fechamento — default do produto). Na cloud
(futuro): conforme contrato. `[PENDENTE]`

## 8. Alterações desta política

Versões desta política serão registradas no repositório; mudanças materiais
serão anunciadas no CHANGELOG/docs com aviso razoável. `[PENDENTE: definir
canal de anúncio]`
