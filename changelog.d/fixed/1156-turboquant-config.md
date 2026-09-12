docker-compose.turboquant.yml montava docs/deployment/config.turboquant.yml,
que nao existia no repo — o `docker compose -f docker-compose.turboquant.yml up`
falhava no boot. Config criado com provider `llamacpp` (keyless) apontando para
o servico `llama-turboquant:8080` da rede do compose.
