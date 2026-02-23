# Ferramentas (Tools)

O runtime de agentes do GarraIA fornece ao LLM acesso a ferramentas integradas e ferramentas MCP registradas dinamicamente. Quando o LLM decide usar uma ferramenta, o runtime a executa e envia o resultado de volta para a conversa.

---

# Loop de Ferramentas (Tool Loop)

O agente processa cada mensagem através de um loop de ferramentas que executa por até **10 iterações**. Em cada iteração:

1. O LLM gera uma resposta, opcionalmente incluindo chamadas de ferramentas
2. Se houver chamadas de ferramentas, o runtime as executa
3. Os resultados das ferramentas são adicionados à conversa e enviados de volta ao LLM
4. O loop continua até que o LLM responda sem chamadas de ferramentas ou o limite de iterações seja atingido

---

# Ferramentas Integradas

## bash

Executa comandos do sistema.

Utiliza:

* `bash -c` em sistemas Unix
* `powershell -Command` no Windows

|Propriedade|Valor|
|-|-|
|Timeout|30 segundos|
|Saída máxima|32 KB (truncada se excedida)|

**Entrada:**

```json
{ "command": "ls -la /tmp" }
````

Tanto stdout quanto stderr são capturados.

stderr aparece prefixado com:

```
STDERR:
```

Códigos de saída diferentes de zero são reportados como erro.

---

## file\_read

Lê o conteúdo de um arquivo.

|Propriedade|Valor|
|-|-|
|Tamanho máximo do arquivo|1 MB|
|Proteção contra path traversal|Ativado (`..` bloqueado)|

**Entrada:**

```json
{ "path": "/home/user/notas.txt" }
```

Arquivos maiores que o limite retornam erro em vez de truncamento.

---

## file\_write

Escreve conteúdo em um arquivo.

* Cria o arquivo se não existir
* Sobrescreve se já existir
* Cria diretórios automaticamente se necessário

|Propriedade|Valor|
|-|-|
|Tamanho máximo do conteúdo|1 MB|
|Proteção contra path traversal|Ativado (`..` bloqueado)|

**Entrada:**

```json
{ "path": "/home/user/saida.txt", "content": "Olá, mundo!" }
```

---

## web\_fetch

Busca o conteúdo de uma página web.

Retorna o conteúdo bruto:

* HTML
* JSON
* Texto simples

|Propriedade|Valor|
|-|-|
|Timeout|30 segundos|
|Tamanho máximo da resposta|1 MB (truncado se excedido)|

**Entrada:**

```json
{ "url": "https://example.com" }
```

Códigos HTTP diferentes de 2xx retornam erro.

É possível configurar uma lista de domínios bloqueados:

```
blocked\\\_domains
```

para restringir acesso.

---

## web\_search

Pesquisa na internet usando a API Brave Search.

Disponível apenas quando `BRAVE\\\_API\\\_KEY` está configurada.

|Propriedade|Valor|
|-|-|
|Timeout|15 segundos|
|Resultados padrão|5|
|Resultados máximos|10|
|Requer|BRAVE\_API\_KEY|

**Entrada:**

```json
{ "query": "comparação runtimes async rust", "count": 5 }
```

Parâmetro `count` é opcional:

* padrão: 5
* mínimo: 1
* máximo: 10

Resultados são retornados em formato Markdown contendo:

* título
* descrição
* URL

A ferramenta é registrada apenas se a chave Brave estiver configurada.

---

## schedule\_heartbeat

Agenda uma execução futura do agente.

Útil para:

* lembretes
* verificações futuras
* tarefas assíncronas
* monitoramento

|Propriedade|Valor|
|-|-|
|Delay máximo|30 dias (2.592.000 segundos)|
|Máximo por sessão|5 heartbeats pendentes|

**Entrada:**

```json
{ "delay\\\_seconds": 3600, "reason": "Verificar se o deploy terminou" }
```

Regras:

* delay deve ser positivo
* não pode agendar heartbeat dentro de outro heartbeat
* previne loops recursivos

Tarefas são armazenadas em SQLite.

O scheduler verifica periodicamente tarefas pendentes.

---

# Ferramentas MCP

Além das ferramentas integradas, o agente pode usar ferramentas de servidores MCP conectados.

Veja:

```
./mcp.md
```

Ferramentas MCP são:

* descobertas automaticamente na inicialização
* registradas com namespace

Formato:

```
server.tool\\\_name
```

Exemplo:

Servidor:

```
fs
```

Ferramenta:

```
read\\\_file
```

Nome completo:

```
fs.read\\\_file
```

---

# Interface das ferramentas MCP

Do ponto de vista do LLM, ferramentas MCP funcionam exatamente como ferramentas integradas:

Entrada:

```
JSON
```

Saída:

```
Texto
```

---

# Referência MCP

Veja o arquivo:

```
mcp.md
```

para detalhes completos de configuração.

