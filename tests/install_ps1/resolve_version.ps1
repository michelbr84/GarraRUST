# Unit tests for Resolve-GarraiaVersion and its helpers in install.ps1
# (the PowerShell counterpart of tests/install_sh/resolve_version.sh).
#
# Version resolution is a three-tier ladder, and the ORDER is the point.
# api.github.com allows 60 unauthenticated requests/hour per IP, which a shared
# cloud egress IP exhausts immediately; the github.com web redirect does not.
# So the redirect is the primary path and the API is the fallback, exactly as in
# install.sh:203-249. A regression that reverses them still passes a naive
# "does it return a tag" test while breaking every install on a busy host.
#
# Every network call funnels through Invoke-GhRequest, so overriding that one
# function makes the whole suite offline and deterministic.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

# --- Mock plumbing -----------------------------------------------------------

$script:Calls = @()
$script:Responses = @{}

function New-RedirectResponse {
    # Windows PowerShell 5.1 shape: BaseResponse is an HttpWebResponse whose
    # ResponseUri is the post-redirect URL.
    param([string]$FinalUrl)
    return [pscustomobject]@{ BaseResponse = [pscustomobject]@{ ResponseUri = [uri]$FinalUrl } }
}

function New-CoreRedirectResponse {
    # PowerShell 7 shape: BaseResponse is an HttpResponseMessage; the final URL
    # lives on RequestMessage.RequestUri.
    param([string]$FinalUrl)
    return [pscustomobject]@{
        BaseResponse = [pscustomobject]@{
            RequestMessage = [pscustomobject]@{ RequestUri = [uri]$FinalUrl }
        }
    }
}

function New-JsonResponse {
    param([string]$Json)
    return [pscustomobject]@{ Content = $Json }
}

# Replaces install.ps1's Invoke-GhRequest for the rest of this suite. Function
# lookup is by name at call time, so Resolve-GarraiaVersion picks this up.
function Invoke-GhRequest {
    param(
        [Parameter(Mandatory)][string]$Uri,
        [string]$OutFile,
        [hashtable]$Headers = @{},
        [int]$MaxAttempts = 5,
        [int]$DelaySeconds = 2
    )
    $script:Calls += $Uri
    foreach ($key in $script:Responses.Keys) {
        if ($Uri -like $key) {
            $value = $script:Responses[$key]
            if ($value -is [string] -and $value -eq 'THROW') { throw "mock failure for $Uri" }
            return $value
        }
    }
    throw "unmocked request: $Uri"
}

function Reset-Mock {
    $script:Calls = @()
    $script:Responses = @{}
}

# --- Get-TagFromReleaseUrl ---------------------------------------------------

Write-Host 'Get-TagFromReleaseUrl'
Assert-Equal 'plain tag URL' 'v0.3.3' `
    (Get-TagFromReleaseUrl 'https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.3')
Assert-Equal 'tag URL with query' 'v0.3.3' `
    (Get-TagFromReleaseUrl 'https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.3?foo=1')
Assert-Equal 'tag URL with fragment' 'v0.2.0-beta' `
    (Get-TagFromReleaseUrl 'https://github.com/michelbr84/GarraRUST/releases/tag/v0.2.0-beta#notes')
Assert-Equal 'non-tag URL yields nothing' '' `
    (Get-TagFromReleaseUrl 'https://github.com/michelbr84/GarraRUST/releases')
Assert-Equal 'empty input yields nothing' '' (Get-TagFromReleaseUrl '')
Assert-Equal 'null input yields nothing'  '' (Get-TagFromReleaseUrl $null)

# --- Get-EffectiveUri --------------------------------------------------------

Write-Host ''
Write-Host 'Get-EffectiveUri: both PowerShell editions'
Assert-Equal 'PS 5.1 shape (BaseResponse.ResponseUri)' 'https://example.test/releases/tag/v1.2.3' `
    (Get-EffectiveUri (New-RedirectResponse 'https://example.test/releases/tag/v1.2.3'))
Assert-Equal 'PS 7 shape (BaseResponse.RequestMessage.RequestUri)' 'https://example.test/releases/tag/v1.2.3' `
    (Get-EffectiveUri (New-CoreRedirectResponse 'https://example.test/releases/tag/v1.2.3'))
Assert-Equal 'response without BaseResponse' '' `
    (Get-EffectiveUri ([pscustomobject]@{ BaseResponse = $null }))

# --- Resolve-GarraiaVersion --------------------------------------------------

Write-Host ''
Write-Host 'Resolve-GarraiaVersion: pinned version short-circuits'
Reset-Mock
Assert-Equal 'pinned tag returned as-is' 'v0.1.2' (Resolve-GarraiaVersion -PinnedVersion 'v0.1.2')
Assert-Equal 'pinned tag makes no network call' 0 $script:Calls.Count

Write-Host ''
Write-Host 'Resolve-GarraiaVersion: redirect is the primary path'
Reset-Mock
$script:Responses['https://github.com/*/releases/latest'] =
    New-RedirectResponse 'https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.3'
Assert-Equal 'tag read from the redirect' 'v0.3.3' (Resolve-GarraiaVersion)
Assert-Equal 'exactly one request made' 1 $script:Calls.Count
Assert-False 'api.github.com never contacted' ($script:Calls -match 'api\.github\.com').Count

Write-Host ''
Write-Host 'Resolve-GarraiaVersion: falls back to the REST API'
Reset-Mock
$script:Responses['https://github.com/*/releases/latest'] = 'THROW'
$script:Responses['https://api.github.com/*/releases/latest'] = New-JsonResponse '{"tag_name":"v0.3.2"}'
Assert-Equal 'tag read from /releases/latest' 'v0.3.2' (Resolve-GarraiaVersion)
Assert-True  'the API was reached only after the redirect failed' ($script:Calls.Count -eq 2)

Write-Host ''
Write-Host 'Resolve-GarraiaVersion: falls back to the newest non-draft release'
Reset-Mock
$script:Responses['https://github.com/*/releases/latest'] = 'THROW'
$script:Responses['https://api.github.com/*/releases/latest'] = 'THROW'
$script:Responses['https://api.github.com/*/releases'] = New-JsonResponse @'
[{"draft":true,"tag_name":"v0.4.0-wip"},{"draft":false,"tag_name":"v0.3.1"},{"draft":false,"tag_name":"v0.3.0"}]
'@
Assert-Equal 'draft releases are skipped' 'v0.3.1' (Resolve-GarraiaVersion)

Write-Host ''
Write-Host 'Resolve-GarraiaVersion: every channel down'
Reset-Mock
$script:Responses['https://github.com/*'] = 'THROW'
$script:Responses['https://api.github.com/*'] = 'THROW'
Assert-Throws 'throws rather than installing an unknown version' { Resolve-GarraiaVersion }

Exit-WithSummary 'resolve_version'
