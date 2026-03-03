$apiKey = "REDACTED_LINEAR_API_KEY"
$query = @"
query {
  workflowStates(filter: { team: { key: { eq: "GAR" } } }) {
    nodes { id name }
  }
}
"@
$body = @{ query = $query } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body
$resp.data.workflowStates.nodes | ForEach-Object { "$($_.name): $($_.id)" }
