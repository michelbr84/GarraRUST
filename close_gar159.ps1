$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

$mutation = @"
mutation {
  issueUpdate(id: "16a35a4e-ba68-4aae-b561-a25d1af89de9", input: { stateId: "$doneStateId" }) {
    success
  }
}
"@
$body = @{ query = $mutation } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body
if ($resp.data.issueUpdate.success) { Write-Host "Closed GAR-159 as Done" }
else { Write-Host "Failed: $($resp | ConvertTo-Json)" }
