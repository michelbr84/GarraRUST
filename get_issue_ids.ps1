$apiKey = "REDACTED_LINEAR_API_KEY"
$query = @"
query {
  issues(filter: { team: { key: { eq: "GAR" } }, number: { in: [198, 199, 201, 203, 204, 206, 159] } }) {
    nodes { id identifier title }
  }
}
"@
$body = @{ query = $query } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body
$resp.data.issues.nodes | Sort-Object { [int]($_.identifier -replace 'GAR-', '') } | ForEach-Object {
    "$($_.identifier): $($_.id)"
}
