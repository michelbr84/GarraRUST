# Desenvolver um Plugin (Rust para WASM)

Este guia explica como criar um plugin para o GarraIA compilando Rust para WebAssembly (WASM), executado em sandbox seguro via Wasmtime.

---

## O que são plugins GarraIA?

Plugins são binários WebAssembly que estendem o comportamento do agente adicionando novas **ferramentas** (tools) disponíveis durante a execução do LLM. Eles rodam em sandbox isolado — sem acesso direto ao sistema operacional, rede ou memória do processo principal.

### Capacidades de um plugin

- Expor novas ferramentas ao runtime do agente
- Receber e retornar dados via interface WASM segura
- Integrar APIs externas (via função `http_fetch` fornecida pelo host)
- Processar texto, JSON, ou dados binários

### Limitações por design (sandbox)

- Sem acesso direto ao sistema de arquivos (apenas via funções host autorizadas)
- Sem acesso direto à rede (apenas via `http_fetch` do host)
- Limite de memória: 64 MB por padrão (configurável)
- Timeout de execução: 30 segundos por chamada (configurável)

---

## Pré-requisitos

```bash
# Rust 1.85+
rustc --version

# Adicionar o target WebAssembly
rustup target add wasm32-wasip1

# Verificar
rustup target list --installed | grep wasm
```

---

## Passo 1 — Criar o projeto do plugin

```bash
cargo new --lib garraia-plugin-exemplo
cd garraia-plugin-exemplo
```

Configure o `Cargo.toml`:

```toml
[package]
name = "garraia-plugin-exemplo"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# SDK do GarraIA para plugins (geração de bindings WASM)
garraia-plugin-sdk = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

---

## Passo 2 — Implementar o plugin

Edite `src/lib.rs`:

```rust
use garraia_plugin_sdk::{plugin_main, Tool, ToolInput, ToolOutput};
use serde_json::{json, Value};

/// Declara as ferramentas que este plugin expõe.
plugin_main! {
    tools: [WordCountTool, ReverseTextTool],
}

/// Ferramenta que conta palavras em um texto.
struct WordCountTool;

impl Tool for WordCountTool {
    fn name(&self) -> &str {
        "word_count"
    }

    fn description(&self) -> &str {
        "Conta o número de palavras em um texto."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "O texto para contar palavras."
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: ToolInput) -> ToolOutput {
        let text = input
            .params
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let count = text.split_whitespace().count();

        ToolOutput::success(json!({
            "word_count": count,
            "text_preview": &text[..text.len().min(50)]
        }))
    }
}

/// Ferramenta que inverte o texto recebido.
struct ReverseTextTool;

impl Tool for ReverseTextTool {
    fn name(&self) -> &str {
        "reverse_text"
    }

    fn description(&self) -> &str {
        "Inverte os caracteres de um texto."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "O texto a ser invertido."
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: ToolInput) -> ToolOutput {
        let text = input
            .params
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let reversed: String = text.chars().rev().collect();

        ToolOutput::success(json!({ "result": reversed }))
    }
}
```

---

## Passo 3 — Compilar para WASM

```bash
cargo build --release --target wasm32-wasip1
```

O arquivo gerado estará em:

```
target/wasm32-wasip1/release/garraia_plugin_exemplo.wasm
```

---

## Passo 4 — Instalar o plugin

**Via API REST (recomendado):**

```bash
curl -X POST http://127.0.0.1:3888/api/plugins/install \
  -F "file=@target/wasm32-wasip1/release/garraia_plugin_exemplo.wasm" \
  -F "name=exemplo" \
  -F "description=Plugin de exemplo com word_count e reverse_text"
```

**Manualmente (copiando o arquivo):**

```bash
mkdir -p ~/.garraia/plugins/
cp target/wasm32-wasip1/release/garraia_plugin_exemplo.wasm \
   ~/.garraia/plugins/exemplo.wasm
```

Adicione ao `~/.garraia/config.yml`:

```yaml
plugins:
  - name: exemplo
    path: "~/.garraia/plugins/exemplo.wasm"
    enabled: true
```

---

## Passo 5 — Verificar a instalação

```bash
# Listar plugins instalados
curl http://127.0.0.1:3888/api/plugins | jq .

# Testar via chat
curl -X POST http://127.0.0.1:3888/api/chat \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Quantas palavras tem a frase: o rato roeu a roupa do rei?",
    "session_id": "teste-plugin"
  }' | jq .response
```

O agente deverá chamar automaticamente a ferramenta `word_count` para responder.

---

## Gerenciar plugins

```bash
# Listar todos os plugins
curl http://127.0.0.1:3888/api/plugins

# Remover um plugin
curl -X DELETE http://127.0.0.1:3888/api/plugins/exemplo
```

---

## Configuração de segurança do sandbox

Ajuste os limites no `config.yml`:

```yaml
plugins:
  sandbox:
    memory_limit_mb: 64        # Memória máxima por plugin (padrão: 64)
    execution_timeout_secs: 30 # Timeout por chamada (padrão: 30)
    allow_http: false          # Permitir chamadas HTTP via host (padrão: false)
    allow_env: false           # Permitir leitura de variáveis de ambiente (padrão: false)
```

---

## Interface host disponível para plugins

O runtime expõe as seguintes funções para plugins via WASM imports:

| Função | Descrição | Requer permissão |
|--------|-----------|-----------------|
| `log(msg)` | Escreve no log do GarraIA | Não |
| `http_fetch(url, method, body)` | Requisição HTTP | `allow_http: true` |
| `get_env(key)` | Lê variável de ambiente | `allow_env: true` |
| `kv_get(key)` / `kv_set(key, val)` | Armazenamento chave-valor por plugin | Não |

---

## Resolução de problemas

**Erro de compilação WASM:**

```bash
# Certifique-se de que o target está instalado
rustup target add wasm32-wasip1

# Verifique se todas as dependências são compatíveis com WASM
cargo check --target wasm32-wasip1
```

**Plugin não aparece na lista após instalação:**

- Verifique se o arquivo `.wasm` é um binário válido: `file meu_plugin.wasm`
- Confirme que o GarraIA foi reiniciado após a instalação manual
- Verifique os logs: `garraia logs --level debug`

**Ferramenta não é chamada pelo LLM:**

- O LLM escolhe ferramentas com base na descrição. Escreva descrições claras e específicas.
- Confirme que a ferramenta aparece na listagem de ferramentas disponíveis.
