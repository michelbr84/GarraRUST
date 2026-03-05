$key = 'REDACTED_LINEAR_API_KEY'
$headers = @{ "Authorization" = $key; "Content-Type" = "application/json" }
$url = "https://api.linear.app/graphql"
$teamId = "cf3ca822-b504-4638-a89c-789e3c8a7592"

$nums = "303,304,305,306,307,308,309,310,311,312,313,314,315,316,317,318,319,320,321,322,323,324,325,326,327,328,329"
$q = "query { team(id: `"$teamId`") { issues(filter: { number: { in: [$nums] } }) { nodes { identifier number title state { name } } } } }"
$body = [System.Text.Encoding]::UTF8.GetBytes((@{ query = $q } | ConvertTo-Json -Compress))
$resp = Invoke-RestMethod -Uri $url -Method POST -Headers $headers -Body $body -ContentType "application/json; charset=utf-8"

$resp.data.team.issues.nodes | Sort-Object number | ForEach-Object {
    $state = $_.state.name
    $color = if ($state -eq "Done") { "Green" } elseif ($state -eq "In Progress") { "Yellow" } else { "DarkGray" }
    Write-Host ("[$($_.identifier)] " + $state.PadRight(13) + $_.title) -ForegroundColor $color
}
