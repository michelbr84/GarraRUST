# GarraIA — Data Processing Agreement (DPA) — TEMPLATE / DRAFT

> **Status: RASCUNHO de template — NÃO VÁLIDO até revisão jurídica.**
> Este arquivo é o artefato de engenharia previsto em `docs/compliance/dpia.md`
> §"Contrato com suboperadores" e na issue de GA correspondente
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)). Ele existe
> para que a revisão jurídica tenha um texto concreto para corrigir — não para
> ser assinado como está. Todo item marcado `[PENDENTE — revisão legal]`
> precisa de validação de advogado antes de qualquer oferta comercial cloud.

Escopo deste template: qualquer deployment em que o GarraIA processa dados
pessoais de terceiros. O produto é **self-host por padrão** — nesse modelo o
operador self-host **é o controlador** e o projeto GarraIA fornece software,
sem relação de processor (ver `dpia.md` §"Papéis"). Este contrato aplica-se de
fato quando existir uma **oferta cloud operada pelo projeto** ou quando um
cliente enterprise exigir contrato de processamento para uso self-host.

---

## 1. Papéis e definições — classificação por fluxo

Papéis LGPD **não são fixos por organização**: a mesma entidade pode ser
controladora num fluxo e processor em outro. A classificação correta é
**por fluxo de tratamento**:

| Fluxo | Controlador | Processor / terceiro | Observação |
|---|---|---|---|
| Self-host: mensagens do agente | Operador self-host | LLM/canais que o operador conecta | Projeto GarraIA fornece software; não é processor |
| Garra Cloud: mensagens de conta cloud | Operador da conta cloud (cliente) | Projeto GarraIA (processor) | `[PENDENTE — revisão legal: confirmar entidade jurídica do processor]` |
| Garra Cloud: billing/cadastro | Projeto GarraIA | Processador de pagamento | Análise própria, não é este DPA de mensagens `[PENDENTE]` |
| Site garraia.org (comunidade/distribuição) | Projeto GarraIA | Hosting do site `[PENDENTE: confirmar]` | Fluxos fora do gateway |
| Desenvolvimento/CI do repositório | Projeto GarraIA | GitHub | Ver §5 — sem acesso presumido a conversas |

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
  injustificada** — a orientação de prática da ANPD (comunicação em até
  **3 dias úteis** na regra geral; ver `docs/compliance/incident-response.md`)
  vale como teto interno de referência para notificar o controlador;
  `[PENDENTE — revisão legal: fixar prazo contratual exato]`;
- Ao término: retornar ou destruir dados, conforme instrução do controlador.

## 4. Categorias de dados e titulares

Fonte: `docs/compliance/dpia.md` §2 (inventário por tabela). Categorias
principais: conteúdo de mensagens processadas pelo agente; `users.email`;
`display_name`; `ip_inet`; `user_agent`; sessões; chaves de API (somente
metadados — o hash nunca sai do servidor); registros de auditoria.
Titulares: usuários finais do agente, membros de grupos e contatos nos canais
conectados. `[PENDENTE — revisão legal: conferir exaustividade com o DPIA]`

## 5. Inventário de terceiros por fluxo (suboperadores)

Na oferta self-host, quem o operador conecta é **escolha do operador**; os
provedores upstream continuam sendo suboperadores do próprio operador, com os
contratos de cada um aplicando-se diretamente. O inventário abaixo descreve
os fluxos do projeto — a classificação é **por fluxo**, e "estar integrado"
não significa acesso a conversas:

| Terceiro | Fluxo | O que recebe | Retenção do terceiro | Instrumento / região |
|---|---|---|---|---|
| GitHub | Desenvolvimento e distribuição do repositório | Código, issues, PRs, artefatos de CI | Conforme GitHub Terms | Termos GitHub; dados de conversa do agente **não** fazem parte deste fluxo |
| OpenRouter | Rota LLM default (quando configurada) | Conteúdo da mensagem enviada para inferência | Conforme política do OpenRouter **e do provedor final escolhido** | https://openrouter.ai/privacy — inclui o provedor final |
| Anthropic | Rota LLM (quando configurada) | Conteúdo da mensagem | Conforme política Anthropic | https://www.anthropic.com/legal/privacy |
| OpenAI | Rota LLM (quando configurada) | Conteúdo da mensagem | Conforme política OpenAI | https://openai.com/policies/row-privacy-policy/ |
| Ollama (local) | Rota LLM local quando verificada em instalação self-host | Conteúdo da mensagem | Local, sem saída de rede | Fluxo local — sem transferência |
| Canais (WhatsApp/Meta, Telegram, Discord, Slack) | Entrega de mensagens | Mensagens trafegam pela plataforma do canal | Conforme cada plataforma | Termos de cada plataforma; listar canais efetivamente ativados |
| Lovable | Hospedagem do site garraia.org | Fluxos do site (navegação) — **confirmar infra efetiva** | `[PENDENTE]` | `[PENDENTE: confirmar se Lovable só publica ou também processa]` |
| Hospedagem/banco/storage/backups da Garra Cloud | Operação cloud | Dados das contas cloud | `[PENDENTE — a cloud não existe]` | `[PENDENTE: vendors reais da cloud, a preencher quando decididos]` |
| Pagamentos, e-mail transacional, observabilidade | Cadastro/faturamento/plataforma | Dados de billing, e-mails, telemetria | `[PENDENTE — quando implementados]` | `[PENDENTE: adicionar ao inventário no ato da implementação]` |

`[PENDENTE — revisão legal: o DPA precisa de processo de atualização
(notificação + direito de objeção), não desta lista estática]`.

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
(perfil, sessões, chaves de API, auditoria, grupos); a extensão da exportação
ao arquivo de mensagens foi decidida (2026-10-09) e está em implementação de
engenharia — registrar como limitação vigente até o código entregar.
`[PENDENTE — revisão legal + produto: SLA de 30s/10k mensagens prometido no
DPIA ainda não verificado]`

## 8. Transferência internacional

Transferências ocorrem quando o LLM provider ou canal é estrangeiro (ver §5,
coluna região). `[PENDENTE — revisão legal: base de transferência (cláusulas
contratuais padrão / adequação) para cada suboperador listado]`

## 9. Retorno e destruição

No término: por instrução do controlador, retornar dados em formato comum e/
ou destruir, com evidência. Na nuvem do projeto: exclusão da conta executa
soft-delete com tombstone e purge posterior (ver `DELETE /v1/me` no código);
restauração de backup reaplica exclusões. `[PENDENTE — revisão legal: prazo
de purge]`

## 10. Assinatura

| | |
|---|---|
| Entidade (controlador) | |
| Entidade (processor, quando cloud) | |
| Assinatura / data | |

`[PENDENTE — revisão legal: cláusula de foro (observar alternativa do foro
do consumidor — ver rascunho de ToS §10), responsabilidade e limite de
responsabilidade — ver também rascunho de ToS §7]`
