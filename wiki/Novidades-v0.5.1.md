# Novidades da v0.5.1

> 🇬🇧 [English version](Whats-New-v0.5.1) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.1)

Release estável de 2 de novembro de 2026 — substitui a v0.5.0 como release `latest`.

A versão é de uma frente: **o GarraIA usa o seu plano do ChatGPT**, sem API
key, pelo fluxo oficial da OpenAI para apps open-source. Junto vêm três
correções de segurança e a chave de assinatura definitiva do APK Android.

## O plano do ChatGPT, sem API key (#1796)

- **Continuar com ChatGPT.** Na página **Providers** da porta 3888, o cartão
  ChatGPT abre o login da OpenAI no navegador; pela linha de comando,
  `garra chatgpt login --padrao`. Você autoriza o uso do plano e o cartão
  passa para **Conectado**.
- **Só o caminho oficial.** OAuth com PKCE S256, registro dinâmico do app,
  escopo `chatgpt.tokens.use.direct` e ID token validado em RS256 contra as
  chaves publicadas pela OpenAI. A inferência vai ao endpoint público
  `POST /v1/responses` com `store: false`. O GarraIA não usa endpoints
  privados do ChatGPT, não lê suas conversas e não reaproveita credenciais de
  outras ferramentas.
- **Tokens protegidos.** Ficam em `credentials/openai-codex` com permissão
  `0600`, nunca em log; a renovação é serializada entre gateway, CLI e
  servidor MCP (o refresh token rotaciona) e **Sair** revoga a sessão.
- **GPT-6.1 Sol com raciocínio High** por padrão. O cartão oferece os níveis
  Minimal, Low, Medium, High, Extra High, Max e Ultra e as velocidades
  Thinking, Fast e Ultrafast; o que o modelo ou a conta não oferecem aparece
  desativado, com o motivo (Ultra existe só no Codex). **Testar conexão** faz
  uma inferência real e mostra o tier que a OpenAI serviu.
- **Limite do plano vira fallback.** Quando a OpenAI avisa que o limite de
  uso acabou, o GarraIA não repete o pedido pelo plano e passa direto para o
  primeiro provider de `agent.fallback_providers` (por exemplo o OpenRouter).
  Ao tornar o ChatGPT padrão, o provider padrão anterior já vira o primeiro
  fallback.

Guia: [Usar o plano do ChatGPT](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/connect-chatgpt.md).

## Segurança

- **Voz: um usuário deixa de ouvir a resposta do outro (#1754).** Hibiki e LM
  Studio passavam o áudio por um arquivo temporário com o mesmo nome para
  todas as chamadas; agora os bytes vêm direto da resposta HTTP. Whisper e
  Hibiki ganham timeout de 120 s.
- **Gate de comandos reconhece o executável do Windows (#1791).** `curl.exe`,
  `C:\...\git.exe`, `CURL.EXE` e caminhos entre aspas passam pela mesma
  checagem que `curl` e `git`; o mesmo vale para `sudo`, pipe para shell e
  `bash -c` com caminho absoluto. `kubectl exec` passa a pedir confirmação,
  como o `docker exec`.
- **Dependências (#1795).** `serialize-javascript` 7.1.4 no site de docs
  (GHSA-m5mm-p8h7-m29v) e `clipboard-win` 5.4.3 na CLI do Windows (a 5.4.1 foi
  retirada do registro).

## Garra Mobile (Android): chave de assinatura definitiva

O APK da v0.5.1 é o primeiro assinado com a **chave definitiva** do projeto;
o da v0.5.0 saiu com a chave de debug do runner do CI. O Android não instala
uma atualização assinada com outra chave, então **desinstale a v0.5.0 uma
vez** e instale a v0.5.1. Daqui em diante, as atualizações instalam por cima.

## Como instalar

Windows: `irm https://garraia.org/install.ps1 | iex` · Linux/macOS:
`curl -fsSL https://garraia.org/install.sh | sh` · Docker: `docker pull
ghcr.io/michelbr84/garraia:0.5.1` — e o `garra update` a partir da 0.5.0.
