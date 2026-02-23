# Segurança

A segurança é um princípio fundamental do GarraIA.

O sistema foi projetado desde o início para proteger credenciais, prevenir acessos não autorizados e isolar componentes potencialmente perigosos.

---

## Recursos de Segurança

### Vault de credenciais criptografado

O GarraIA armazena chaves de API e credenciais utilizando criptografia **AES-256-GCM**, um padrão moderno e seguro utilizado em aplicações de nível corporativo.

Características:

* Criptografia forte com AES-256-GCM
* Arquivo protegido em:

```
~/.garraia/credentials/vault.json
```

* Desbloqueado apenas com:

```
GARRAIA_VAULT_PASSPHRASE
```

Isso garante que mesmo com acesso ao disco, as credenciais não possam ser utilizadas sem a senha.

---

### Autenticação obrigatória via gateway

O gateway WebSocket do GarraIA exige autenticação através de um sistema de pareamento.

Características:

* Código de pareamento único
* Autorização por sessão
* Proteção contra conexões não autorizadas
* Controle total sobre quem pode acessar o agente

Isso previne que terceiros controlem o agente remotamente.

---

### Allowlist por canal

O GarraIA permite definir listas de permissões por canal de comunicação.

Você pode controlar exatamente quem pode interagir com o agente em:

* Telegram
* Discord
* Slack
* WhatsApp
* iMessage

Exemplo de proteção:

* Apenas usuários autorizados podem enviar comandos
* Bloqueio automático de usuários desconhecidos

---

### Proteção contra Prompt Injection

O GarraIA valida e sanitiza entradas antes de processá-las.

Proteções incluem:

* Validação de entrada
* Sanitização de dados
* Isolamento de ferramentas
* Prevenção de execução não autorizada

Isso reduz o risco de ataques como:

* Prompt injection
* Escalada de privilégio
* Execução maliciosa de comandos

---

### Sandbox de plugins WASM

Plugins executam em um ambiente isolado utilizando WebAssembly e Wasmtime.

Características:

* Isolamento completo do sistema host
* Sem acesso direto ao sistema operacional
* Sem acesso direto à memória do processo principal
* Interfaces controladas e seguras

Isso garante que plugins não possam comprometer o sistema.

---

## Arquitetura de Segurança

A arquitetura de segurança do GarraIA inclui múltiplas camadas:

* Criptografia de credenciais
* Controle de autenticação
* Isolamento de execução
* Validação de entrada
* Sandbox de plugins
* Controle de acesso por canal

Essa abordagem reduz significativamente a superfície de ataque.

---

## Superfícies de Ataque

As principais superfícies protegidas incluem:

* Gateway WebSocket
* Execução de ferramentas
* Plugins WASM
* Configuração do sistema
* Comunicação com provedores LLM
* Canais de comunicação externos

Cada uma dessas superfícies possui mecanismos de proteção específicos.

---

## Checklist de Segurança

Checklist recomendado para uso em produção:

* [ ] Definir `GARRAIA_VAULT_PASSPHRASE`
* [ ] Utilizar vault criptografado para todas as chaves
* [ ] Configurar allowlist nos canais
* [ ] Não expor o gateway diretamente à internet sem autenticação
* [ ] Utilizar firewall quando aplicável
* [ ] Manter o GarraIA atualizado
* [ ] Monitorar logs regularmente
* [ ] Utilizar apenas plugins confiáveis

---

## Documentação relacionada

Consulte também:

* Architecture.md — Arquitetura interna
* providers.md — Configuração de provedores
* plugins.md — Sistema de plugins
* mcp.md — Integração com servidores MCP