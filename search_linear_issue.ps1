# Search for a specific issue by identifier
$apiKey = "REDACTED_LINEAR_API_KEY"

$query = @"
query {
  issue(identifier: "GAR-221") {
    id
    identifier
    title
    state {
      name
    }
  }
}
"@

$body = @{
    query = $query
} | ConvertTo-Json

$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

$response | ConvertTo-Json -Depth 10
