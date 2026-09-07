- **Acento derrubava o turno no roteador por LLM.** O corte da mensagem para a
  chamada de classificacao era `&text[..text.len().min(400)]`, e `len()` conta
  **bytes**: numa mensagem em portugues com mais de 400 bytes, o corte cai no
  meio de um `c`-cedilha ou de um `a`-til e o slice entra em panico. Passa a
  cortar por caractere. Caminho alcancavel sempre que
  `agent.auto_router_llm_enabled` estiver ligado e a heuristica ficar em duvida.
