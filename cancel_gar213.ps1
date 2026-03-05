$key = 'REDACTED_LINEAR_API_KEY'
$headers = @{ "Authorization" = $key; "Content-Type" = "application/json" }
$url = "https://api.linear.app/graphql"
$teamId = "cf3ca822-b504-4638-a89c-789e3c8a7592"
$cancelId = "601f758f-344a-4968-b3d3-72a60ca3c881"

$q = "query { team(id: `"$teamId`") { issues(filter: { number: { eq: 213 } }) { nodes { id identifier } } } }"
$body = [System.Text.Encoding]::UTF8.GetBytes((@{ query = $q } | ConvertTo-Json -Compress))
$resp = Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $body -ContentType "application/json; charset=utf-8"
$issue = $resp.data.team.issues.nodes[0]

$m = "mutation { issueUpdate(id: `"$($issue.id)`", input: { stateId: `"$cancelId`" }) { success } }"
$b = [System.Text.Encoding]::UTF8.GetBytes((@{ query = $m } | ConvertTo-Json -Compress))
$r = Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $b -ContentType "application/json; charset=utf-8"
Write-Host "$($issue.identifier): cancelled=$($r.data.issueUpdate.success)" -ForegroundColor Green
