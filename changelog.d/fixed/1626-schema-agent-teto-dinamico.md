- **Schema de `garra_agent` anuncia o teto do operador (#1626).** `timeout_secs`
  passa a sair com `maximum` e `default` derivados de `agent_max_seconds`, em vez
  de 600/300 fixos; antes o schema dizia que um valor valia e a recusa vinha depois.
