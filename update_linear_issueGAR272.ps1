# Load from .env if exists
$envPath = ".env"
if (Test-Path $envPath) {
    Get-Content $envPath | ForEach-Object {
        if ($_ -match "^LINEAR_API_KEY=(.+)$") {
            $env:LINEAR_API_KEY = $matches[1].Trim()
        }
    }
}

$apiKey = $env:LINEAR_API_KEY
if (-not $apiKey) {
    Write-Host "LINEAR_API_KEY not found" -ForegroundColor Red
    exit 1
}

$issueId = "672aaeb6-dc63-45be-99dc-5df6ab29651d"  # GAR-272 internal ID

$description = @"
## Description
The gateway panics on startup with the error: \`Path segments must not start with ':'\`

## Root Cause
The router uses old-style axum capture syntax \`/:id\` instead of \`/{id}\` which is invalid in axum v0.7+

## Fix Applied
Changed line 90 in \`crates/garraia-gateway/src/router.rs\`:
- BEFORE: \`.route(\"/api/modes/custom/:id\", ...)\`
- AFTER: \`.route(\"/api/modes/custom/{id}\", ...)\`

## Prevention
Added new test file \`crates/garraia-gateway/tests/router_smoke_test.rs\` with 3 tests:
1. \`router_build_does_not_panic\` - Basic router build
2. \`router_build_with_voice_does_not_panic\` - With voice config
3. \`no_legacy_route_syntax_in_router\` - Compile-time documentation

Also verified no legacy \`/:\` or \`/*\` patterns exist in any router files.

## Acceptance Criteria (completed)
- [x] \`cargo run --bin garraia -- start\` does not panic
- [x] \`cargo run --bin garraia -- start --with-voice\` does not panic  
- [x] \`cargo test -p garraia-gateway\` passes (including router_smoke tests)
- [x] No legacy \`/:\` or \`/*\` routes found in codebase

## Impact
- Fixed auth tests that were failing
- Gateway now starts properly in production

## Status
**FIXED & VERIFIED**
"@

$query = @"
mutation {
    issueUpdate(id: "$issueId", input: {
        description: $(($description -replace '"', '\"') | ConvertTo-Json -Compress)
    }) {
        success
    }
}
"@

try {
    $body = @{ query = $query } | ConvertTo-Json -Compress
    $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } -Body $body
    
    if ($response.errors) {
        Write-Host "GraphQL Error: $($response.errors[0].message)" -ForegroundColor Red
    } elseif ($response.data.issueUpdate.success) {
        Write-Host "SUCCESS: Updated issue GAR-272 with acceptance criteria" -ForegroundColor Green
    } else {
        Write-Host "Failed to update issue" -ForegroundColor Red
    }
} catch {
    Write-Host "Error: $_" -ForegroundColor Red
}
