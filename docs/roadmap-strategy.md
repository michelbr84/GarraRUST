# GarraRUST - Plano Estratégico de Execução

## 1. Executive Summary

### Estado Atual do Backlog

| Estado | Quantidade |
|--------|------------|
| **Todo** | 71 issues |
| **Backlog** | 26 issues |
| **Done** | ~76 issues |
| **Canceled** | 26 issues |

### Principais Problemas Identificados

1. **Acúmulo de work-in-progress**: 71 issues no "Todo" - maioria relacionada a duas iniciativas grandes (Modos de Execução + Glob & Ignore Engine)

2. **Iniciativas sobrepostas**: 
   - Modos de Execução (GAR-219~GAR-240) - foca em runtime/agent
   - Glob & Ignore Engine (GAR-241~GAR-270) - foca em filesystem/traversal

3. **Dependencies não explícitas**: Muitas issues dependem de outras mas não há linkage no Linear

4. **Items Cancelados**: 26 issues ADM-1 a ADM-25 foram canceladas - sugere mudança de foco

5. **Débito Técnico Identificado**:
   - GAR-159: Unify garraia-tools::Tool trait
   - GAR-207: Session control
   - GAR-208: Context policy
   - GAR-209: Content sanitization

### Estratégia Geral Recomendada

1. **Focar em estabilidade primeiro** - resolver issues P0/P1 do modo "Telegram-first"
2. **Dividir iniciativas grandes** - quebrar em chunks menores executáveis
3. **Definir dependências explícitas** - criar blockings no Linear
4. **Parallelizar onde possível** - issues independentes podem ser feitas em paralelo
5. **Limpar cancelados** - архив issues canceladas para reduzir ruído

---

## 2. Priority Order

### Onde Focar Primeiro (P0/P1 no Todo)

| # | Issue | Título | Categoria | Prioridade | Dependências |
|---|-------|--------|-----------|------------|--------------|
| 1 | GAR-219 | M0-1: Definir contrato de Modo | Nova Funcionalidade | **Critical** | - |
| 2 | GAR-221 | M1-1: AgentMode enum + ModeProfile | Nova Funcionalidade | **Critical** | GAR-219 |
| 3 | GAR-224 | M2-1: ToolPolicyEngine | Nova Funcionalidade | **Critical** | GAR-221 |
| 4 | GAR-226 | M3-1: Heurísticas determinísticas (Auto Mode) | Nova Funcionalidade | **Critical** | GAR-221 |
| 5 | GAR-228 | M4-1: Tool repo_search | Nova Funcionalidade | **Critical** | - |
| 6 | GAR-229 | M4-2: Tool list_dir | Nova Funcionalidade | **Critical** | - |
| 7 | GAR-223 | M1-3: Comandos /mode e /modes | Nova Funcionalidade | **Critical** | GAR-221 |
| 8 | GAR-222 | M1-2: Persistência por sessão | Infraestrutura | **Critical** | - |
| 9 | GAR-225 | M2-2: tool_choice support | Nova Funcionalidade | **Critical** | GAR-224 |
| 10 | GAR-238 | M9-1: Logs padronizados | Observabilidade | **Critical** | - |
| 11 | GAR-239 | M9-2: Testes modos e políticas | QA | **Critical** | GAR-224 |
| 12 | GAR-241 | 1.1.1: Glob Semantics Doc | Nova Funcionalidade | **High** | - |
| 13 | GAR-244 | 2.1.1: garraia-glob API | Infraestrutura | **High** | - |
| 14 | GAR-245 | 2.1.2: Path normalization | Infraestrutura | **High** | GAR-244 |
| 15 | GAR-246 | 2.1.3: Performance guardrails | Performance | **High** | GAR-244 |

### Dependências Críticas Identificadas

