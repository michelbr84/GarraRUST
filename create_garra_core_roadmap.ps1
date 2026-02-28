# PowerShell script para criar roadmap completo no Linear - Garra Core Chat Sync
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\create_garra_core_roadmap.ps1

param(
    [string]$ApiKey = $env:LINEAR_API_KEY
)

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    Write-Host "Obtenha em: https://linear.app/settings/api" -ForegroundColor Yellow
    Write-Host "Exemplo: `$env:LINEAR_API_KEY='lin_api_xxx'; .\create_garra_core_roadmap.ps1" -ForegroundColor Cyan
    exit 1
}

Write-Host "Criando roadmap Garra Core Chat Sync no Linear..." -ForegroundColor Green

$headers = @{
    "Authorization" = $ApiKey
    "Content-Type" = "application/json"
}

# Buscar ID do projeto/equipe GAR
Write-Host "Buscando equipe GAR..." -ForegroundColor Cyan
$query = @{ query = "query { teams { nodes { id name key } } }" } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $query -ErrorAction Stop
$team = $response.data.teams.nodes | Where-Object { $_.key -eq "GAR" }

if (-not $team) {
    Write-Host "Equipe GAR nao encontrada!" -ForegroundColor Red
    exit 1
}

Write-Host "Equipe GAR encontrada: $($team.id)" -ForegroundColor Green

# Buscar estados disponiveis
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$states = $stateResponse.data.team.states.nodes

$backlogState = $states | Where-Object { $_.name -eq "Backlog" } | Select-Object -First 1
if (-not $backlogState) {
    $backlogState = $states | Select-Object -First 1
}

Write-Host "Usando estado: $($backlogState.name) ($($backlogState.id))" -ForegroundColor Green

