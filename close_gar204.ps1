$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

# GAR-204: History unification
$mutation = @"
mutation {
  issueUpdate(id: "cdfcb5c2-dffd-4e47-8914-fd09e00e68b7", input: { stateId: "$doneStateId" }) {
    success
  }
}
"@
$body = @{ query = $mutation } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body
if ($resp.data.issueUpdate.success) { Write-Host "Closed GAR-204 as Done" }
else { Write-Host "Failed: $($resp | ConvertTo-Json)" }
