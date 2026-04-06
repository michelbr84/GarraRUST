# Conectar Bot do Telegram

Este guia mostra como criar um bot no Telegram e conectá-lo ao GarraIA passo a passo.

---

## Pré-requisitos

- GarraIA instalado e configurado (veja [Início Rápido](../getting-started.md))
- Conta no Telegram
- Servidor GarraIA em execução (`garraia start`)

---

## Passo 1 — Criar o bot no Telegram

1. Abra o Telegram e inicie uma conversa com [@BotFather](https://t.me/BotFather)
2. Envie o comando `/newbot`
3. Escolha um **nome de exibição** para o bot (ex: `Meu Assistente IA`)
4. Escolha um **username** terminado em `bot` (ex: `meu_assistente_ia_bot`)
5. O BotFather retornará um token no formato:

```
1234567890:ABCdefGHIjklMNOpqrSTUvwxYZ
```

Guarde esse token — ele será necessário na configuração.

---

## Passo 2 — Configurar o canal no GarraIA

Abra `~/.garraia/config.yml` e adicione a seção `channels`:

```yaml
channels:
  telegram:
    type: telegram
    enabled: true
    bot_token: "1234567890:ABCdefGHIjklMNOpqrSTUvwxYZ"
    # Opcional: restrinja quem pode interagir com o bot
    # allowlist:
    #   - 123456789   # seu ID numérico do Telegram
    #   - 987654321
```

Para armazenar o token de forma segura no cofre (recomendado):

```bash
garraia vault set TELEGRAM_BOT_TOKEN "1234567890:ABCdefGHIjklMNOpqrSTUvwxYZ"
```

Com o token no cofre, use a variável de ambiente no config:

```yaml
channels:
  telegram:
    type: telegram
    enabled: true
    # bot_token é resolvido automaticamente do cofre via TELEGRAM_BOT_TOKEN
```

---

## Passo 3 — Reiniciar o servidor

```bash
garraia restart
# ou, se estiver rodando em foreground:
# Ctrl+C para parar, depois:
garraia start
```

O GarraIA detecta a configuração do Telegram e inicia automaticamente o polling de mensagens:

```
[INFO] Canal Telegram inicializado: @meu_assistente_ia_bot
[INFO] Iniciando polling de mensagens...
```

---

## Passo 4 — Testar o bot

1. Abra o Telegram e inicie uma conversa com o seu bot pelo username
2. Envie `/start` ou qualquer mensagem
3. Aguarde a resposta do GarraIA

Se o canal tiver `allowlist` configurada, o bot enviará um código de pareamento na primeira mensagem:

```
Para usar este bot, envie o código de pareamento exibido no terminal do GarraIA.
```

Verifique o terminal do GarraIA para obter o código de 6 dígitos e envie-o ao bot.

---

## Configurações avançadas

### Allowlist de usuários

Descubra seu ID numérico do Telegram via [@userinfobot](https://t.me/userinfobot):

```yaml
channels:
  telegram:
    type: telegram
    enabled: true
    bot_token: "SEU_TOKEN"
    allowlist:
      - 123456789
      - 987654321
```

### Streaming de respostas

O GarraIA envia respostas em tempo real via Telegram, usando MarkdownV2 para formatação. Não há configuração adicional necessária — o streaming está ativo por padrão.

### Indicadores de digitação

O bot exibe o indicador "digitando..." enquanto o LLM processa a resposta. Esse comportamento é automático.

---

## Verificação

Verifique se o canal está ativo:

```bash
curl http://127.0.0.1:3888/api/runtime/state | jq '.channels'
```

Saída esperada:

```json
{
  "channels": {
    "telegram": {
      "status": "connected",
      "bot_username": "meu_assistente_ia_bot"
    }
  }
}
```

---

## Resolução de problemas

**O bot não responde:**

- Verifique se o token está correto: `garraia vault get TELEGRAM_BOT_TOKEN`
- Confirme que o servidor está rodando: `curl http://127.0.0.1:3888/health`
- Verifique os logs: `garraia logs --channel telegram`

**Erro "Unauthorized" no log:**

O token do bot está inválido ou foi revogado. Gere um novo token no BotFather com `/revoketoken` e atualize o cofre.

**Bot responde mas sem formatação:**

O Telegram pode rejeitar MarkdownV2 com caracteres especiais. O GarraIA escapa os caracteres automaticamente; se houver problemas, relate como bug no [GitHub Issues](https://github.com/michelbr84/GarraRUST/issues).
