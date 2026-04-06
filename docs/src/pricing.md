# Preços e Planos

O GarraIA é open-source (MIT) e pode ser executado localmente sem nenhum custo. Os planos gerenciados existem para quem prefere não hospedar a própria infraestrutura.

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

**Tudo do Free, mais:**
- Canais ilimitados (Telegram + Discord + Slack + WhatsApp + iMessage simultaneamente)
- Provedores LLM ilimitados
- Hospedagem gerenciada na nuvem (sem servidor próprio)
- Dashboard web para monitoramento
- Backups automáticos diários
- Acesso prioritário a novos recursos
- Suporte via e-mail (resposta em até 48 horas)

**SLA:** 99.5% de uptime mensal

**Limite de uso:** 50.000 mensagens/mês (excesso: R$ 0,001 por mensagem)

**Como assinar:**
Acesse [garraia.cloud/pricing](https://garraia.cloud/pricing) e crie sua conta.

---

### Enterprise — Preço sob consulta

Para empresas que precisam de customização, compliance e suporte dedicado.

**Tudo do Pro, mais:**
- Contrato SLA personalizado (até 99.99% de uptime)
- Implantação on-premise ou nuvem privada
- Integração com IdP corporativo (SSO via SAML 2.0 / OIDC)
- Auditoria de logs e conformidade (SOC 2, LGPD)
- Multi-tenancy gerenciado
- Treinamento e onboarding para a equipe
- Suporte dedicado via Slack privado (resposta em até 4 horas em horário comercial)
- Possibilidade de desenvolvimento de features customizadas

**Como contratar:**
Entre em contato em [enterprise@garraia.cloud](mailto:enterprise@garraia.cloud) ou [agende uma demo](https://garraia.cloud/demo).

---

## Comparativo de planos

| Recurso | Free | Pro | Enterprise |
|---------|------|-----|------------|
| Canais | 1 | Ilimitados | Ilimitados |
| Provedores LLM | 1 | Ilimitados | Ilimitados |
| Hospedagem | Self-hosted | Gerenciada | On-premise / nuvem privada |
| Dashboard web | Não | Sim | Sim |
| Backups automáticos | Não | Diários | Personalizável |
| Mensagens/mês | Ilimitadas (self-hosted) | 50.000 + excesso | Sob contrato |
| SLA | — | 99.5% | Até 99.99% |
| SSO / SAML | Não | Não | Sim |
| Conformidade (SOC 2, LGPD) | — | — | Sim |
| Suporte | Comunidade | E-mail (48h) | Slack dedicado (4h) |
| Preço | Gratuito | R$ 50/mês | Sob consulta |

---

## Perguntas frequentes

**O código-fonte continuará sendo open-source?**

Sim. O GarraIA é e sempre será MIT. Os planos pagos financiam a infraestrutura gerenciada, não o código.

**Posso hospedar o Pro eu mesmo?**

O plano Pro se refere à hospedagem gerenciada. Se preferir self-hosted, o plano Free (open-source) não tem limitações de canais ou provedores — apenas suporte.

**Como é cobrado o excesso de mensagens no Pro?**

O excesso é cobrado automaticamente no cartão cadastrado ao final de cada mês. Você pode configurar alertas de uso no dashboard.

**Posso cancelar a qualquer momento?**

Sim. Não há fidelidade. O cancelamento é efetivo no final do período faturado.

**O GarraIA armazena minhas conversas nos planos gerenciados?**

As conversas são armazenadas apenas para o funcionamento da memória do agente. Consulte nossa [Política de Privacidade](https://garraia.cloud/privacy) para detalhes.
