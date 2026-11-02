# Usar o plano do ChatGPT (Sign in with ChatGPT)

O provider `openai-codex` deixa o GarraIA usar o **seu plano do ChatGPT** pelo
fluxo oficial da OpenAI para apps open-source
([Sign in with ChatGPT — token sharing](https://developers.openai.com/siwc/token-sharing-open-source)).
Não precisa de API key: você entra com a sua conta do ChatGPT no navegador e
autoriza o uso do plano.

O que o GarraIA faz, e o que não faz:

- Inferência no endpoint **público** `POST https://api.openai.com/v1/responses`,
  sempre com `store: false` e `stream: true`. Nunca os endpoints privados do
  ChatGPT.
- Não tem acesso às suas conversas do ChatGPT.
- Não lê credenciais de outras ferramentas (Codex CLI, Hermes): o login é do
  GarraIA, feito por você.
- Quando o plano não atende (limite de uso atingido, conta não elegível,
  sessão expirada), o pedido passa para o provider que **você** configurou em
  `agent.fallback_providers` — por exemplo o OpenRouter. Sem fallback
  configurado, o erro aparece para você; o GarraIA nunca cria um fallback nem
  usa uma API key que você não configurou.

## Entrar pelo painel (porta 3888)

1. Abra `http://127.0.0.1:3888` e vá em **Providers**.
2. No cartão **ChatGPT**, deixe marcado "Usar como provedor padrão" (GPT-6.1
   Sol, raciocínio High) e clique em **Continuar com ChatGPT**.
3. Na aba da OpenAI, entre e autorize o uso do plano. A aba volta para o
   GarraIA e o cartão passa para **Conectado**.
4. Escolha modelo, **Raciocínio** e **Velocidade**, clique em **Salvar** e
   depois em **Testar conexão** — é uma inferência curta e real, que mostra o
   texto, o modelo, o `reasoning.effort` enviado, o tier que a OpenAI serviu e
   os trechos recebidos em streaming.

O cartão mostra os estados: não conectado, aguardando autenticação, conectado,
uso do plano pendente/negado, sessão expirada, erro de autenticação e provedor
indisponível. **Trocar de conta** registra outra conta ou workspace;
**Sair** revoga a sessão e apaga os tokens locais.

> O callback do login é `http://127.0.0.1:<porta>/auth/callback` (porta 1455,
> ou outra livre). Por isso o login pelo painel exige o navegador **na mesma
> máquina** do gateway. Para um gateway numa VM, veja "Gateway remoto" abaixo.

## Entrar pela linha de comando

```bash
garra chatgpt login --padrao     # abre o navegador; grava llm.chatgpt e a torna padrao
garra chatgpt status             # conta em uso e a entrada do config.yml
garra chatgpt modelos            # catalogo da sua conta (GET /v1/models)
garra chatgpt testar             # inferencia curta e real, com streaming
garra chatgpt configurar --modelo gpt-6.1-sol --esforco high --velocidade thinking --padrao
garra chatgpt testar --velocidade fast   # testa sem gravar
garra chatgpt login --nova-conta # outra conta ou workspace
garra chatgpt usar <email|client_id>
garra chatgpt logout
```

## A entrada no `config.yml`

```yaml
llm:
  chatgpt:
    provider: openai-codex
    model: gpt-6.1-sol
    reasoning_effort: high   # none | minimal | low | medium | high | xhigh | max
    speed: thinking          # thinking | fast | ultrafast
agent:
  default_provider: chatgpt
  fallback_providers: [openrouter]   # assume quando o plano não atende
```

Sem `api_key` e sem `base_url`: a credencial é o login salvo e o endpoint é o
oficial. O `reasoning.effort` vai em **todo** pedido. Quando você troca o
padrão para o ChatGPT, o provider padrão anterior vira o primeiro do
`fallback_providers` em vez de sumir.

## Raciocínio e velocidade

| Opção | O que é enviado | Observação |
| --- | --- | --- |
| Minimal, Low, Medium, High, Extra High, Max | `reasoning.effort`: `minimal` … `xhigh`, `max` | o cartão desativa o que o modelo não oferece no catálogo da sua conta |
| Ultra | — | existe só no Codex; a Responses API pública recusa, por isso fica desativado |
| Thinking (padrão) | nenhum `service_tier` | raciocínio no tier normal |
| Fast | `service_tier: priority` | o catálogo chama o tier `priority` de "Fast" |
| Ultrafast | `service_tier: ultrafast` | só ativo quando o catálogo da conta oferece |

A OpenAI decide o tier que de fato atende: o **Testar conexão** mostra o
pedido e o servido (por exemplo, "pedido: fast • servido: default"). Com o
`gpt-6.1-sol`, `minimal` e `none` são recusados pela API.

## Quando o limite do plano acaba

O limite de uso do plano é da OpenAI. Quando ela responde
`subscription_sharing_usage_limit_exceeded`, o GarraIA **não repete** o pedido
pelo plano e passa direto para o primeiro provider de
`agent.fallback_providers` — no chat (streaming) e nas chamadas em lote. O
mesmo vale sem conta conectada ou com a sessão expirada. Ao tornar o ChatGPT
o padrão, o provider padrão anterior (por exemplo o OpenRouter) já entra como
primeiro fallback.

Acompanhe o uso em ChatGPT → Configurações → Uso
(<https://chatgpt.com/#settings/Usage>).

## Onde ficam as credenciais

`<config_dir>/credentials/openai-codex/` (em Linux, normalmente
`~/.config/garraia/credentials/openai-codex/`):

| Arquivo | Conteúdo |
| --- | --- |
| `host.json` | o `ext_agent_host_id` deste computador (`urn:uuid:…`, opaco, sem dado seu) |
| `contas/<client_id>.json` | uma conta: identidade validada, tokens, escopos, validade |
| `ativa.json` | qual conta está em uso |

Tudo gravado de forma atômica com permissão `0600`. Tokens nunca vão para log.
O access token dura 1 hora e é renovado sozinho; a renovação é serializada
entre gateway, CLI e servidor MCP, porque o refresh token **rotaciona**.

## Gateway remoto (VM)

Siga o guia oficial [self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms):

1. Na VM, crie o host id antes de importar: `garra chatgpt status` (cria
   `host.json`).
2. No seu computador, `garra chatgpt login`.
3. Copie por SSH o arquivo `contas/<client_id>.json` (e o `ativa.json`) para
   a mesma pasta na VM, **sem** sobrescrever o `host.json` da VM.

## Problemas comuns

| Mensagem | O que fazer |
| --- | --- |
| "nenhuma conta do ChatGPT conectada" | `garra chatgpt login` ou o botão no painel |
| "entrou sem permitir o uso do plano" | entre de novo e autorize o uso do plano |
| "a sessão do ChatGPT … expirou ou foi desconectada" | entre de novo; acontece se você desconectou o app nas configurações do ChatGPT |
| "limite de uso do plano do ChatGPT atingido" | o fallback configurado assume; sem fallback, o uso pelo plano fica pausado até o limite renovar (ChatGPT → Configurações → Uso) |
| "o uso do plano não está disponível para esta conta" | a conta, o workspace ou a política não permitem; use outro provider |
| "recurso não suportado pelo uso do plano (param: …)" | o modelo ou a ferramenta pedida não está liberado para o plano |

## Limitações (prévia da OpenAI)

- O catálogo de modelos vem da sua conta; um modelo no catálogo só está
  confirmado depois de uma inferência real (**Testar conexão**).
- Ferramentas hospedadas (geração de imagem, file search, Code Interpreter,
  MCP hospedado) não estão disponíveis por este caminho; as ferramentas locais
  do GarraIA funcionam como funções.
- Áudio/vídeo como entrada e a Files API não são suportados.
- Parâmetros como `temperature` e `max_output_tokens` não são enviados.
