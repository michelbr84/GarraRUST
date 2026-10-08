- **Timeout do LLM ganha janela por provider e erro que nomeia a chave certa (#1593).** Um host lento (build Rust de 20-40 min no Termux, modelo grande sem GPU) batia nos 120s de `timeouts.llm.default_secs` e a resposta inteira era descartada — sem como aumentar a janela só para o provider lento, e com um erro que não dizia o que fazer.

Agora `llm.<nome>.timeout_secs` sobrescreve a janela global para aquele provider (ausente = global; `config check` avisa em 0, como no valor global), e o card de erro de timeout nomeia as duas chaves que o operador pode aumentar. A mensagem mantém o prefixo da classe e o texto do reqwest palavra por palavra (#1249).

Correção de semântica junto: `timeouts.llm.default_secs: 0` (e o novo `timeout_secs: 0`) agora significa de fato "sem timeout" — antes virava `Duration::ZERO`, um sleep já vencido que fazia **toda** chamada LLM falhar em milissegundos, o oposto do que a mensagem do `config check` prometia.

Refs #1593