```
GAR-219 (Modo Contract)
  └─> GAR-221 (AgentMode enum)
       ├─> GAR-222 (Persistência)
       ├─> GAR-223 (/mode commands)
       ├─> GAR-224 (ToolPolicyEngine)
       │    ├─> GAR-225 (tool_choice)
       │    └─> GAR-239 (testes)
       └─> GAR-226 (Auto Router)
            └─> GAR-227 (LLM router)

GAR-241 (Glob Spec)
  └─> GAR-244 (garraia-glob API)
       ├─> GAR-245 (path norm)
       └─> GAR-246 (perf guards)
            └─> GAR-253 (.garraignore)
                 └─> GAR-257 (Scanner)
```

---

## 3. Recommended Execution Waves

### Wave 1 — Stabilization & Core (Semanas 1-2)
**Objetivo**: Estabelecer base para as outras iniciativas

| Issue | Título | Justificativa |
|-------|--------|---------------|
| GAR-219 | Definir contrato de Modo | Base para todo o sistema de modos |
| GAR-221 | AgentMode enum + ModeProfile | Core type definitions |
| GAR-241 | Documento Glob Semantics | Spec evita retrabalho |
| GAR-244 | garraia-glob API | Base para filesystem operations |

**Riscos**: Baixo - tarefas de definição
**Ganhos**: Base sólida para tudo mais

---

### Wave 2 — Mode System Core (Semanas 3-4)
**Objetivo**: Implementar o motor de modos

| Issue | Título | Dependências |
|-------|--------|--------------|
| GAR-222 | Persistência por sessão | GAR-221 |
| GAR-223 | Comandos /mode | GAR-221 |
| GAR-224 | ToolPolicyEngine | GAR-221 |
| GAR-226 | Heurísticas Auto Mode | GAR-221 |
| GAR-228 | Tool repo_search | - |
| GAR-229 | Tool list_dir | - |

**Riscos**: Médio - integração com runtime existente
**Ganhos**: Sistema de modos funcional

---

### Wave 3 — OpenAI Compatibility & Testing (Semanas 5-6)
**Objetivo**: Garantir compatibilidade e qualidade

| Issue | Título | Dependências |
|-------|--------|--------------|
| GAR-220 | OpenAI streaming + tool calling | - |
| GAR-225 | tool_choice support | GAR-224 |
| GAR-227 | LLM Router opcional | GAR-226 |
| GAR-238 | Logs padronizados | - |
| GAR-239 | Testes modos e políticas | GAR-224 |

**Riscos**: Médio - API compatibility
**Ganhos**: API stable, testável

---

### Wave 4 — Glob & Ignore Foundation (Semanas 7-8)
**Objetivo**: Sistema de filesystem robusto

| Issue | Título | Dependências |
|-------|--------|--------------|
| GAR-245 | Path normalization | GAR-244 |
| GAR-246 | Performance guardrails | GAR-244 |
| GAR-247 | * vs ** matching | - |
| GAR-248 | Extglob sem backtracking | - |
| GAR-249 | Testes Picomatch | GAR-247 |

**Riscos**: Médio - performance sensitive
**Ganhos**: Filesystem predictable

---

### Wave 5 — UI & Integration (Semanas 9-12)
**Objetivo**: Finalizar integrações e UI

| Issue | Título | Dependências |
|-------|--------|--------------|
| GAR-230 | API HTTP para modos | GAR-223 |
| GAR-231 | UI Mode Sidebar | GAR-230 |
| GAR-232 | UI Edit Mode | GAR-231 |
| GAR-233 | Continue config templates | - |
| GAR-253 | .garraignore | GAR-246 |
| GAR-257 | Scanner unificado | GAR-253 |
| GAR-259 | Filtro watcher | GAR-257 |
| GAR-261 | CLI config | GAR-244 |

**Riscos**: Baixo - UI only
**Ganhos**: UX completa

---

### Wave 6 — Advanced Features & Polish (Semanas 13-16)
**Objetivo**: Features avançadas

