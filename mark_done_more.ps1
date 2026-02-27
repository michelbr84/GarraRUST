# PowerShell script para marcar mais issues como Done

$headers = @{
    'Authorization' = 'lin_api_3YBH54BPlThDxZnrKpS06wO23VUE1GmSHM8TgiQ5'
    'Content-Type' = 'application/json'
}

Write-Host "Marcando mais issues como Done..." -ForegroundColor Green

# Buscar estado Done
$query = @{ query = 'query { team(id: "GAR") { states { nodes { id name } } } }' } | ConvertTo-Json
$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method POST -Headers $headers -Body $query -ErrorAction Stop
$doneState = $response.data.team.states.nodes | Where-Object { $_.name -eq 'Done' }

Write-Host "Estado Done: $($doneState.id)" -ForegroundColor Green

# Issues ja implementadas
$implemented = @(
    'GAR-185',  # Agent Router Consolidado - ja existe em agent_router.rs
    'GAR-186',  # Agent State Persistente - ja existe em state.rs com agent_id
    'GAR-191'   # Slash Help Melhorado - ja e dinamico via list_for_role
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
