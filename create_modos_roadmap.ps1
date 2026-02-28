# PowerShell script para criar roadmap de Modos de Execução no Linear
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\create_modos_roadmap.ps1

param(
    [string]$ApiKey = $null
)

# Try to get API key from environment or .env file
if ([string]::IsNullOrEmpty($ApiKey)) {
    $ApiKey = $env:LINEAR_API_KEY
}

# Try to read from .env file if not in environment
if ([string]::IsNullOrEmpty($ApiKey)) {
    $envPath = Join-Path $PSScriptRoot ".env"
    if (Test-Path $envPath) {
        Get-Content $envPath | ForEach-Object {
            if ($_ -match "^LINEAR_API_KEY=(.+)$") {
                $ApiKey = $matches[1].Trim()
            }
        }
    }
}

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: LINEAR_API_KEY nao encontrada" -ForegroundColor Red
    Write-Host "Defina em .env ou variavel de ambiente" -ForegroundColor Yellow
    exit 1
}

Write-Host "Criando roadmap de Modos de Execução no Linear..." -ForegroundColor Green

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

# Buscar estado
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$todoState = $stateResponse.data.team.states.nodes | Where-Object { $_.name -eq "Todo" -or $_.name -eq "Backlog" } | Select-Object -First 1

if (-not $todoState) {
    $todoState = $stateResponse.data.team.states.nodes | Select-Object -First 1
}

Write-Host "Usando estado: $($todoState.name)" -ForegroundColor Green
Write-Host ""

# Função para criar issue
function New-LinearIssue {
    param(
        [string]$title,
        [string]$description,
        [int]$priority
    )
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $todoState.id
                title = $title
                description = $description
                priority = $priority
            }
        }
    } | ConvertTo-Json -Depth 5
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueCreate.success) {
            Write-Host "  OK" -ForegroundColor Green
            return $true
        }
    } catch {
        if ($_.Exception.Message -like "*duplicate*") {
            Write-Host "  JA EXISTE" -ForegroundColor Yellow
        } else {
            Write-Host "  ERRO: $($_.Exception.Message)" -ForegroundColor Red
        }
    }
    return $false
}

# ==============================================================================
# EPIC GAR-M0 — Princípios, Guardrails e Compatibilidade (P0)
# ==============================================================================
Write-Host "EPIC GAR-M0 - Princípios e Compatibilidade" -ForegroundColor Cyan
Write-Host "  GAR-300: Definir contrato de Modo (runtime vs UI)..." -NoNewline
$desc0_1 = "Formalizar modo como estrategia de execucao. Definir precedencia: Header > Comando > Canal > Usuario > Default. Documentar em docs/src/modes.md"
New-LinearIssue -title "M0-1: Definir contrato de Modo (runtime vs UI)" -description $desc0_1 -priority 1

Write-Host "  GAR-301: Compatibilidade OpenAI streaming + tool calling..." -NoNewline
$desc0_2 = "Garantir /v1/chat/completions com SSE e tool_choice (auto/none/required/named). Testes de integracao com stream=true e tool_choice required."
New-LinearIssue -title "M0-2: Compatibilidade OpenAI streaming + tool calling" -description $desc0_2 -priority 1

# ==============================================================================
# EPIC GAR-M1 — Núcleo de Modos (P0)
# ==============================================================================
Write-Host "EPIC GAR-M1 - Nucleo de Modos" -ForegroundColor Cyan
Write-Host "  GAR-302: AgentMode enum + ModeProfile struct..." -NoNewline
$desc1_1 = "Criar enum AgentMode: auto, search, architect, code, ask, debug, orchestrator, review, edit. Criar ModeProfile com name, description, system_prompt_template, tool_policy, llm_defaults, limits. API interna: get_mode_profile(mode) e list_modes()."
New-LinearIssue -title "M1-1: AgentMode enum + ModeProfile struct" -description $desc1_1 -priority 1

Write-Host "  GAR-303: Persistencia por sessao e canal..." -NoNewline
$desc1_2 = "Persistir modo atual no session_store (sessions.db) por session_id. Permitir default por canal: telegram=ask, openai_api=auto, web=auto. Sessoes mantem modo entre requests."
New-LinearIssue -title "M1-2: Persistencia por sessao e por canal" -description $desc1_2 -priority 1

