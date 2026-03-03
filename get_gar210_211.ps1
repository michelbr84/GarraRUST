$apiKey = "REDACTED_LINEAR_API_KEY"

$query = '{ issues(filter: { team: { key: { eq: "GAR" } }, number: { in: [210, 211] } }) { nodes { id number title } } }'

$body = @{ query = $query } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body

$resp.data.issues.nodes | ForEach-Object {
    Write-Host ("GAR-{0}: {1}" -f $_.number, $_.id)
}
