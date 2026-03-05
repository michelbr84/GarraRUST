$key = 'REDACTED_LINEAR_API_KEY'
$headers = @{ "Authorization" = $key; "Content-Type" = "application/json" }
$url = "https://api.linear.app/graphql"
$teamId = "cf3ca822-b504-4638-a89c-789e3c8a7592"

$q = "query { team(id: `"$teamId`") { issues(filter: { state: { type: { nin: [`"completed`",`"cancelled`"] } } }, first: 50) { nodes { identifier title state { name } priority } } } }"
$body = [System.Text.Encoding]::UTF8.GetBytes((@{ query = $q } | ConvertTo-Json -Compress))
$resp = Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $body -ContentType "application/json; charset=utf-8"

$resp.data.team.issues.nodes | Sort-Object identifier | ForEach-Object {
    $color = switch ($_.state.name) {
        "In Progress" { "Yellow" }
        "In Review"   { "Cyan" }
        "Todo"        { "White" }
        default       { "DarkGray" }
    }
    Write-Host "$($_.identifier)  [$($_.state.name)]  $($_.title)" -ForegroundColor $color
}
Write-Host "`nTotal: $($resp.data.team.issues.nodes.Count) issues abertos"
