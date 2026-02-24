# MCP (Model Context Protocol)

O MCP permite conectar servidores de ferramentas externas ao GarraIA. Qualquer servidor compatível com MCP — acesso ao sistema de arquivos, GitHub, bancos de dados, busca na web, entre outros — torna-se disponível como ferramentas nativas do agente.

---

## Como funciona

1. Você configura servidores MCP no arquivo `config.yml` ou em `~/.garraia/mcp.json`
2. Na inicialização, o GarraIA conecta-se a cada servidor habilitado e descobre suas ferramentas
3. As ferramentas MCP aparecem junto com as ferramentas nativas usando nomes com namespace: `servidor.nome_da_ferramenta`
4. O agente pode chamá-las normalmente durante uma conversa

---

## Tipos de transporte

* **stdio** (padrão)
  O GarraIA inicia o processo do servidor e comunica-se via stdin/stdout.

* **HTTP**
  Permite conectar a um servidor MCP remoto via HTTP.

---

## Configuração

Servidores MCP podem ser configurados em dois locais. Ambos são combinados automaticamente na inicialização.

---

### config.yml

Adicione uma seção `mcp:` em:

```text
~/.garraia/config.yml
```

Exemplo:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    enabled: true

  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_..."
```

---

### mcp.json (compatível com Claude Desktop)

Você também pode usar o arquivo:

```text
~/.garraia/mcp.json
```

Formato:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      }
    }
  }
}
```

Se o mesmo servidor estiver definido em ambos os arquivos, a configuração do `config.yml` terá prioridade.

---

## Campos de configuração (McpServerConfig)

| Campo       | Tipo     | Padrão      | Descrição                                            |
| ----------- | -------- | ----------- | ---------------------------------------------------- |
| `command`   | string   | obrigatório | Executável a ser iniciado                            |
| `args`      | string[] | `[]`        | Argumentos de linha de comando                       |
| `env`       | mapa     | `{}`        | Variáveis de ambiente do processo                    |
| `transport` | string   | `"stdio"`   | Tipo de transporte (`stdio` ou `http`)               |
| `url`       | string   | nenhum      | URL para transporte HTTP                             |
| `enabled`   | boolean  | `true`      | Define se o servidor será conectado na inicialização |
| `timeout`   | inteiro  | `30`        | Tempo limite de conexão em segundos                  |

---

## Comandos CLI do MCP

### Listar servidores configurados

```bash
garraia mcp list
```

Mostra todos os servidores MCP configurados, incluindo:

* Status (habilitado/desabilitado)
* Comando
* Argumentos
* Timeout

---

### Inspecionar ferramentas

```bash
garraia mcp inspect <nome>
```

Conecta-se ao servidor especificado, descobre todas as ferramentas disponíveis e exibe no formato:

```text
servidor.nome_da_ferramenta
```

Depois desconecta automaticamente.

---

### Listar recursos

```bash
garraia mcp resources <nome>
```

Mostra todos os recursos disponíveis no servidor:

* URI
* Tipo MIME
* Nome
* Descrição

---

### Listar prompts

```bash
garraia mcp prompts <nome>
```

Mostra todos os prompts disponíveis:

* Nome
* Descrição
* Argumentos
* Campos obrigatórios

---

## Namespace das ferramentas

Ferramentas MCP usam namespace baseado no nome do servidor para evitar conflitos.

Exemplo:

Servidor:

```yaml
filesystem
```

Ferramenta:

```text
read_file
```

Nome final no GarraIA:

```text
filesystem.read_file
```

Isso permite que múltiplos servidores tenham ferramentas com o mesmo nome sem conflito.

---

## Exemplos

### Servidor de sistema de arquivos

Permite ler e escrever arquivos em um diretório específico:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/documentos"]
```

---

### Servidor GitHub

Permite interação com repositórios GitHub:

```yaml
mcp:
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_..."
```

---

### Servidor SQLite

Permite consultar um banco de dados local:

```yaml
mcp:
  sqlite:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-sqlite", "/caminho/para/banco.db"]
```

---

### Servidor HTTP

Permite conectar a um servidor MCP remoto:

```yaml
mcp:
  ferramentas-remotas:
    transport: http
    url: "https://mcp.exemplo.com"
    timeout: 60
```

---

## Detalhes de implementação

* O suporte ao MCP é controlado pela feature `mcp` no crate `garraia-agents` (habilitado por padrão)
* Utiliza o crate oficial Rust MCP SDK: `rmcp`
* O gerenciador está em:

```text
crates/garraia-agents/src/mcp/manager.rs
```

* A ponte entre MCP e ferramentas internas está em:

```text
crates/garraia-agents/src/mcp/tool_bridge.rs
```

Classe responsável:

```text
McpTool
```

---

## Estado atual e melhorias

A partir desta versão, o MCP oferece suporte completo ao transporte HTTP nativo (o recurso `mcp-http` está habilitado por padrão). As operações de recursos e prompts foram integradas ao MCP, permitindo que ferramentas, recursos e prompts sejam expostos como ferramentas nativas pelo agente. Além disso, o gerenciador tenta reconectar automaticamente aos servidores MCP após falhas, garantindo maior resiliência.

Atualmente não há issues abertas relacionadas a essas funcionalidades. Novas melhorias serão avaliadas conforme o feedback da comunidade.