| Issue | Título | Dependências |
|-------|--------|--------------|
| GAR-234 | Continue headers | GAR-233 |
| GAR-235 | Orchestrator multi-step | GAR-224 |
| GAR-236 | Bash security | - |
| GAR-237 | git_diff tool | - |
| GAR-250 | Bash extglob | GAR-249 |
| GAR-265 | Test vectors | GAR-249 |
| GAR-268 | Anti-pattern bombs | GAR-246 |

**Riscos**: Baixo - features opcionais
**Ganhos**: Feature complete

---

## 4. Risk Analysis

### Áreas Sensíveis do Sistema

| Área | Riscos | Mitigação |
|------|--------|-----------|
| **Runtime/Agent** | Quebrar execução atual de tools | Testes unitários primeiro |
| **File I/O** | Path traversal vulnerabilities | GAR-246 (guardrails) primeiro |
| **Telegram** | Breaking existing functionality | Manter ask como default |
| **OpenAI API** | Breaking compatibility | Testar streaming SSE |

### Issues que Exigem Testes Antes

1. **GAR-220** - OpenAI compatibility - precisa E2E tests
2. **GAR-225** - tool_choice - pode mudar behavior
3. **GAR-257** - Scanner - pode mudar indexing behavior
4. **GAR-235** - Orchestrator - execução multi-step complexa

### Possíveis Gargalos

1. **GAR-222** - Persistência: envolve mudanças no DB schema
2. **GAR-246** - Performance: precisa benchmarking
3. **GAR-235** - Orchestrator: design complexo

---

## 5. Suggested Issue Restructuring

### Issues para Consolidar

| Original | Proposta |
|----------|----------|
| GAR-228 + GAR-229 | Uma issue "File system tools (repo_search + list_dir)" |
| GAR-261 + GAR-262 + GAR-263 | Uma issue "CLI integration (config + flags + test)" |
| GAR-247 + GAR-248 + GAR-249 | Uma issue "Picomatch implementation + tests" |

### Issues para Dividir

| Original | Novas Issues |
|----------|--------------|
| GAR-235 | GAR-235a: Executor base; GAR-235b: Validation loop; GAR-235c: Summary generation |
| GAR-257 | GAR-257a: WalkBuilder integration; GAR-257b: Stats output; GAR-257c: Coverage logs |

### Dependências para Criar (no Linear)

```
Bloqueantes:
- GAR-221 bloqueia GAR-222, GAR-223, GAR-224, GAR-226
- GAR-224 bloqueia GAR-225, GAR-239
- GAR-244 bloqueia GAR-245, GAR-246
- GAR-246 bloqueia GAR-253
```

---

## 6. Recommended First Actions

### Esta Semana

1. **Definir contrato de modo** (GAR-219)
   - Criar spec document
   - Definir enum AgentMode
   
2. **Criar spec de globbing** (GAR-241)
   - Documentar * vs ** behavior
   - Definir default picomatch

3. **Mapear dependências no Linear**
   - Criar relationships entre issues
   - Identificar blockers

### Próximas Semanas

4. **Implementar AgentMode enum** (GAR-221)
5. **Criar garraia-glob crate skeleton** (GAR-244)
6. **Adicionar logging estruturado** (GAR-238)

---

## Métricas Sugeridas

- **Velocity**: Issues concluídas por sprint
- **Cycle Time**: Tempo from Todo → Done
- **Blocked Ratio**: % de issues bloqueadas
- **Test Coverage**: % de código coberto por testes

---

## Conclusão

O backlog tem 71 issues ativas mas com boa estrutura em 2 iniciativas principais:
1. **Modos de Execução** (GAR-219~240) - focado em runtime/agent behavior  
2. **Glob & Ignore Engine** (GAR-241~270) - focado em filesystem/traversal

A recomendação é **dividir em waves menores**, começar com definição/specs (Wave 1), depois implementar core (Wave 2-3), e finalizar UI/integrações (Wave 5-6).

**Foco inicial**: GAR-219 + GAR-221 + GAR-241 + GAR-244 = base para tudo mais.
