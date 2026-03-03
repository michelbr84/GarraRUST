$apiKey = "REDACTED_LINEAR_API_KEY"
$q = '{ issues(filter: { team: { key: { eq: "GAR" } }, number: { in: [214, 215] } }) { nodes { id number title description } } }'
$body = @{ query = $q } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body
$resp.data.issues.nodes | ForEach-Object {
    Write-Host ("=== GAR-{0}: {1} ===" -f $_.number, $_.title)
    Write-Host $_.description
    Write-Host ""
}
