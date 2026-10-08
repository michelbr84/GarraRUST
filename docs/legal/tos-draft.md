# GarraIA — Termos de Serviço — RASCUNHO

> **Status: RASCUNHO — NÃO PUBLICAR até revisão jurídica.** Artefato de
> engenharia para a issue de GA
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)). Onde o texto
> depende de escolha comercial/jurídica, está marcado `[PENDENTE]`. O código do
> projeto é licenciado MIT (ver `LICENSE`); isto define o software, não
> necessariamente o serviço ao redor dele. `[PENDENTE: decisão do maintainer
> sobre licença do serviço vs. código — rastreada na issue de negócio]`

## 1. O que é o GarraIA

GarraIA é um agente de mensagens de **auto-hospedagem (self-host)**: você
instala o software na sua própria infraestrutura e ele opera sob o seu
controle. Versões, canais e capacidades são documentados no repositório e no
CHANGELOG.

## 2. Licença do código

MIT — ver `LICENSE`. Você pode usar, modificar e redistribuir o código sob os
termos dessa licença. `[PENDENTE: qualquer serviço/comercialização redor do
produto precisa de decisão do maintainer sobre o que permanece MIT]`

## 3. Conta e acesso

Funcionalidades de gateway (usuários, sessões, API keys) operam na sua
instalação. Você é responsável por quem tem acesso ao seu deployment e pela
segurança das suas chaves (canal/LLM) no vault.

## 4. Uso aceitável

Você não deve usar o GarraIA para: violar lei aplicável; invadir privacidade
de terceiros; enviar spam ou conteúdo ilícito pelos canais conectados; tentar
contornar as barreiras de segurança do produto (sandbox, allowlist). `[PENDENTE:
revisão legal: lista e consequências]`

## 5. Dados

Em self-host, **os dados do seu agente são seus**, processados na sua
infraestrutura (ver `docs/legal/privacy-policy-draft.md`). O projeto não
acessa esses dados. Responsabilidade sobre dados de usuários finais seus é do
operador (controlador).

## 6. Disponibilidade e suporte

Self-host: **sem SLA contratual** — a disponibilidade depende da sua
infraestrutura; suporte é comunitário, conforme `SUPPORT.md` e
`docs/operations/support-policy.md`. Justificativa e opções de SLA futuro:
`docs/operations/sla-decision-brief.md`. `[PENDENTE: se existir oferta cloud
ou suporte pago, esta seção muda e exige contrato próprio]`

## 7. Garantia e responsabilidade

O software é fornecido **"como está"**, sem garantias, na medida permitida
pela lei aplicável e pela licença MIT. `[PENDENTE — revisão legal: cláusula
de limite de responsabilidade, exclusões e interação com direitos
irrenunciáveis do consumidor quando aplicável]`

## 8. Encerramento

Você pode parar de usar o software a qualquer momento; em self-host, basta
desinstalar — os dados permanecem sob seu controle (e podem ser apagados pelos
endpoints de direitos do titular ou pela sua própria operação de banco).
`[PENDENTE: termos para oferta cloud — retenção, purge, evidência]`

## 9. Alterações

Mudanças materiais destes termos serão anunciadas com aviso razoável via
repositório/documentação. `[PENDENTE: definir aviso mínimo e canal]`

## 10. Contato

Dúvidas sobre estes termos: canal do projeto (GitHub Discussions) e
`[PENDENTE: e-mail jurídico/designação de representante]`
