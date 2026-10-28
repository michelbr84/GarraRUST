- `fallback_providers` agora e acionado quando o provider primario recusa a
  credencial (401/403), em batch e em streaming, sem retry no primario. Erros do
  pedido (400/404/413/422) continuam sem fallback (#1735).
