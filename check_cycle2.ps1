$apiKey = "REDACTED_LINEAR_API_KEY"

$query = @"
query {
  issues(filter: { team: { key: { eq: "GAR" } }, number: { in: [198, 199, 201, 203, 204, 206, 159] } }) {
    nodes {
      identifier
      title
      state { name }
      priority
      description
    }
  }
}
"@

$body = @{ query = $query } | ConvertTo-Json -Depth 10

$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body

$resp.data.issues.nodes | Sort-Object { [int]($_.identifier -replace 'GAR-', '') } | ForEach-Object {
    $desc = if ($_.description) { $_.description.Substring(0, [Math]::Min(150, $_.description.Length)) } else { "(sem descricao)" }
    Write-Host "[$($_.identifier)] P$($_.priority) | $($_.state.name) | $($_.title)"
    Write-Host "  $desc"
    Write-Host ""
}