Write-Host "  GAR-304: Comandos /mode e /modes..." -NoNewline
$desc1_3 = "Implementar comandos: /mode (mostra atual), /mode <nome> (muda), /modes (lista). Suportar header X-Agent-Mode no OpenAI endpoint. Precedence: Header > Comando > Persistido."
New-LinearIssue -title "M1-3: Comandos universais /mode e /modes" -description $desc1_3 -priority 1

# ==============================================================================
# EPIC GAR-M2 — Tool Policy por Modo (P0)
# ==============================================================================
Write-Host "EPIC GAR-M2 - Tool Policy por Modo" -ForegroundColor Cyan
Write-Host "  GAR-305: ToolPolicyEngine..." -NoNewline
$desc2_1 = "Engine que decide: tools permitidas, proibidas, read-only (bash com allowlist), required por intencao. search: file_read, repo_search, bash read-only. code: file_write, file_read, bash. ask: tools opcionais. safe: nega bash e file_write."
New-LinearIssue -title "M2-1: ToolPolicyEngine" -description $desc2_1 -priority 1

Write-Host "  GAR-306: Suporte a tool_choice no OpenAI..." -NoNewline
$desc2_2 = "Respeitar tool_choice: none (desabilita tools), auto (normal), required (forca tool call quando modelo aceitar)."
New-LinearIssue -title "M2-2: Suporte a tool_choice no OpenAI request" -description $desc2_2 -priority 1

# ==============================================================================
# EPIC GAR-M3 — Auto Mode Router (P0)
# ==============================================================================
Write-Host "EPIC GAR-M3 - Auto Mode Router" -ForegroundColor Cyan
Write-Host "  GAR-307: Heuristicas deterministicas..." -NoNewline
$desc3_1 = "Router por padroes: path (C:, G:, /home/) = search/debug; refatorar/implementar/criar = code; explique/o que e = ask; erro/stacktrace/panic = debug; roadmap/design = architect; review/diff = review. Logs: auto -> resolved_mode=..."
New-LinearIssue -title "M3-1: Heuristicas deterministicas" -description $desc3_1 -priority 1

Write-Host "  GAR-308: Auto por micro-LMM router opcional..." -NoNewline
$desc3_2 = "Usar chamada curta ao LLM para classificar modo quando heuristica for ambigua. Com fallback para heuristica. Feature flag agent.auto_router_llm_enabled."
New-LinearIssue -title "M3-2: Auto por micro-LMM router opcional" -description $desc3_2 -priority 2

# ==============================================================================
# EPIC GAR-M4 — Search Mode (P0)
# ==============================================================================
Write-Host "EPIC GAR-M4 - Search Mode" -ForegroundColor Cyan
Write-Host "  GAR-309: Tool nativa repo_search..." -NoNewline
$desc4_1 = "Tool Rust dedicada para busca segura: parametros query, globs, max_results, context_lines. Retorna trechos e paths. Respeita limites e mascara tokens."
New-LinearIssue -title "M4-1: Tool nativa repo_search" -description $desc4_1 -priority 1

Write-Host "  GAR-310: Tool nativa list_dir..." -NoNewline
$desc4_2 = "Tool dedicada para listar diretorios com filtros (nomes, depth). Evita comandos perigosos."
New-LinearIssue -title "M4-2: Tool nativa list_dir" -description $desc4_2 -priority 1

# ==============================================================================
# EPIC GAR-M5 — UI Modo Picker (P1)
# ==============================================================================
Write-Host "EPIC GAR-M5 - UI Modo Picker" -ForegroundColor Cyan
Write-Host "  GAR-311: API HTTP para modos..." -NoNewline
$desc5_1 = "Endpoints: GET /api/modes (lista), POST /api/mode/select (seta modo), GET /api/mode/current (modo atual), POST /api/modes/custom (cria custom), PATCH /api/modes/custom/:id (edita)."
New-LinearIssue -title "M5-1: API HTTP para modos" -description $desc5_1 -priority 2

Write-Host "  GAR-312: UI Mode Sidebar..." -NoNewline
$desc5_2 = "Componentes: Search input (filtra modos), Lista com icone + descricao (Architect/Code/Ask/Debug/Orchestrator/Review), Secao Custom, Auto destacado. Estado persistido por sessao."
New-LinearIssue -title "M5-2: UI Mode Sidebar" -description $desc5_2 -priority 2