# Roadmap completo - Garra Core Chat Sync
$roadmap = @(
    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 0 — Definition & Guardrails
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-300"
        title = "[P1] Definir escopo Garra Core Chat Sync"
        desc = "## Objetivo
Documentar a visao: single source of truth para conversas multi-cliente.

## Descricao
* Documentar a visao: single source of truth para conversas multi-cliente.
* Definir o que entra no upgrade e o que fica fora.

## Acceptance Criteria
* Documento `docs/src/chat-sync.md` com:
  * objetivos
  * nao-objetivos
  * fluxos (VS Code, Telegram)
  * decisoes (DB, sessoes, compatibilidade OpenAI)

**Labels:** area:core, type:epic"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 1 — Persistencia e Modelo de Dados (Supabase/Postgres)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-310"
        title = "[P0] Criar schema de banco para sessoes e mensagens"
        desc = "## Descricao
Criar tabelas:
* `chat_sessions`
* `chat_messages`
* `chat_participants` (opcional)
* `chat_summaries` (opcional)
* `chat_session_keys` (opcional p/ mapping Telegram chat_id)

## Acceptance Criteria
* Migracoes SQL (compativeis com Supabase)
* Indices por `(session_id, created_at)`
* Campos minimos:
  * `session_id`, `role`, `content`, `source`, `created_at`, `provider`, `model`, `tokens_in/out` (se disponivel)

**Labels:** area:db, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-311"
        title = "[P0] Implementar camada DB no Garra (repo/db crate ou module)"
        desc = "## Descricao
Introduzir `Db` trait + implementacao Postgres (sqlx ou equivalente)

## Funcoes
* `create_session()`
* `append_message()`
* `list_messages(session_id, limit)`
* `get_session_by_external_key(source, external_id)` (ex: telegram chat_id)

## Acceptance Criteria
* Testes unitarios/integration (minimo: insert/list)
* Config via env/config (URL do Postgres)

**Labels:** area:db, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-312"
        title = "[P2] RLS e seguranca no Supabase"
        desc = "## Descricao
Regras para proteger dados por user/tenant
Separar Service Role key (somente backend) vs anon key

## Acceptance Criteria
* Documento `docs/src/security.md`
* RLS policies prontas (ou justificativa se nao usar)

**Labels:** area:db, type:chore"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 2 — Session Manager Unificado
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-320"
        title = "[P0] Implementar Session Manager (state machine)"
        desc = "## Descricao
Centralizar logica de:
* criar sessao se nao existir
* mapear Telegram `chat_id` -> `session_id`
* mapear VS Code user/workspace -> session_id
* TTL / arquivamento (opcional)

## Acceptance Criteria
* API interna: `resolve_session(source, external_id, hints) -> session_id`
* Logs claros para debugging

**Labels:** area:core, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-321"
        title = "[P1] Estrategia de session key"
        desc = "## Descricao
Definir como o VS Code vai garantir mesmo chat:
* Opcao A: `X-Session-Id` header
* Opcao B: `metadata.session_id` no corpo OpenAI-compatible
* Opcao C: derivar de user_id + workspace_id

## Acceptance Criteria
* Decisao documentada e implementada
* Fallback seguro (se nao vier session_id, criar nova)

**Labels:** area:core, type:feature"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 3 — API OpenAI-Compatible (para VS Code)
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-330"
        title = "[P0] Criar endpoint /v1/chat/completions"
        desc = "## Descricao
Implementar compatibilidade com OpenAI Chat Completions:
* `model`
* `messages[]`
* `temperature`, `top_p`, etc (aceitar e repassar)
* Ignorar/aceitar campos extras sem quebrar.

## Acceptance Criteria
* `curl` de exemplo funciona
* Retorna no formato esperado (id, object, choices, usage quando possivel)

**Labels:** area:api, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-331"
        title = "[P0] Unificacao de historico: server is source of truth"
        desc = "## Descricao
Quando o VS Code enviar `messages[]`, o servidor:
1. resolve `session_id`
2. busca historico do DB
3. decide merge strategy:
   * preferir DB como verdade
   * aceitar mensagem do request como nova entrada (nao confiar no historico do cliente)
4. salva a mensagem do usuario
5. chama LLM
6. salva resposta do assistant

## Acceptance Criteria
* Duplicacao evitada (idempotencia via `client_message_id` opcional)
* Teste: mandar uma msg no Telegram, depois no VS Code e ver o contexto carregado

**Labels:** area:api, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-332"
        title = "[P2] Streaming SSE para /v1/chat/completions"
        desc = "## Descricao
Implementar `stream: true` compativel (SSE)
Util para UX de VS Code

## Acceptance Criteria
* Cliente que suporta SSE recebe tokens incrementais
* Sem quebrar modo nao-streaming

**Labels:** area:api, type:feature"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 4 — Integracao Telegram no Mesmo Core
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-340"
        title = "[P0] Bot Telegram: handler de updates -> Garra Core"
        desc = "## Fluxo
* Recebe update
* extrai `chat_id`
* resolve `session_id`
* salva mensagem
* chama pipeline LLM (mesmo do VS Code)
* responde com `sendMessage`

## Acceptance Criteria
* Mensagem enviada no Telegram aparece no DB e influencia respostas no VS Code

**Labels:** area:telegram, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-341"
        title = "[P1] Comandos e controle de sessao"
        desc = "## Descricao
Adicionar comandos uteis:
* `/new` cria nova sessao e seta ativa
* `/session` mostra session_id
* `/reset` limpa contexto curto (nao apaga DB, mas inicia thread nova)
* `/model <nome>` (se suportar)

## Acceptance Criteria
* Comandos registrados e documentados
* Logs e erros amigaveis

**Labels:** area:telegram, type:feature"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 5 — Memory & Context Policy
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-350"
        title = "[P1] Policy de contexto (janela + resumo)"
        desc = "## Estrategia
* puxar ultimas N mensagens (ex: 30-80)
* se historico for grande: usar `summary` incremental
* Salvar summary em `chat_summaries`

## Acceptance Criteria
* Conversas longas nao degradam custo/latencia absurdamente
* Teste: conversa com >200 msgs mantem coerencia

**Labels:** area:core, type:feature"
        priority = 2
    },
    
    @{
        id = "GAR-351"
        title = "[P2] Normalizacao e sanitizacao de conteudo"
        desc = "## Descricao
* Remover ANSI, controlar tamanho maximo, prevenir payload gigante
* Padrao para logs

## Acceptance Criteria
* Nenhuma mensagem explode o request
* Conteudo seguro em logs

**Labels:** area:core, type:chore"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 6 — Providers & Model Routing
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-360"
        title = "[P1] Router unificado de providers (OpenRouter / OpenAI / outros)"
        desc = "## Descricao
Garantir que:
* VS Code e Telegram usam o mesmo router
* `model` pode ser override por sessao/comando
* Headers app identity (como ja fizeram no OpenRouter) permanecem corretos

## Acceptance Criteria
* `model` default + override funciona
* Observabilidade do provider (qual modelo respondeu)

**Labels:** area:core, type:feature"
        priority = 2
    },
    
    @{
        id = "GAR-361"
        title = "[P1] Config: config.basic.yml + env"
        desc = "## Descricao
Adicionar:
* DB URL
* defaults de contexto
* modo streaming
* toggles por canal (telegram/vscode)

## Acceptance Criteria
* Rodar local com `.env`
* Rodar producao com variaveis

**Labels:** area:core, type:chore"
        priority = 2
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 7 — VS Code Client Strategy
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-370"
        title = "[P0] Plano A: Configurar VS Code com extensao OpenAI-compatible"
        desc = "## Descricao
Escolher extensao que permite `baseUrl` custom + `apiKey` custom
Documentar configuracao apontando para `http://localhost:XXXX/v1`

## Acceptance Criteria
* VS Code envia prompts via `/v1/chat/completions` do Garra
* Mesma sessao do Telegram continua

**Labels:** area:vscode, type:feature"
        priority = 1
    },
    
    @{
        id = "GAR-371"
        title = "[P3] Plano B (opcional): Extensao propria Garra Chat"
        desc = "## Descricao
Painel de chat com session selector
Realtime via polling/SSE

## Acceptance Criteria
* MVP: enviar/receber mensagens
* Selecionar sessao e ver historico

**Labels:** area:vscode, type:feature"
        priority = 4
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 8 — Observabilidade, Qualidade, Seguranca
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-380"
        title = "[P1] Logs estruturados + trace ids"
        desc = "## Descricao
`request_id`, `session_id`, `source`, latencia, provider/model
Export (stdout + opcional JSON)

## Acceptance Criteria
* Debug simples de ponta a ponta

**Labels:** area:core, type:chore"
        priority = 2
    },
    
    @{
        id = "GAR-381"
        title = "[P2] Rate limit e abuse protection"
        desc = "## Descricao
* Evitar flood (Telegram)
* Limitar payload e requests/minuto

## Acceptance Criteria
* Respostas adequadas em caso de limite excedido

**Labels:** area:core, type:chore"
        priority = 3
    },
    
    @{
        id = "GAR-382"
        title = "[P2] Testes end-to-end Telegram <-> VS Code"
        desc = "## Descricao
Script de teste:
* simula update do Telegram
* faz call OpenAI-compatible
* valida continuidade

## Acceptance Criteria
* Teste automatizado no CI

**Labels:** area:core, type:chore"
        priority = 3
    },

    # ═══════════════════════════════════════════════════════════════════════════════
    # EPIC 9 — Deploy & Runbooks
    # ═══════════════════════════════════════════════════════════════════════════════
    
    @{
        id = "GAR-390"
        title = "[P1] Docker compose: Garra + Postgres (ou Supabase local)"
        desc = "## Setup dev
* `docker-compose.yml`
* migrations
* seed opcional

## Acceptance Criteria
* `docker compose up` e funciona

**Labels:** area:core, type:chore"
        priority = 2
    },
    
    @{
        id = "GAR-391"
        title = "[P2] Runbook producao (Cloudflare Tunnel / VPS / etc.)"
        desc = "## Descricao
* Como expor `/v1` com TLS
* Como configurar webhook/polling Telegram
* Como rotacionar secrets

## Acceptance Criteria
* Documento como subir pronto

**Labels:** area:core, type:chore"
        priority = 3
    }
)

