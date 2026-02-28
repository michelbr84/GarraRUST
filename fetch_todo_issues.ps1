# Script para buscar issues em Todo
$apiKey = "REDACTED_LINEAR_API_KEY"
$headers = @{
    "Authorization" = $apiKey
    "Content-Type" = "application/json"
}

# Buscar team ID
$query = @{ query = "query { teams { nodes { id name key } } }" } | ConvertTo-Json
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $query
$team = $resp.data.teams.nodes | Where-Object { $_.key -eq "GAR" }
Write-Host "Team: $($team.name) ($($team.key)) - ID: $($team.id)"

# Buscar estados
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery
Write-Host "`nEstados disponiveis:"
$stateResp.data.team.states.nodes | ForEach-Object { Write-Host "  - $($_.name) [ID: $($_.id)]" }

# Buscar issues em Todo
$todoState = $stateResp.data.team.states.nodes | Where-Object { $_.name -eq "Todo" } | Select-Object -First 1
Write-Host "`nBuscando issues em Todo..."

$issuesQuery = @{ query = "query { issues(filter: { state: { id: { eq: `"$($todoState.id)`"} } }, first: 50) { nodes { id identifier title priority state { name } } } }" } | ConvertTo-Json
$issuesResp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $issuesQuery

Write-Host "`n=== ISSUES EM TODO ==="
$issuesResp.data.issues.nodes | ForEach-Object { 
    $p = "P" + $_.priority
    Write-Host "$($_.identifier) [$p] - $($_.title)"
}
Write-Host "`nTotal: $($issuesResp.data.issues.nodes.Count) issues"
