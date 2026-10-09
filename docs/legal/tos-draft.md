# GarraIA — Termos de Serviço — RASCUNHO

> **Status: RASCUNHO — NÃO PUBLICAR até revisão jurídica.** Artefato de
> engenharia para a issue de GA
> ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)). Onde o texto
> depende de escolha comercial/jurídica, está marcado `[PENDENTE]`. As decisões
> de negócio do maintainer de 2026-10-09 estão refletidas aqui (contextos de
> operação, responsabilidades BYOK, foro, suspensão), mas **não substituem**
> revisão de advogado. O código do projeto é licenciado MIT (ver `LICENSE`);
> isto define o software, não necessariamente o serviço ao redor dele.

## 1. Contextos de operação

O GarraIA opera em três contextos distintos, com consequências diferentes:

1. **GarraRUST Community (self-host, vigente):** você instala o software MIT
   na sua própria infraestrutura e opera sob o seu controle. Uso pessoal ou
   comercial é permitido pela licença, sem limites artificiais de canal,
   agente ou provedor (ver `docs/src/pricing.md`).
2. **Garra Cloud (PLANEJADO — não é oferta disponível):** modelo gerenciado
   pelo projeto, em programa beta limitado quando existir. Enquanto não
   existir, não há serviço cloud para contratar.
3. **Site e comunidade (garraia.org, repositório, canais):** dados próprios
   de navegação e participação, tratados conforme
   `docs/legal/privacy-policy-draft.md`.

Estes termos cobrem o contexto 1 integralmente; o contexto 2 será contratado
por termos próprios quando a oferta existir; o contexto 3 já é regido pela
política de privacidade.

## 2. Licença do código

MIT — ver `LICENSE`. Você pode usar, modificar e redistribuir o código,
inclusive comercialmente, sob os termos dessa licença. A licença cobre o
**software**; serviços gerenciados ao redor dele (quando existirem) são
regidos pelos termos do serviço correspondente.

## 3. Conta e acesso

**Self-host:** funcionalidades de gateway (usuários, sessões, API keys)
operam na sua instalação. Você é responsável por quem tem acesso ao seu
deployment e pela segurança das suas chaves no vault.

**Responsabilidade sobre chaves de LLM (BYOK):**

- **Self-host com suas próprias chaves:** a responsabilidade pelo uso,
  custo e vazamento das chaves fornecidas por você é **sua** — as mensagens
  vão aos provedores que você configurou, regidos pelos termos de cada um.
- **Garra Cloud (quando existir), chaves de serviço do operador:** o operador
  da cloud é responsável pelas chaves de serviço que ele mesmo gerencia.
- **Garra Cloud, chave do cliente armazenada:** se a cloud guardar chave de
  LLM fornecida pelo cliente, a responsabilidade pela guarda daquela chave é
  do operador (armazenamento em vault criptografado).

A afirmação de que "a chave fica local" vale **apenas** para o fluxo
self-host — em cloud, o processamento acontece na infraestrutura do operador.

## 4. Uso aceitável

Você não deve usar o GarraIA para: violar lei aplicável; invadir privacidade
de terceiros; enviar spam ou conteúdo ilícito pelos canais conectados; tentar
contornar as barreiras de segurança do produto (sandbox, allowlist).
`[PENDENTE: revisão legal — lista e consequências]`

## 5. Dados

Em self-host, **os dados do seu agente são seus**, processados na sua
infraestrutura (ver `docs/legal/privacy-policy-draft.md`). O projeto não
acessa esses dados. Responsabilidade sobre dados de usuários finais seus é do
operador (controlador). Registros de acesso guardados pelo operador self-host
seguem as obrigações do Marco Civil da Internet (Lei 12.965/2014).

## 6. Disponibilidade e suporte

**SLA de melhor esforço — sem percentual contratual.**

- **Self-host:** a disponibilidade depende da sua infraestrutura; o projeto
  não a controla e não promete percentual algum. Suporte é comunitário,
  conforme `SUPPORT.md` e `docs/operations/support-policy.md`.
- **Garra Cloud (quando existir):** lançamento sem promessa contratual de
  uptime, comunicado com clareza na contratação. SLA numérico é possivelmente
  futuro, restrito a contratos enterprise; não é regra geral dos planos.