# Criar issues
$created = 0
$failed = 0

Write-Host ""
Write-Host "=== Criando $($roadmap.Count) issues do Garra Core Chat Sync ===" -ForegroundColor Cyan

foreach ($item in $roadmap) {
    Write-Host "Criando $($item.id): $($item.title)..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $backlogState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 10
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueCreate.success) {
            Write-Host "  $($item.id) -> OK" -ForegroundColor Green
            $created++
        } else {
            Write-Host "  $($item.id) -> FALHA: $($resp | ConvertTo-Json -Compress)" -ForegroundColor Red
            $failed++
        }
    } catch {
        $errMsg = $_.Exception.Message
        if ($errMsg -like "*duplicate*") {
            Write-Host "  $($item.id) -> JA EXISTE (pulando)" -ForegroundColor Yellow
        } else {
            try {
                $errResp = [System.Text.Encoding]::UTF8.GetString($_.Exception.Response.GetResponseStream().ReadToEnd())
                Write-Host "  $($item.id) -> ERRO: $errMsg | Response: $errResp" -ForegroundColor Red
            } catch {
                Write-Host "  $($item.id) -> ERRO: $errMsg" -ForegroundColor Red
            }
        }
        $failed++
    }
}

Write-Host ""
Write-Host "=== Resultado ===" -ForegroundColor Green
Write-Host "Criadas: $created" -ForegroundColor Green
Write-Host "Falhas/Existentes: $failed" -ForegroundColor Yellow
Write-Host ""
Write-Host "Roadmap Garra Core Chat Sync criado com sucesso!" -ForegroundColor Green
Write-Host ""
Write-Host "=== Ordem recomendada de execucao ===" -ForegroundColor Cyan
Write-Host "1. EPIC 1 (DB) + EPIC 2 (Session Manager)" -ForegroundColor White
Write-Host "2. EPIC 3.1 + 3.2 (OpenAI-compatible + historico server-side)" -ForegroundColor White
Write-Host "3. EPIC 4.1 (Telegram no mesmo core)" -ForegroundColor White
Write-Host "4. EPIC 7.1 (VS Code via extensao OpenAI-compatible)" -ForegroundColor White
Write-Host "5. EPIC 5 (resumo/janela) + EPIC 6 (routing)" -ForegroundColor White
Write-Host "6. EPIC 8/9 (qualidade + deploy)" -ForegroundColor White
