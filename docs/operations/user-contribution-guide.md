# GarraIA — Guia de Contribuição do Usuário Final — PROPOSTA

> **Status: PROPOSTA — aguarda ratificação do maintainer.** Este guia fecha o
> item "Community" da issue de GA de operação (#1577): como reportar bug, onde
> pedir ajuda e como entrar como contributor — o lado do usuário do
> `CONTRIBUTING.md`, que continua sendo o guia de desenvolvimento.
> **Nada aqui é promessa contratual**: os tempos citados são alvos de
> comunidade, definidos na [política de suporte](support-policy.md).

## 1. Reportar um bug

Antes de abrir a issue, dois passos economizam um ciclo inteiro de ida e volta:

1. **Busque issues parecidas** (abertas e fechadas) —
   [nova busca](https://github.com/michelbr84/GarraRUST/issues?q=is%3Aissue).
2. **Rode o pré-flight** e cole a saída na issue:
   ```bash
   garra config check    # valida a configuração efetiva
   garra verify          # verificação de saúde do ambiente
   garraia doctor        # diagnóstico por componente (inclui bash/sandbox/canais)
   ```

Depois abra a issue com o **template certo** — cada template pede o contexto
que aquele subsistema precisa:

| Situação | Template |
|---|---|
| Comportamento inesperado num comando ou recurso | [Bug report](https://github.com/michelbr84/GarraRUST/issues/new?template=bug_report.yml) |
| Problema num provedor LLM (OpenRouter, Ollama…) | [Provider issue](https://github.com/michelbr84/GarraRUST/issues/new?template=provider_issue.yml) |
| Problema no servidor MCP ou numa tool MCP | [MCP issue](https://github.com/michelbr84/GarraRUST/issues/new?template=mcp_issue.yml) |
| Comando do Telegram não responde como esperado | [Telegram command](https://github.com/michelbr84/GarraRUST/issues/new?template=telegram_command.yml) |
| Ideia de recurso novo | [Feature request](https://github.com/michelbr84/GarraRUST/issues/new?template=feature_request.yml) |
| Qualquer outra coisa | [Geral](https://github.com/michelbr84/GarraRUST/issues/new?template=general.yml) |

**O que sempre ajuda**: versão (`garra --version`), sistema operacional, o que
você esperava vs. o que aconteceu, passos para reproduzir e os logs do pré-flight.

**Redija segredos antes de colar.** Logs podem conter tokens, chaves e senhas de
connection string — apague-os. Nunca cole API key em issue, PR ou discussion.

## 2. Pedir ajuda

| Tipo de dúvida | Canal |
|---|---|
| "Como faço X?", configuração, primeiro passo | [GitHub Discussions](https://github.com/michelbr84/GarraRUST/discussions) |
| Troca rápida com a comunidade | [Discord](https://discord.gg/aEXGq5cS) |
| Documentação | [Wiki](https://github.com/michelbr84/GarraRUST/wiki) · [garraia.org/docs](https://garraia.org/docs/introducao) · [`docs/`](../) |

Os tempos-alvo de resposta por canal estão na
[política de suporte](support-policy.md) — são alvos de comunidade, não SLA.

## 3. Pedir um recurso novo

Pelo template de [feature request](https://github.com/michelbr84/GarraRUST/issues/new?template=feature_request.yml).
Descreva o problema que o recurso resolve, não só a solução que você imaginou —
muitas vezes existe um caminho já disponível e alguém aponta na discussão. Para
features grandes, abrir a discussion **antes** do código evita retrabalho dos
dois lados.

## 4. Vulnerabilidades de segurança

**Nunca em issue pública.** Siga o [SECURITY.md](../../SECURITY.md): advisory
privado no GitHub ou security@garraia.org. Issues de segurança abertas em
público são fechadas e o histórico não é apagado — quem achar primeiro pode ser
alguém que não é você.

## 5. Entrar como contributor de código

O guia completo é o [CONTRIBUTING.md](../../CONTRIBUTING.md) (setup do ambiente,
regras e fluxo de PR). O caminho curto:

1. Comece por uma [good first issue](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen).
2. Branch a partir da `main` atualizada, commit com a mensagem no padrão
   `tipo(escopo): descrição (#issue)`.
3. **Gates antes do PR**: `cargo fmt -p <crate>`,
   `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo test -p <crate>`
   e um fragmento de changelog em `changelog.d/<categoria>/` (o CI barra PR sem
   fragmento).
4. PR com corpo em pt-BR: resumo, o que foi validado e follow-ups honestos.

## 6. Propostas operacionais em ratificação

Estes documentos estão em proposta — feedback de quem usa é bem-vindo nas
discussions, e a ratificação é do maintainer:

| Documento | Status |
|---|---|
| [Política de suporte](support-policy.md) | Proposta |
| [Versões suportadas](version-support-policy.md) | Proposta |
| [Depreciação](deprecation-policy.md) | Proposta |
| [SLA — decisão registrada](sla-decision-brief.md) | Decisão registrada: self-host distribuído **sem SLA** (com justificativa e opções) |
