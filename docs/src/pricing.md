# Preços e Planos

> **Status: rascunho interno, não publicado, não vigente.**
>
> Esta página descreve uma **proposta comercial**, não uma oferta ativa. Hoje **não existe**:
> sistema de cobrança, contas de usuário, hospedagem gerenciada, backups automáticos,
> contrato de SLA ou certificação SOC 2. O produto é open-source (MIT) e roda
> localmente sem custo — é o único caminho que existe de fato.
>
> Os números abaixo são hipóteses de precificação para decisão do maintainer
> (rastreado em [#1576](https://github.com/michelbr84/GarraRUST/issues/1576)), não
> preços praticados. Termos contratuais (ToS, DPA, política de privacidade) dependem
> de revisão jurídica externa ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)).
> Nada aqui é promessa de disponibilidade, de conformidade ou de suporte.

O GarraIA é open-source (MIT) e pode ser executado localmente sem nenhum custo. Os planos gerenciados **não existem ainda**; a tabela abaixo é a proposta de como seriam.

---

## Planos disponíveis

### Free — Gratuito para sempre

Ideal para uso pessoal, experimentação e projetos de código aberto.

**Inclui:**
- 1 canal de comunicação (Telegram, Discord, Slack, WhatsApp ou iMessage)
- 1 provedor LLM configurado
- Memória persistente (SQLite local)
- Suporte a MCP (stdio)
- Plugins WASM
- Atualizações automáticas

**Limitações:**
- Hospedagem própria (self-hosted)
- Suporte apenas pela comunidade (GitHub Issues, Discord)

**Como começar:**
```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
garraia init
```

---

### Pro — R$ 50/mês (ou US$ 10/mês)

Para profissionais e equipes pequenas que querem eliminar a complexidade de infraestrutura.

**Tudo do Free, mais (proposto):**
- Canais ilimitados (Telegram + Discord + Slack + WhatsApp + iMessage simultaneamente)
- Provedores LLM ilimitados
- Hospedagem gerenciada na nuvem — **não operacional hoje**; exige infraestrutura e custo de infra ainda não modelados
- Dashboard web para monitoramento — parcialmente existente no gateway local; versão gerenciada não existe
- Backups automáticos diários — **não operacional hoje**
- Acesso prioritário a novos recursos — depende de capacidade de suporte, que não existe
- Suporte via e-mail — **não operacional hoje**; o canal real hoje é GitHub Issues (best effort)

**SLA proposto (não contratual):** 99,5% de uptime mensal — **não há infraestrutura de produção multi-cliente que sustente essa promessa, nem acordo que a garanta.** Definir SLA exige decisão do maintainer e rede de clientes que ainda não existe.

**Limite de uso proposto:** 50.000 mensagens/mês — sem sistema de medição ou cobrança implementado.

**Como assinar:** não existe caminho de assinatura hoje. O produto é distribuído pelo repositório e pelo `install.sh`; quando houver cobrança, esta seção terá o fluxo real.

---

### Enterprise — Preço sob consulta

Para empresas que precisam de customização, compliance e suporte dedicado.

**Tudo do Pro, mais (proposto):**
- Contrato SLA personalizado — **não existe contrato nem infraestrutura que o sustente.** Qualquer percentual aqui seria promessa sem lastro.
- Implantação on-premise ou nuvem privada — o produto já roda localmente (é o modelo atual); implantação assistida não existe como serviço.
- Integração com IdP corporativo (SSO via SAML 2.0 / OIDC) — **não implementado**; o gateway autentica por API key/token próprio.
- Conformidade — **não há certificação SOC 2, nem registro/auditoria LGPD concluído.** A análise de prontidão de conformidade (DPIA, LIA, tabletop) está aberta em [#1565](https://github.com/michelbr84/GarraRUST/issues/1565); a política de privacidade e os papéis legais dependem de revisão jurídica ([#1575](https://github.com/michelbr84/GarraRUST/issues/1575)).
- Multi-tenancy gerenciado — **não implementado**; o gateway opera por principal/sessão, não por tenant de cliente.
- Treinamento e onboarding para a equipe — não operacional.
- Suporte dedicado — **não operacional**; não há equipe nem canal contratual.
- Desenvolvimento de features customizadas — decisão comercial do maintainer, sem processo registrado.

**Como contratar:**
Entre em contato em [enterprise@garraia.cloud](mailto:enterprise@garraia.cloud) ou [agende uma demo](https://garraia.cloud/demo).

---

## Comparativo de planos (proposto — nenhum dos itens "Sim" existe hoje)

| Recurso | Free (real) | Pro (proposto) | Enterprise (proposto) |
|---------|------|-----|------------|
| Canais | 1 | Ilimitados | Ilimitados |
| Provedores LLM | 1 | Ilimitados | Ilimitados |
| Hospedagem | Self-hosted | Gerenciada — **não operacional** | On-premise — o produto já roda local; serviço assistido não existe |
| Dashboard web | Não | Parcial (gateway local) | — |
| Backups automáticos | Não | **Não operacional** | — |
| Mensagens/mês | Ilimitadas (self-hosted) | 50.000 (sem medição/cobrança) | Sob contrato |
| SLA | — | **Proposto 99,5% — sem infraestrutura nem acordo** | **Proposto — sem contrato** |
| SSO / SAML | Não | Não | **Não implementado** |
| Conformidade (SOC 2, LGPD) | — | — | **Nenhuma certificação; conformidade em aberto ([#1565](https://github.com/michelbr84/GarraRUST/issues/1565))** |
| Suporte | Comunidade (GitHub Issues) | **E-mail — não operacional** | **Slack dedicado — não operacional** |
| Preço | Gratuito | R$ 50/mês (hipótese) | Sob consulta (hipótese) |

---

## Perguntas frequentes

> Estas respostas descrevem o estado **real** do produto hoje, não a proposta de planos acima.

**O código-fonte continuará sendo open-source?**

Sim. O GarraIA é e continuará MIT. Qualquer modelo de receita futuro financiaria infraestrutura gerenciada, não restringiria o código — mas **nenhum plano pago existe hoje**, então nada há a financiar.

**Posso hospedar eu mesmo?**

Sim — é o único modo que existe. O `install.sh` e os pacotes de release instalam o produto localmente, sem custo e sem limites de canal ou provedor. Não há hospedagem gerenciada operacional.

**Como é cobrado o excesso de mensagens?**

Não é cobrado: **não existe sistema de cobrança, medição por mensagem nem dashboard de uso.** O produto roda localmente e não conta mensagens.

**Posso cancelar a qualquer momento?**

Não há assinatura para cancelar. O produto é gratuito e self-hosted.

**O GarraIA armazena minhas conversas?**

No modo self-hosted (o único existente), as conversas ficam **na sua máquina** — SQLite local, memória do agente, sem envio a servidores do projeto. Não existe plano gerenciado que armazene conversas em infraestrutura nossa, e **a política de privacidade ainda não foi publicada** (em revisão jurídica, [#1575](https://github.com/michelbr84/GarraRUST/issues/1575)).