- Justificativa e opções de SLA futuro:
  `docs/operations/sla-decision-brief.md`.

## 7. Garantia e responsabilidade

**Software (self-host, MIT):** fornecido **"como está"**, sem garantias, na
medida permitida pela lei aplicável e pela licença MIT.

**Serviço gerenciado (Garra Cloud, quando existir):** os termos do serviço
observam o Código de Defesa do Consumidor (Lei 8.078/1990) — direitos
irrenunciáveis do consumidor não são afastados por cláusula de "como está".
`[PENDENTE — revisão legal: redação da cláusula de responsabilidade do
serviço cloud, alinhada ao CDC]`

## 8. Idade mínima

O uso da **Garra Cloud** exige, no lançamento, **maioria civil (18 anos)**.
O self-host é de responsabilidade do operador definir sua política de idade.
`[PENDENTE: avaliação específica do ECA Digital (Lei 14.841/2024) quanto a
dados de menores em mensagens processadas pelo agente — decisão própria,
não resolvida por esta cláusula]`

## 9. Suspensão e encerramento

Medidas aplicáveis ao uso do serviço (Cloud, quando existir) seguem
critérios **objetivos**:

- fraude ou tentativa de fraude;
- spam em escala ou abuso de canais conectados;
- ilegalidade (uso do serviço para finalidade vedada por lei);
- comprometimento de segurança que ponha em risco a plataforma ou terceiros;
- violação material destes termos.

**Proporcionalidade:** a medida é proporcional à gravidade — advertência,
restrição de função, suspensão temporária ou encerramento. Suspensão imediata
ocorre apenas em emergências de segurança; nos demais casos, aviso prévio
quando possível.

**Contestação:** o usuário pode contestar medida aplicada pelo canal de
suporte indicado na §12, com resposta humana. `[PENDENTE: definir prazo de
resposta de contestação]`

**Encerramento por iniciativa do usuário:** a qualquer momento. Em self-host,
basta desinstalar — os dados permanecem sob seu controle. Na cloud (quando
existir): saldo de crédito pré-pago não consumido é reembolsado conforme
regras da §11; acesso aos dados antes do encerramento é garantido via
exportação; purge de dados segue retenção documentada na política de
privacidade. `[PENDENTE: prazo de purge pós-encerramento]`

## 10. Foro e entidades

**Este documento é brasileiro.** Cláusula de foro exclusivo no exterior **não
é adotada**: em contratos de adesão, o consumidor brasileiro mantém a opção
do foro do seu domicílio (Código de Processo Civil art. 63, IX, e Marco Civil
da Internet art. 22, parágrafo único). Qualquer foro indicado respeita essa
alternativa.

`[PENDENTE: identificação da entidade jurídica que opera a Garra Cloud —
razão social, CNPJ e endereço — a incluir antes da publicação. Este texto não
inventa esses dados.]`

## 11. Planos, cobrança e limites de uso

A grade de planos, quotas e regras de cobrança está em
`docs/src/pricing.md`, que integra estes termos por referência. Pontos
contratuais:

- A Garra Cloud será oferecida primeiro como **programa beta limitado**; a
  inscrição no beta não é assinatura de plano pago.
- Conversão de trial/beta para plano pago exige **aceite explícito** — nunca
  automática.
- Cobros extras automáticos (ex.: consumo acima da franquia de LLM) só
  ocorrem com **autorização específica do cliente** e teto definido pelo
  cliente; sem isso, o esgotamento da franquia leva a BYOK ou recharge
  pré-pago, nunca a fatura posterior.
- Pagamentos em BRL (cartão e Pix) conforme meios efetivamente disponíveis
  no lançamento. `[PENDENTE: confirmar processador, entidade cobradora e
  modalidade de recorrência do Pix antes de anunciar]`

## 12. Alterações e contato

Mudanças materiais destes termos serão anunciadas com aviso razoável via
repositório/documentação. `[PENDENTE: definir aviso mínimo e canal]`

Dúvidas sobre estes termos: canais em `SUPPORT.md` (GitHub Discussions para
dúvidas gerais; canal privado para questões que contenham dados pessoais,
credenciais ou vulnerabilidades).
`[PENDENTE: e-mail jurídico/designação de representante — não inventado aqui]`
