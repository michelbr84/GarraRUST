# PowerShell script para marcar issues como Done no Linear

$headers = @{
    'Authorization' = 'lin_api_3YBH54BPlThDxZnrKpS06wO23VUE1GmSHM8TgiQ5'
    'Content-Type' = 'application/json'
}

Write-Host "Marcando issues como Done..." -ForegroundColor Green

# Buscar estado Done
$query = @{ query = 'query { team(id: "GAR") { states { nodes { id name } } } }' } | ConvertTo-Json
$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method POST -Headers $headers -Body $query -ErrorAction Stop
$doneState = $response.data.team.states.nodes | Where-Object { $_.name -eq 'Done' }

Write-Host "Estado Done: $($doneState.id)" -ForegroundColor Green

# Issues ja implementadas (verificadas no codigo)
$implemented = @(
    'GAR-178',  # Observabilidade Estruturada - ja existe tracing + prometheus
    'GAR-182',  # MCP Spawn Correto - ja existe em mcp/manager.rs
    'GAR-183',  # MCP Tool Health - ja existe spawn_health_monitor
    'GAR-188',  # Provider Resilience - ja existe circuit breaker
    'GAR-189',  # Rate Limiting - ja existe em router.rs
    'GAR-192',  # Admin Web Console - ja existe
    'GAR-193',  # WebSocket Dashboard - ja existe
    'GAR-194',  # Docker - ja existe Dockerfile
    'GAR-195'   # Release workflow - ja existe release.yml
)

foreach ($issue in $implemented) {
    Write-Host "Marcando $issue como Done..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation { issueUpdate(id: `"$issue`", input: { stateId: `"$($doneState.id)`" }) { success } }"
    } | ConvertTo-Json
    
    try {
        $resp = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueUpdate.success) {
            Write-Host "  $issue -> OK" -ForegroundColor Green
        } else {
            Write-Host "  $issue -> FALHA" -ForegroundColor Red
        }
    } catch {
        Write-Host "  $issue -> ERRO: $($_.Exception.Message)" -ForegroundColor Red
    }
}

Write-Host "Concluido!" -ForegroundColor Green
