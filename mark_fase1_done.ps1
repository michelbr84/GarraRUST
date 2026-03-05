$key = 'REDACTED_LINEAR_API_KEY'
$headers = @{ "Authorization" = $key; "Content-Type" = "application/json" }
$url = "https://api.linear.app/graphql"
$doneId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"
$teamId = "cf3ca822-b504-4638-a89c-789e3c8a7592"

function Invoke-Linear([string]$query) {
    $body = [System.Text.Encoding]::UTF8.GetBytes((@{ query = $query } | ConvertTo-Json -Compress))
    return Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $body -ContentType "application/json; charset=utf-8"
}

$ids = @("GAR-303","GAR-304","GAR-305","GAR-306","GAR-307","GAR-308","GAR-309","GAR-310")
$filter = ($ids | ForEach-Object { "`"$_`"" }) -join ","
$q = "query { team(id: `"$teamId`") { issues(filter: { number: { in: [303,304,305,306,307,308,309,310] } }) { nodes { id identifier } } } }"
$resp = Invoke-Linear $q
foreach ($issue in $resp.data.team.issues.nodes) {
    $m = "mutation { issueUpdate(id: `"$($issue.id)`", input: { stateId: `"$doneId`" }) { success } }"
    $r = Invoke-Linear $m
    Write-Host "$($issue.identifier): done=$($r.data.issueUpdate.success)" -ForegroundColor Green
}
