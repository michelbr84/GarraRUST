---
name: code-review
description: Revisar código em busca de bugs, falhas de segurança, problemas de performance e questões de estilo. Funciona com arquivos ou trechos inline.
triggers:
  - revisar
  - revisão de código
  - code review
  - auditoria
  - verificar este código
dependencies: []
---

# Revisão de Código

Quando o usuário pedir para revisar código, siga este processo estruturado.

---

## Obter o código

- **Caminho de arquivo**: Use `file_read` para ler o arquivo.
- **Diretório**: Use `bash` para executar `find` ou `ls`, depois leia os arquivos relevantes.
- **Trecho inline**: Trabalhe diretamente com o código enviado na mensagem.

---

## Checklist de revisão

Analise o código nesta ordem:

---

### 1. Segurança

- Segredos hardcoded (chaves de API, senhas, tokens)
- Vetores de SQL injection, XSS ou command injection
- Desserialização insegura ou uso de `eval`
- Falta de validação de entrada em fronteiras expostas ao usuário
- Acesso excessivamente permissivo a arquivos ou rede

---

### 2. Correção (Corretude)

- Erros de off-by-one e condições de limite
- Tratamento de null/undefined (falta de validações, `unwrap` fora de testes)
- Condições de corrida em código concorrente
- Falhas no tratamento de erros (erros ignorados, `except:` genérico, ausência de `.catch()`)
- Erros lógicos em condicionais

---

### 3. Performance

- Alocações desnecessárias em caminhos críticos
- Queries N+1 ou iterações sem limite
- Ausência de paginação em endpoints de listagem
- Chamadas bloqueantes em contextos assíncronos

---

### 4. Estilo e Manutenibilidade

- Código morto ou blocos comentados
- Funções com mais de 50 linhas
- Números mágicos sem constantes nomeadas
- Nomes ausentes, ambíguos ou enganosos
- Prints de debug deixados em código de produção

---

## Formato de saída

Para cada problema encontrado, reporte no formato:

```

[SEVERIDADE] arquivo:linha - descrição
Sugestão: como corrigir

```

---

### Níveis de severidade

- **CRÍTICO**: Vulnerabilidade de segurança ou risco de perda de dados. Corrigir imediatamente.
- **BUG**: Causará comportamento incorreto. Deve ser corrigido antes do merge.
- **AVISO**: Problema potencial ou code smell. Deve ser tratado.
- **NOTA**: Questão de estilo ou melhoria menor. Desejável corrigir.

---

## Regras

- Sempre leia o código antes de revisar. Nunca revise código que você não viu.
- Seja específico — referencie linhas e variáveis exatas, não dê conselhos vagos.
- Se o código estiver bom, diga isso. Não invente problemas.
- Limite-se aos 5–10 problemas mais importantes. Não sobrecarregue com nitpicks.