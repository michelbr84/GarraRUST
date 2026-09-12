- Corrige `docker-compose.turboquant.yml`, que montava
  `docs/deployment/config.turboquant.yml` inexistente no repo — o
  `docker compose -f docker-compose.turboquant.yml up` falhava no boot.
  Config criado com provider `llamacpp` (keyless) apontando para o
  servico `llama-turboquant:8080` da rede do compose.
