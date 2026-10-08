# GarraIA — Data Processing Agreement (DPA) — TEMPLATE / DRAFT

> **Status: RASCUNHO de template — NÃO VÁLIDO até revisão jurídica.**
> Este arquivo é o artefato de engenharia previsto em `docs/compliance/dpia.md`
> §"Contrato com suboperadores" e na issue de GA correspondente
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)). Ele existe
> para que a revisão jurídica tenha um texto concreto para corrigir — não para
> ser assinado como está. Todo item marcado `[PENDENTE — revisão legal]`
> precisa de validação de advogado antes de qualquer oferta comercial cloud.

Escopo deste template: qualquer deployment em que o GarraIA processa dados pessoais
de terceiros. O produto é **self-host por padrão** — nesse modelo o operador
self-host **é o controlador** e o projeto GarraIA fornece software, sem
relação de processor (ver `dpia.md` §"Papéis"). Este contrato aplica-se de
fato quando existir uma **oferta cloud operada pelo projeto** ou quando um
cliente enterprise exigir contrato de processamento para uso self-host.

---

## 1. Papéis e definições

- **Controlador**: entidade que determina as finalidades e os meios do
  tratamento (no self-host: o operador/empresa do cliente).
- **Processor**: quem trata dados em nome do controlador. Na oferta cloud
  operada pelo projeto: o projeto GarraIA. `[PENDENTE — revisão legal:
  confirmar qual entidade jurídica figura como processor na oferta cloud]`
- **Dados pessoais**: conforme LGPD art. 5º, I e GDPR art. 4(1).

## 2. Objeto, duração e natureza do tratamento

- **Objeto**: processamento de mensagens, metadados de sessão e dados de
  conta necessários ao funcionamento do agente (categorías na §4).
- **Duração**: enquanto vigente a relação contratual + períodos legais de
  retenção.
- **Natureza/finalidade**: prestação do serviço de agente de mensagens
  (resposta, automação, memória de contexto), conforme instruções do
  controlador. `[PENDENTE — revisão legal: limitar finalidades]`

## 3. Obrigações do processor

- Tratar os dados apenas conforme instruções documentadas do controlador;
- Garantir confidencialidade de quem tem acesso (pessoas autorizadas);
- Implementar as medidas técnicas da §6;
- Auxiliar o controlador no atendimento a direitos dos titulares (§7);
- Notificar o controlador de incidentes de segurança **sem dilatação
  injustificada** `[PENDENTE — revisão legal: prazo exato (GDPR art. 33
  prevê 72h para a autoridade; o prazo de notificação ao controlador deve
  ser menor e definido)]`;
- Ao término: retornar ou destruir dados, conforme instrução do controlador.

## 4. Categorias de dados e titulares

Fonte: `docs/compliance/dpia.md` §3–4 (tabela de dados pessoais). Categorias
principais: conteúdo de mensagens processadas pelo agente; `users.email`;
`display_name`; `ip_inet`; `user_agent`; sessões; chaves de API (somente
metadados — o hash nunca sai do servidor); registros de auditoria.
Titulares: usuários finais do agente, membros de grupos e contatos nos canais
conectados. `[PENDENTE — revisão legal: conferir exaustividade com o DPIA]`

## 5. Suboperadores

Na oferta self-host, quem o operador conecta é **escolha do operador**; os
provedores upstream continuam sendo suboperadores do próprio operador, com os
contratos de cada um aplicando-se diretamente. Na oferta cloud operada pelo
projeto, os suboperadores do projeto são, hoje, os provedores já integrados e
configuráveis:

| Suboperador | Papel | Política de dados |
|---|---|---|
| OpenAI | LLM (rota configurável) | https://openai.com/policies/row-privacy-policy/ |
| Anthropic | LLM (rota configurável) | https://www.anthropic.com/legal/privacy |
| OpenRouter | Agregador de LLM (rota default) | https://openrouter.ai/privacy |

`[PENDENTE — revisão legal: lista é dinâmica — o DPA precisa de processo de
atualização (notificação + direito de objeção), não de lista estática]`.
Canais de mensageria conectados (Telegram, Discord, Slack, WhatsApp etc.)
processam mensagens conforme os termos de cada plataforma; o operador cloud
deve listá-los ao contratar canal. `[PENDENTE — revisão legal]`

## 6. Medidas técnicas e organizacionais (verificáveis no repositório)

Medidas que o código sustenta hoje (referência de commit/PR pode ser anexada
na revisão):

- **Self-host é o default**: em instalação self-host, dados ficam na máquina
  do operador; o projeto não os vê.
- **Process isolation**: bash roda com fronteira `agent.bash_allowlist`
  (modo `HostComAllowlist`, v0.4.8) e denylist de safety gate.
- **Vault criptografado** para segredos de canal/LLM.
- **Sessões**: revogação individual (`DELETE /v1/me/sessions/{id}`) e
  revogação geral na exclusão/anonimização da conta.
- **Direitos do titular implementados**: exportação (`GET /v1/me/export`),
  anonimização (`POST /v1/me/anonymize`), exclusão (`DELETE /v1/me`),
  atualização de perfil (`PATCH /v1/me`).
- **Auditoria**: eventos de conta (`GET /v1/me/audit`).
- `[PENDENTE — revisão legal: alinhar este item ao nível de medida exigido
  pelo cliente enterprise; hoje é descrição técnica, não certificação]`

## 7. Assistência a direitos dos titulares

O processor auxilia o controlador mediante os endpoints acima (art. 20 LGPD /
arts. 15, 17, 20 GDPR). Exportação implementada em escopo de **conta**
(perfil, sessões, chaves de API, auditoria, grupos); exportação de arquivo
completo de mensagens ainda não é coberta pelo endpoint — registrar como
limitação conhecida. `[PENDENTE — revisão legal + produto: SLA de 30s/10k
mensagens prometido no DPIA ainda não verificado]`

## 8. Transferência internacional

`[PENDENTE — revisão legal: base de transferência (cláusulas contratuais
padrão / adequação) para os suboperadores listados na §5]`

## 9. Retorno e destruição

No término: por instrução do controlador, retornar dados em formato comum e/
ou destruir, com evidência. Na nuvem do projeto: exclusão da conta executa
soft-delete com tombstone e purge posterior (ver `DELETE /v1/me` no código).
`[PENDENTE — revisão legal: prazo de purge]`

## 10. Assinatura

| | |
|---|---|
| Entidade (controlador) | |
| Entidade (processor, quando cloud) | |
| Assinatura / data | |

`[PENDENTE — revisão legal: cláusula de foro, responsabilidade e limite de
responsabilidade — ver também rascunho de ToS]`
