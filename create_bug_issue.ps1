#!/usr/bin/env pwsh

# Create urgent issue for router fix in Linear

$ErrorActionPreference = "Stop"

# Load API key from .env
$envPath = Join-Path $PSScriptRoot ".env"
if (Test-Path $envPath) {
    Get-Content $envPath | ForEach-Object {
        if ($_ -match "^LINEAR_API_KEY=(.+)$") {
            $env:LINEAR_API_KEY = $matches[1].Trim()
        }
    }
}

if (-not $env:LINEAR_API_KEY) {
    Write-Host "LINEAR_API_KEY not set." -ForegroundColor Red
    exit 1
}

$headers = @{
    "Authorization" = $env:LINEAR_API_KEY
    "Content-Type" = "application/json"
}

# First, get the team ID for GAR
$teamQuery = '{"query":"{ teams(first: 10) { nodes { id key name } } }"}'
$teamResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers $headers -Body $teamQuery

$garTeam = $teamResponse.data.teams.nodes | Where-Object { $_.key -eq "GAR" }
if (-not $garTeam) {
    Write-Host "Could not find GAR team" -ForegroundColor Red
    exit 1
}

$teamId = $garTeam.id
Write-Host "Found GAR team: $teamId"

# Get the urgent priority ID (priority 0)
$priorityQuery = '{"query":"{ issuePriorityValues { nodes { id priority label } } }"}'
$priorityResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers $headers -Body $priorityQuery

# Linear uses priority 0 = Urgent, 1 = High, 2 = Medium, 3 = Low, 4 = Backlog
$urgentPriority = $priorityResponse.data.issuePriorityValues.nodes | Where-Object { $_.priority -eq 0 }
$urgentPriorityId = $urgentPriority.id

# Create the issue
$mutation = @"
{
  "query": "mutation { issueCreate(input: { teamId: \"$teamId\", title: \"[URGENT] Fix gateway startup panic: axum route segments with ':' instead of '{ }'\", description: \"## Problema\\n\\nO gateway faz **panic fatal** na inicialização devido ao axum Router invalidar rotas com segmentos que começam com `:` (dois pontos). O erro ocorre em:\\n\\n```\\nPath segments must not start with ':'. For capture groups, use '{capture}'.\\n```\\n\\n## Causa Raiz\\n\\nA rota `/:id` está sendo usada em vez de `/{id}`. O axum v0.7+ requer o formato de captura `/{key}`.\\n\\n## Correção Aplicada\\n\\nArquivo: `crates/garraia-gateway/src/router.rs`\\n\\nMudou:\\n```rust\\n.route(\\\"/api/modes/custom/:id\\\", ...)  // ANTES\\n```\\nPara:\\n```rust\\n.route(\\\"/api/modes/custom/{id}\\\", ...)  // DEPOIS\\n```\\n\\n## Acceptance Criteria\\n\\n- [x] `cargo build --workspace` compila sem erros\\n- [x] `cargo test --workspace` passa (auth tests agora passam)\\n- [x] Gateway inicia sem panic\\n\\n## Notas\\n\\nOs testes de auth que falhavam (ws_accepts_correct_api_key_header, etc.) agora passam após a correção do router.\", priority: $urgentPriorityId, labels: [\"bug\", \"urgent\"] }) { success issue { id identifier title } } }"
}
"@

try {
    $result = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
        -Method Post `
        -Headers $headers `
        -Body $mutation
    
    if ($result.data.issueCreate.success) {
        $issue = $result.data.issueCreate.issue
        Write-Host "SUCCESS: Created issue [$($issue.identifier)] - $($issue.title)" -ForegroundColor Green
        Write-Host "URL: https://linear.app/chatgpt25/issue/$($issue.identifier)" -ForegroundColor Cyan
    } else {
        Write-Host "Failed to create issue" -ForegroundColor Red
    }
} catch {
    Write-Host "Error: $_" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
}
