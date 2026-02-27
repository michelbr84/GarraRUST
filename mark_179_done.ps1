# Marcar GAR-179 como Done

$headers = @{
    'Authorization' = 'lin_api_3YBH54BPlThDxZnrKpS06wO23VUE1GmSHM8TgiQ5'
    'Content-Type' = 'application/json'
}

# Buscar estado Done
$query = @{ query = 'query { team(id: "GAR") { states { nodes { id name } } } }' } | ConvertTo-Json
$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method POST -Headers $headers -Body $query -ErrorAction Stop
$doneState = $response.data.team.states.nodes | Where-Object { $_.name -eq 'Done' }

# Marcar GAR-179 como Done
$mutation = @{
    query = 'mutation { issueUpdate(id: "GAR-179", input: { stateId: "' + $doneState.id + '" }) { success } }'
} | ConvertTo-Json

Write-Host "Marcando GAR-179 como Done..." -ForegroundColor Cyan
$resp = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
Write-Host "Resultado: $($resp | ConvertTo-Json -Compress)"
