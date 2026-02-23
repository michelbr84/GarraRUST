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
docker compose -f docker-compose.ollama.yml exec ollama ollama pull llama3.1
```

*(Observação: ajuste `llama3.1` se você alterou o modelo em `docs/deployment/config.ollama.yml`)*

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