Write-Host "  GAR-313: UI Edit Mode..." -NoNewline
$desc5_3 = "Criar/editar modo custom: nome, descricao, base mode, tool policy overrides, prompt override, defaults (temp/max_tokens)."
New-LinearIssue -title "M5-3: UI Edit Mode" -description $desc5_3 -priority 2

# ==============================================================================
# EPIC GAR-M6 — Integração Continue/VS Code (P1)
# ==============================================================================
Write-Host "EPIC GAR-M6 - Integracao Continue/VS Code" -ForegroundColor Cyan
Write-Host "  GAR-314: Templates de continue config.yaml..." -NoNewline
$desc6_1 = "Criar docs/src/continue-modes.md com exemplos: auto por padrao, override com X-Agent-Mode, modelos OpenAI-compatible. Snippets para 3 perfis: auto, code, debug."
New-LinearIssue -title "M6-1: Templates de continue config.yaml" -description $desc6_1 -priority 2

Write-Host "  GAR-315: Suporte a headers no Continue..." -NoNewline
$desc6_2 = "Investigar requestOptions/headers suportados pelo Continue. Se nao suportar, usar /mode via chat ou prefix (mode: debug)."
New-LinearIssue -title "M6-2: Suporte a headers no Continue via config" -description $desc6_2 -priority 3

# ==============================================================================
# EPIC GAR-M7 — Orchestrator Mode (P1)
# ==============================================================================
Write-Host "EPIC GAR-M7 - Orchestrator Mode" -ForegroundColor Cyan
Write-Host "  GAR-316: Execucao multi-etapas com planos..." -NoNewline
$desc7_1 = "Orchestrator gera plano curto, executa tools, valida, responde. Ajustar regras de loop e timeout por modo."
New-LinearIssue -title "M7-1: Execucao multi-etapas com planos" -description $desc7_1 -priority 2

Write-Host "  GAR-317: Checklist de seguranca para bash..." -NoNewline
$desc7_2 = "Allowlist/denylist de comandos, limitar cwd ao workspace permitido, bloquear comandos destrutivos (ex: rm -rf)."
New-LinearIssue -title "M7-2: Checklist de seguranca para bash" -description $desc7_2 -priority 2

# ==============================================================================
# EPIC GAR-M8 — Review Mode (P2)
# ==============================================================================
Write-Host "EPIC GAR-M8 - Review Mode" -ForegroundColor Cyan
Write-Host "  GAR-318: Tool git_diff..." -NoNewline
$desc8_1 = "Tool para retornar git diff e status, com limites. Revisao de changes localmente."
New-LinearIssue -title "M8-1: Tool git_diff" -description $desc8_1 -priority 3

# ==============================================================================
# EPIC GAR-M9 — Observabilidade, QA e Documentação (P0/P1)
# ==============================================================================
Write-Host "EPIC GAR-M9 - Observabilidade e Documentacao" -ForegroundColor Cyan
Write-Host "  GAR-319: Logs padronizados por request..." -NoNewline
$desc9_1 = "Logar sempre: channel, session_id, user_id, mode, resolved_mode, provider, model, tool_uses. Uma linha por request."
New-LinearIssue -title "M9-1: Logs padronizados por request" -description $desc9_1 -priority 1

Write-Host "  GAR-320: Testes modos e politicas..." -NoNewline
$desc9_2 = "Testes unitarios do router auto, integracao /v1/chat/completions com streaming SSE, testes tool_choice (none/auto/required)."
New-LinearIssue -title "M9-2: Testes modos e politicas" -description $desc9_2 -priority 1

Write-Host "  GAR-321: Documentacao completa..." -NoNewline
$desc9_3 = "Documentar em: docs/src/modes.md (conceitos), docs/src/modes-ui.md (UI e endpoints), docs/src/continue.md (config e troubleshooting)."
New-LinearIssue -title "M9-3: Documentacao completa" -description $desc9_3 -priority 2

Write-Host ""
Write-Host "=== Roadmap de Modos de Execucao criado ===" -ForegroundColor Green
Write-Host "Total: 22 issues criadas" -ForegroundColor Cyan
