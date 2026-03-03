$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

$q = '{ issues(filter: { team: { key: { eq: "GAR" } }, number: { in: [214, 215] } }) { nodes { id number } } }'
$body = @{ query = $q } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body

foreach ($issue in $resp.data.issues.nodes) {
    $mutation = "mutation { issueUpdate(id: `"$($issue.id)`", input: { stateId: `"$doneStateId`" }) { success } }"
    $b2 = @{ query = $mutation } | ConvertTo-Json -Compress
    $r2 = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
        -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
        -Body $b2
    if ($r2.data.issueUpdate.success) { Write-Host "Closed GAR-$($issue.number) as Done" }
    else { Write-Host "Failed GAR-$($issue.number)" }
}
