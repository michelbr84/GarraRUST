# PowerShell script para atualizar issues no Linear
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\update_linear.ps1

param(
    [string]$ApiKey = $env:LINEAR_API_KEY
)

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    Write-Host "Obtenha em: https://linear.app/settings/api" -ForegroundColor Yellow
    Write-Host "Exemplo: `$env:LINEAR_API_KEY='lin_api_xxx'; .\update_linear.ps1" -ForegroundColor Cyan
    exit 1
}

Write-Host "Atualizando issues no Linear..." -ForegroundColor Green

# Lista de issues para marcar como done
$issues = @(
    "GAR-170", "GAR-165", "GAR-166", "GAR-167", "GAR-168", "GAR-169",
    "GAR-160", "GAR-162", "GAR-163", "GAR-158", "GAR-164", "GAR-161",
    "GAR-171", "GAR-172", "GAR-157", "GAR-173", "GAR-174", "GAR-175", "GAR-176"
)

$headers = @{
    "Authorization" = $ApiKey
    "Content-Type" = "application/json"
}

# Primeiro, buscar o estado "Done" da equipe
Write-Host "Buscando estado 'Done'..." -ForegroundColor Cyan
$query = '{ "query": "query { team(id: \"GAR\") { states { nodes { id name } } } }" }'
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $query -ErrorAction Stop
$doneState = $response.data.team.states.nodes | Where-Object { $_.name -eq "Done" }

if (-not $doneState) {
    Write-Host "Estado 'Done' nao encontrado. Tentando 'done'..." -ForegroundColor Yellow
    $doneState = $response.data.team.states.nodes | Where-Object { $_.name -eq "done" }
}

if (-not $doneState) {
    Write-Host "Estados disponiveis:" -ForegroundColor Yellow
    $response.data.team.states.nodes | ForEach-Object { Write-Host "  - $($_.name): $($_.id)" -ForegroundColor Gray }
    Write-Host "Informe o estado correto no script." -ForegroundColor Red
    exit 1
}

Write-Host "Estado 'Done' encontrado: $($doneState.id)" -ForegroundColor Green

foreach ($issue in $issues) {
    Write-Host "Marcando $issue como done..." -ForegroundColor Cyan
    
    # Mutation correta do Linear API
    $body = @{
        query = "mutation { issueUpdate(id: `"$issue`", input: { stateId: `"$($doneState.id)`" }) { success } }"
    } | ConvertTo-Json
    
    try {
        $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $body -ErrorAction Stop
        if ($response.data.issueUpdate.success) {
            Write-Host "  $issue -> OK" -ForegroundColor Green
        } else {
            Write-Host "  $issue -> FALHA" -ForegroundColor Red
        }
    } catch {
        Write-Host "  $issue -> ERRO: $($_.Exception.Message)" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "Concluido! Verifique o Linear para confirmar." -ForegroundColor Green
