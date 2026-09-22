# Implantação com Docker

Este diretório contém exemplos para implantar o GarraIA usando Docker Compose.

## Pré-requisitos

* [Docker](https://docs.docker.com/get-docker/) instalado.
* [Docker Compose](https://docs.docker.com/compose/install/) (incluído no Docker Desktop ou como plugin).

## Configuração

1. Copie `.env.example` para `.env` e preencha suas chaves de API:

```bash
cp .env.example .env
# Edite o arquivo .env com o editor de sua preferência
```

> **Credencial obrigatoria (#1261, v0.4.5).** O container liga em `0.0.0.0`
> e o `garraia start` **recusa subir** num bind nao-loopback sem credencial
> de gateway (exit 78, com a mensagem de como corrigir). Coloque no `.env`
> uma linha `GARRAIA_GATEWAY_API_KEY=` seguida de um segredo longo — gere com
> `openssl rand -hex 32` — e envie-o nos clientes como
> `Authorization: Bearer <chave>`. `/ping`, `/health` e `/api/health` seguem
> abertos para healthcheck. O `.env.example` ja traz a linha, vazia de
> proposito (placeholder seria chave publica), e os tres `docker-compose*.yml` repassam a env. Sem ela o
> `docker compose up -d` termina "com sucesso", mas o container entra em loop
> de reinicio (`restart: unless-stopped`, exit 78 a cada tentativa):
> `docker compose ps` mostra `Restarting` e `docker compose logs garraia`
> mostra o motivo. Helm e Terraform/ECS: veja `deploy/helm/garraia/values.yaml`
> (`gatewayApiKey`) e `deploy/terraform/README.md`
> (`gateway_api_key_secret_arn`). Os `docs/deployment/config.*.yml` nao carregam
> mais `gateway.host`/`gateway.port` (chaves deprecadas, nunca lidas).

---

## Exemplo 1: Gateway Local (Provedores em Nuvem)

Esta configuração executa o GarraIA conectado a provedores de LLM em nuvem (Anthropic, OpenAI, etc.). Ela utiliza `docs/deployment/config.basic.yml`.

```bash
docker compose -f docker-compose.yml up --build -d
```

O GarraIA estará disponível em:

```text
http://localhost:3888
```

---

## Exemplo 2: Gateway + Ollama Local

Esta configuração executa o GarraIA junto com uma instância local do Ollama na mesma rede Docker. Ela utiliza `docs/deployment/config.ollama.yml`.

### 1. Inicie os serviços:

```bash
docker compose -f docker-compose.ollama.yml up --build -d
```

### 2. **Importante:** Você deve baixar o modelo LLM dentro do container Ollama antes que o GarraIA possa utilizá-lo:

```bash
docker compose -f docker-compose.ollama.yml exec ollama ollama pull qwen3.8:latest
```

*(Observação: ajuste `qwen3.8:latest` se você alterou o modelo em `docs/deployment/config.ollama.yml`)*

### 3. O GarraIA estará disponível em:

```text
http://localhost:3888
```

E se comunicará internamente com o Ollama em:

```text
http://ollama:11434
```

---

## Configuração

Os exemplos utilizam arquivos de configuração localizados em:

```text
docs/deployment/
```

Arquivos disponíveis:

* `config.basic.yml`: Configuração padrão para provedores em nuvem
* `config.ollama.yml`: Configuração apontando para o serviço interno do Ollama

Esses arquivos são montados dentro do container em:

```text
/home/garraia/.config/garraia/config.yml
```

Para personalizar a configuração, você pode:

* editar esses arquivos diretamente, ou
* criar seu próprio arquivo de configuração e atualizar o mapeamento de volume no `docker-compose.yml`

### Perfil de execução (`isolated-pod`)

Por padrão o gateway sobe no perfil `standard` — a postura para máquina
compartilhada: piso `search` no WhatsApp pessoal, jail das file tools, MCP
`filesystem` no workspace. Se o container é **descartável** e existe
justamente para dar autonomia plena ao agente, declare isso
([ADR 0024](adr/0024-perfis-de-execucao-isolated-pod.md); guia completo em
[`execution-profiles.md`](execution-profiles.md)):

```yaml
# docker-compose.yml
services:
  garraia:
    environment:
      GARRAIA_EXECUTION_PROFILE: isolated-pod   # vence o config.yml
```

ou, no `config.yml` montado no container:

```yaml
execution:
  profile: isolated-pod
  pod_root: /workspace        # opcional; raiz do MCP filesystem, absoluta
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    owners: ["5511999998888"] # só dono declarado recebe o piso `code`, e só em 1:1
```

> **Atenção.** O perfil é uma declaração do operador, não um mecanismo de
> isolamento: o gateway avisa uma vez no boot (`WARN`) e mantém um `Warning`
> permanente em `GET /api/diagnostics`, mas **não** isola volume do host
> montado, socket do Docker (`/var/run/docker.sock`), `--privileged`,
> `--pid=host`, `network_mode: host`, mounts não declarados nem segredos do
> host no `environment`. Se qualquer um desses vale para o seu serviço, ele
> não é um pod isolado — fique em `standard`. Nunca é inferido de
> `/.dockerenv` ou cgroups; valor inválido recusa o boot.

---

## Solução de Problemas

### "Connection refused" ao Ollama

Certifique-se de que o container do Ollama está em execução e saudável.

Se você estiver executando o Ollama na sua máquina host (fora do Docker), você não pode usar `localhost` no `config.yml`.

Use:

* `host.docker.internal` (Mac / Windows)

ou

* `172.17.0.1` (Linux)

Também garanta que o Ollama está escutando em:

```text
0.0.0.0
```

Você pode configurar isso com:

```bash
OLLAMA_HOST=0.0.0.0
```

O arquivo `docker-compose.ollama.yml` fornecido já configura essa rede automaticamente ao executar o Ollama em um container.

---

### Erros de chave de API

Verifique se o arquivo `.env` está preenchido corretamente e se os nomes das variáveis correspondem ao esperado no `config.yml` ou no padrão de resolução de variáveis de ambiente.

Exemplo:

```text
ANTHROPIC_API_KEY
```

---

### Permissões

Se você encontrar erros de permissão com volumes, verifique se o ID do usuário dentro do container (padrão: `1000`) possui acesso aos diretórios montados.