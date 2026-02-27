# PowerShell script para criar roadmap completo no Linear
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\create_linear_roadmap.ps1

param(
    [string]$ApiKey = $env:LINEAR_API_KEY
)

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    Write-Host "Obtenha em: https://linear.app/settings/api" -ForegroundColor Yellow
    Write-Host "Exemplo: `$env:LINEAR_API_KEY='lin_api_xxx'; .\create_linear_roadmap.ps1" -ForegroundColor Cyan
    exit 1
}

Write-Host "Criando roadmap completo no Linear..." -ForegroundColor Green

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

# Buscar estado "Todo" ou primeiro estado disponivel
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$todoState = $stateResponse.data.team.states.nodes | Where-Object { $_.name -eq "Todo" -or $_.name -eq "Backlog" } | Select-Object -First 1

if (-not $todoState) {
    $todoState = $stateResponse.data.team.states.nodes | Select-Object -First 1
}

Write-Host "Usando estado: $($todoState.name) ($($todoState.id))" -ForegroundColor Green

# Definir roadmap completo
$roadmap = @(
    # Fase 0 - Baseline & Controle
    @{ id = "GAR-200"; title = "Versionamento Semantico (v0.2.0)"; desc = "Implementar versionamento semantico para release v0.2.0 com changelog automatico e tags"; priority = 2; },
    @{ id = "GAR-201"; title = "Observabilidade Estruturada"; desc = "Adicionar logging estruturado com tracing, metricas Prometheus e integracao com OpenTelemetry"; priority = 2; },
    
    # Fase 1 - Voice Real (Prioridade Maxima)
    @{ id = "GAR-210"; title = "Whisper STT Estavel"; desc = "Implementar reconhecimento de voz com Whisper via Ollama ou API, com fallback e tratamento de erros"; priority = 1; },
    @{ id = "GAR-211"; title = "Voice Handler Real"; desc = "Criar VoiceHandler que integra STT -> LLM -> TTS com pipeline completo"; priority = 1; },
    @{ id = "GAR-212"; title = "Voice E2E Telegram"; desc = "Validar fluxo completo: Telegram audio -> STT -> LLM -> TTS -> Telegram audio"; priority = 1; },
    
    # Fase 2 - MCP Real
    @{ id = "GAR-220"; title = "MCP Spawn Correto"; desc = "Implementar spawn correto de processos MCP com lifecycle management e cleanup"; priority = 2; },
    @{ id = "GAR-221"; title = "MCP Tool Health"; desc = "Adicionar health check para tools MCP, verificando disponibilidade e latencia"; priority = 2; },
    @{ id = "GAR-222"; title = "Slash Commands Dinamicos via MCP"; desc = "Auto-registrar ferramentas MCP como slash commands no Telegram dinamicamente"; priority = 2; },
    
    # Fase 3 - Multi-Agent Real
    @{ id = "GAR-230"; title = "Agent Router Consolidado"; desc = "Implementar router que distribui requests para agents baseados em contexto e capacidade"; priority = 2; },
    @{ id = "GAR-231"; title = "Agent State Persistente"; desc = "Criar persistencia de estado de agents em SQLite com session management"; priority = 2; },
    @{ id = "GAR-232"; title = "Agent Tool Escalation"; desc = "Implementar escalacao de tools entre agents com contexto compartilhado"; priority = 2; },
    
    # Fase 4 - Hardening de Producao
    @{ id = "GAR-240"; title = "Provider Resilience"; desc = "Implementar circuit breaker, retry com backoff e failover automatico entre providers"; priority = 2; },
    @{ id = "GAR-241"; title = "Rate Limiting"; desc = "Adicionar rate limiting por tenant com token bucket e configuracao por provider"; priority = 2; },
    @{ id = "GAR-242"; title = "Seguranca MCP"; desc = "Auditar e hardening de seguranca para execucao de tools MCP com sandboxing"; priority = 2; },
    
    # Fase 5 - UX & Produto
    @{ id = "GAR-250"; title = "Slash Help Melhorado"; desc = "Criar /help dinamico com exemplos,docs e busca de comandos"; priority = 3; },
    @{ id = "GAR-251"; title = "Admin Web Console Real"; desc = "Implementar dashboard admin funcional com estatisticas em tempo real"; priority = 3; },
    @{ id = "GAR-252"; title = "WebSocket Dashboard"; desc = "Adicionar WebSocket para updates em tempo real no admin console"; priority = 3; },
    
    # Fase 6 - Ecossistema
    @{ id = "GAR-260"; title = "Docker Oficial"; desc = "Publicar imagem Docker oficial no ghcr.io com Multi-arch (amd64/arm64)"; priority = 3; },
    @{ id = "GAR-261"; title = "Binario Windows Installer"; desc = "Criar instalador Windows (.exe) com NSIS e configuracao automatica"; priority = 3; },
    @{ id = "GAR-262"; title = "Documentacao Tecnica"; desc = "Escrever docs completas em garraia.org com API reference, guides e troubleshooting"; priority = 3; }
)

# Criar issues
$created = 0
$failed = 0

foreach ($item in $roadmap) {
    Write-Host "Criando $($item.id): $($item.title)..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $todoState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 5
    
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
            # Tentar extrair mensagem de erro do response
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
Write-Host "Falhas: $failed" -ForegroundColor Red
Write-Host ""
Write-Host "Roadmap criado com sucesso!" -ForegroundColor Green
