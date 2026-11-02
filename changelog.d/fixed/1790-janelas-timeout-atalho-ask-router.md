- **Janelas de timeout que a #1778 deixou de fora (#1790).** Quatro bordas da
  janela de `timeouts.llm` fechadas:
  - **`garra chat --model <tag>`** (atalho do Ollama local): o provider que sai
    da sonda — instalado ou baixado com `-y` — recebe a janela da entrada
    `llm:` do modelo pedido (`provider: ollama` com aquele `model`, ou a
    entrada do Ollama), e nao mais so a global.
  - **`garra ask`**: quando a janela da config corta a chamada antes do
    `--timeout-secs`, o erro sai como `timeout` (exit 124), com a mensagem
    nomeando `timeouts.llm.default_secs` / `llm.<nome>.timeout_secs` — antes
    saia como `provider_error` (69), como se o provider estivesse quebrado.
    Vale tambem com a janela disparando no meio de uma resposta em streaming.
    O mesmo vale para o `garra_ask` do MCP. O aviso de "aumente a janela" deixa
    de aparecer em timeout de CONEXAO (10s fixos), que a janela nao resolve.
  - **Sonda do atalho e providers ativados em runtime** (`POST
    /api/providers`): a sonda sai de `cliente_llm` como todo cliente de
    provider (sem seguir redirect, regra 14) e nunca passa da janela; o
    provider ativado pelo Web Console, que ficava sem teto de resposta (sem
    `base_url`) ou com 30s fixos (com `base_url`, cliente pinado), passa a
    obedecer `llm.<tipo>.timeout_secs` / `timeouts.llm.default_secs`, ainda
    pinado e sem redirect.
  - **Cliente que nao monta deixa de degradar em silencio**: os builders de
    `garraia_agents::cliente_llm` devolvem o erro em vez de cair num cliente
    padrao (redirect ligado, sem janela). A CLI falha com a causa; o gateway
    loga e nao registra provider de LLM com policy errada.
