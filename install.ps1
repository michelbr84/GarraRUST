<#
.SYNOPSIS
    GarraIA installer for Windows - https://github.com/michelbr84/GarraRUST

.DESCRIPTION
    Windows counterpart of `install.sh`. The two are kept at behavioral parity
    on purpose: when you change one, change the other.

    Downloads the published CLI binary for this platform, verifies it against
    the release's SHA256SUMS, installs it as `garraia.exe`, puts it on the user
    PATH, then chains into `garraia init` and `garraia start`.

.EXAMPLE
    irm https://garraia.org/install.ps1 | iex

    The standard one-liner. `irm | iex` evaluates the script with no arguments,
    so every parameter below takes its default.

.EXAMPLE
    & ([scriptblock]::Create((irm https://garraia.org/install.ps1))) -SkipSetup

    The only way to pass flags through the pipe - the PowerShell analogue of
    `curl ... | sh -s -- --skip-setup`.

.EXAMPLE
    $env:GARRAIA_SKIP_INIT=1; irm https://garraia.org/install.ps1 | iex

    Environment variables work in both forms and are the portable option.

.NOTES
    Mirrors (same script, auto-synced by release.yml):
      irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
      irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
      irm https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.ps1 | iex

    Environment variables (an env var set by the caller always wins over the
    matching parameter, mirroring install.sh:47):
      GARRAIA_VERSION           Pin a release tag (e.g. v0.3.4).
      GARRAIA_INSTALL_DIR       Override the install directory.
      GARRAIA_SKIP_INIT=1       Skip the auto-run of `garraia init`.
      GARRAIA_SKIP_START=1      Skip the auto-run of `garraia start`.
      GARRAIA_BOOTSTRAP_LOCAL=0 Forwarded to `garraia init` - suppresses the
                                GPU/Ollama/Qwen3 prompts. See plan 0126.
      GARRAIA_NO_PATH=1         Install without touching the user PATH.
      GARRAIA_INSTALL_PS1_LIBRARY=1
                                Test-only. Returns before Invoke-Main so the
                                functions can be dot-sourced by the Pester
                                suites in tests/install_ps1/. Mirrors
                                GARRAIA_INSTALL_SH_LIBRARY in install.sh:465.

    Requires Windows PowerShell 5.1 or later. `#Requires` is deliberately NOT
    used: it is only honored for script files and is inert under `iex`, so the
    check lives in Invoke-Main instead.
#>

param(
    [switch]$SkipSetup,
    [switch]$SkipInit,
    [switch]$SkipStart,
    [switch]$NoLocal,
    [string]$Version,
    [string]$InstallDir,
    [switch]$NoModifyPath,
    [switch]$Help
)

# NOTE: no top-level Set-StrictMode / $ErrorActionPreference / $ProgressPreference.
# Under `irm | iex` the script body runs in the *user's* live session, so setting
# them here would silently reconfigure their shell and outlive the install.
# They are set inside Invoke-Main instead, where PowerShell's scoping reverts
# them automatically when the function returns.

$script:Repo = 'michelbr84/GarraRUST'
# The installed command is `garraia`, while `cargo build` produces `garra`.
# The drift is intentional and documented in README.md; install.sh:57 makes the
# same choice via BINARY="garraia".
$script:Binary = 'garraia'
$script:UserAgent = 'garraia-install-ps1'

# Abort the install with a message.
#
# This THROWS rather than calling `exit`. Under `irm | iex` the script body runs
# in the user's live session, where `exit` terminates the host - it would close
# their terminal window on any error. The bottom of the file catches, prints,
# and only converts to a real exit code when running as a script file.
function Write-InstallError {
    param([Parameter(Mandatory)][string]$Message)
    throw $Message
}

# Printed when a GitHub fetch fails even after the retries below. On shared
# cloud hosts an HTTP 429 here usually means the egress IP - shared by many
# users - exhausted GitHub's per-IP quota, not that anything is wrong locally.
# Mirrors rate_limit_hint in install.sh:446-459.
function Write-RateLimitHint {
    Write-Host ''
    Write-Host "If the failure above was HTTP 429 (rate limit): this machine's shared"
    Write-Host "egress IP has exhausted GitHub's per-IP quota. You can:"
    Write-Host '  * retry in a few minutes, or'
    Write-Host '  * fetch the installer from the release CDN instead:'
    Write-Host "      irm https://github.com/$script:Repo/releases/latest/download/install.ps1 | iex"
    Write-Host '  * or from the jsDelivr mirror:'
    Write-Host "      irm https://cdn.jsdelivr.net/gh/$script:Repo@main/install.ps1 | iex"
}

function Show-Usage {
    @'
GarraIA installer (Windows)

Usage:
  irm https://garraia.org/install.ps1 | iex
  & ([scriptblock]::Create((irm https://garraia.org/install.ps1))) [options]

Options:
  -SkipSetup            Install only: skip both `garraia init` and `garraia start`.
  -SkipInit             Skip the interactive setup wizard.
  -SkipStart            Skip starting the gateway.
  -NoLocal              Skip the GPU/Ollama/local-model prompts in the wizard.
  -Version <tag>        Pin a release tag (e.g. v0.3.4) instead of latest.
  -InstallDir <dir>     Install to <dir> instead of %LOCALAPPDATA%\Programs\GarraIA.
  -NoModifyPath         Do not add the install directory to the user PATH.
  -Help                 Show this message.

Every option is an alias for an environment variable of the same meaning; an
env var already set by the caller wins over its flag.
'@ | Write-Host
}

# Resolve the effective options from parameters + environment.
#
# Returns a hashtable and mutates nothing, so the whole precedence table is
# unit-testable without touching the process environment. Mirrors parse_args
# (install.sh:78-114), including the rule that an env var already set by the
# caller is never overridden by a flag.
function Get-GarraiaConfig {
    param(
        [switch]$SkipSetup,
        [switch]$SkipInit,
        [switch]$SkipStart,
        [switch]$NoLocal,
        [string]$Version,
        [string]$InstallDir,
        [switch]$NoModifyPath
    )

    $effectiveVersion = $Version
    if ($env:GARRAIA_VERSION) { $effectiveVersion = $env:GARRAIA_VERSION }

    $effectiveDir = $InstallDir
    if ($env:GARRAIA_INSTALL_DIR) { $effectiveDir = $env:GARRAIA_INSTALL_DIR }

    return @{
        SkipInit     = ($env:GARRAIA_SKIP_INIT -eq '1') -or $SkipInit -or $SkipSetup
        SkipStart    = ($env:GARRAIA_SKIP_START -eq '1') -or $SkipStart -or $SkipSetup
        NoLocal      = ($env:GARRAIA_BOOTSTRAP_LOCAL -eq '0') -or $NoLocal
        NoModifyPath = ($env:GARRAIA_NO_PATH -eq '1') -or $NoModifyPath
        Version      = $effectiveVersion
        InstallDir   = $effectiveDir
    }
}

# Windows PowerShell 5.1 still negotiates TLS 1.0 by default and github.com
# refuses it. Widen the set rather than replacing it, so a host that already
# enabled TLS 1.3 keeps it. On PowerShell 7 / .NET Core ServicePointManager is
# a no-op but still present, hence the try/catch rather than a version test.
function Enable-Tls12 {
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        Write-Host 'warning: could not raise the TLS version; the download may fail.'
    }
}

# Normalize the HTTP status out of a terminating error across PowerShell
# editions: 5.1 raises WebException (StatusCode is an enum with .value__),
# 7 raises HttpRequestException with an HttpResponseMessage.
function Get-HttpStatusCode {
    param([Parameter(Mandatory)]$ErrorRecord)

    $response = $null
    if ($ErrorRecord.Exception.PSObject.Properties['Response']) {
        $response = $ErrorRecord.Exception.Response
    }
    if (-not $response) { return 0 }
    if (-not $response.PSObject.Properties['StatusCode']) { return 0 }

    try { return [int]$response.StatusCode } catch { return 0 }
}

# Every network call goes through this one function. It is the retry point
# (curl gets --retry for free at install.sh:62; Invoke-WebRequest has no
# equivalent before PowerShell 6) and it is the single seam the Pester suites
# mock, which is what keeps the whole test suite offline.
function Invoke-GhRequest {
    param(
        [Parameter(Mandatory)][string]$Uri,
        [string]$OutFile,
        [hashtable]$Headers = @{},
        [int]$MaxAttempts = 5,
        [int]$DelaySeconds = 2
    )

    $allHeaders = @{ 'User-Agent' = $script:UserAgent }
    foreach ($key in $Headers.Keys) { $allHeaders[$key] = $Headers[$key] }

    $attempt = 0
    while ($true) {
        $attempt++
        try {
            $params = @{
                Uri             = $Uri
                Headers         = $allHeaders
                UseBasicParsing = $true
                ErrorAction     = 'Stop'
            }
            if ($OutFile) { $params['OutFile'] = $OutFile }
            return Invoke-WebRequest @params
        } catch {
            $status = Get-HttpStatusCode -ErrorRecord $_

            # 0 means the request never got a response (DNS, TCP, TLS) - worth
            # retrying. 408/429/5xx are transient by definition. Everything else
            # (notably 404 on a missing asset) is terminal; retrying only makes
            # the user wait ten seconds for the same answer.
            $transient = ($status -eq 0) -or ($status -eq 408) -or ($status -eq 429) -or ($status -ge 500)
            if (-not $transient -or $attempt -ge $MaxAttempts) { throw }

            Start-Sleep -Seconds $DelaySeconds
        }
    }
}

# Map the host architecture onto the release asset name. release.yml publishes
# garraia-windows-x86_64.exe and, from v0.3.4 on, garraia-windows-aarch64.exe
# (best-effort). Mirrors detect_platform (install.sh:180).
function Get-GarraiaPlatform {
    # A 32-bit PowerShell on 64-bit Windows reports x86 in PROCESSOR_ARCHITECTURE
    # and the real architecture in PROCESSOR_ARCHITEW6432.
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if (-not $arch) { $arch = 'AMD64' }

    switch ($arch.ToUpperInvariant()) {
        'AMD64' { return "$script:Binary-windows-x86_64.exe" }
        'ARM64' {
            # Native ARM64 builds exist from v0.3.4 onward. Pinning
            # GARRAIA_VERSION to anything older on an ARM64 host 404s (same
            # documented caveat as install.sh on Apple Silicon before v0.2.1);
            # the workaround is downloading garraia-windows-x86_64.exe by hand,
            # which still runs under emulation.
            return "$script:Binary-windows-aarch64.exe"
        }
        'X86' {
            Write-InstallError '32-bit Windows is not supported. Build from source instead.'
        }
        default {
            Write-InstallError "Unsupported architecture: $arch"
        }
    }
}

# `.../releases/tag/v0.3.0[?query]` -> `v0.3.0`; anything else -> $null.
# Kept standalone so it can be unit-tested through the library guard, exactly
# like extract_tag_from_release_url (install.sh:250-252).
function Get-TagFromReleaseUrl {
    param([string]$Url)
    if (-not $Url) { return $null }
    if ($Url -match '/releases/tag/([^/?#]+)') { return $Matches[1] }
    return $null
}

# The effective URL after redirects. PowerShell 5.1 exposes it on
# BaseResponse.ResponseUri (System.Net.HttpWebResponse); PowerShell 7 moved to
# HttpClient and exposes BaseResponse.RequestMessage.RequestUri.
function Get-EffectiveUri {
    param([Parameter(Mandatory)]$Response)

    if (-not $Response.PSObject.Properties['BaseResponse']) { return $null }
    $base = $Response.BaseResponse
    if ($null -eq $base) { return $null }

    if ($base.PSObject.Properties['ResponseUri'] -and $base.ResponseUri) {
        return $base.ResponseUri.AbsoluteUri
    }
    if ($base.PSObject.Properties['RequestMessage'] -and $base.RequestMessage) {
        return $base.RequestMessage.RequestUri.AbsoluteUri
    }
    return $null
}

# Resolve the release tag to install.
#
# The preferred path mirrors install.sh:209-213: follow the web redirect of
# /releases/latest and read the tag out of the final URL. Unlike api.github.com
# (60 unauthenticated requests/hour per IP, exhausted instantly on shared
# egress IPs) the github.com web endpoint tolerates far more traffic. The REST
# API is the fallback, not the primary path.
function Resolve-GarraiaVersion {
    param([string]$PinnedVersion)

    if ($PinnedVersion) {
        Write-Host "Using pinned version: $PinnedVersion"
        return $PinnedVersion
    }

    $version = $null
    # Each tier is allowed to fail so the next one gets a turn, but the reason
    # is kept: without it a failure anywhere in here collapses into a generic
    # "failed to resolve", which is exactly what made a Windows PowerShell 5.1
    # bug in the third tier undiagnosable from the CI log.
    $lastError = $null

    try {
        $resp = Invoke-GhRequest -Uri "https://github.com/$script:Repo/releases/latest"
        $version = Get-TagFromReleaseUrl (Get-EffectiveUri $resp)
    } catch {
        $lastError = "redirect: $($_.Exception.Message)"
        $version = $null
    }

    if (-not $version) {
        try {
            $resp = Invoke-GhRequest `
                -Uri "https://api.github.com/repos/$script:Repo/releases/latest" `
                -Headers @{ 'Accept' = 'application/vnd.github+json' }
            $parsed = ConvertFrom-Json -InputObject $resp.Content
            if ($parsed) { $version = $parsed.tag_name }
        } catch {
            $lastError = "releases/latest: $($_.Exception.Message)"
            $version = $null
        }
    }

    # Last resort: the newest non-draft release. /releases/latest 404s on a
    # repository whose only releases are prereleases. This replaces the awk
    # pipeline of extract_first_non_draft_tag (install.sh:267-282).
    #
    # Assign then `foreach`, rather than piping into Where-Object and taking
    # `(...).tag_name`. Two portability traps live in that shorter form:
    # Windows PowerShell 5.1 and PowerShell 7 do not agree on whether
    # ConvertFrom-Json unrolls a JSON array onto the pipeline, and member
    # access on an empty pipeline result throws under Set-StrictMode. `foreach`
    # iterates an array and a lone object identically on both versions.
    if (-not $version) {
        try {
            $resp = Invoke-GhRequest `
                -Uri "https://api.github.com/repos/$script:Repo/releases" `
                -Headers @{ 'Accept' = 'application/vnd.github+json' }
            $releases = ConvertFrom-Json -InputObject $resp.Content
            foreach ($release in $releases) {
                if (-not $release.draft) {
                    $version = $release.tag_name
                    break
                }
            }
        } catch {
            $lastError = "releases list: $($_.Exception.Message)"
            $version = $null
        }
    }

    if (-not $version) {
        Write-RateLimitHint
        $detail = if ($lastError) { " Last error - $lastError" } else { '' }
        Write-InstallError ("Failed to resolve latest release. " +
            "Set `$env:GARRAIA_VERSION='vX.Y.Z' to pin.$detail")
    }

    Write-Host "Latest version: $version"
    return $version
}

# Select the SHA256SUMS line for $Artifact, tolerating both the two-space
# text-mode separator and the one-space + `*` binary-mode separator, plus stray
# CR line endings from Windows-generated checksum files.
#
# The end-of-line anchor is load-bearing: without it `garraia-windows-x86_64.exe`
# also matches `garraia-windows-x86_64.exe.sha256`. install.sh:341-350 documents
# the same trap after it bit a real release - do not relax this regex.
function Select-ChecksumLine {
    param(
        [Parameter(Mandatory)][string]$Artifact,
        [string[]]$Lines = @()
    )

    $pattern = '[ *]' + [regex]::Escape($Artifact) + '$'
    foreach ($line in $Lines) {
        $clean = $line -replace "`r", ''
        if ($clean -match $pattern) { return $clean }
    }
    return $null
}

# Pull the hash out of a `<hash>  <file>` / `<hash> *<file>` line, lowercased.
# Get-FileHash returns uppercase while sha256sum writes lowercase, so both
# sides of the eventual comparison are normalized here and at the call site.
function Get-ExpectedHash {
    param([Parameter(Mandatory)][string]$ChecksumLine)

    $token = ($ChecksumLine -split '\s+', 2)[0]
    if ($token -notmatch '^[0-9a-fA-F]{64}$') {
        Write-InstallError "Malformed checksum entry: $ChecksumLine"
    }
    return $token.ToLowerInvariant()
}

function Get-VerifiedArtifact {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Artifact,
        [Parameter(Mandatory)][string]$WorkDir
    )

    $baseUrl = "https://github.com/$script:Repo/releases/download/$Version"
    $artifactPath = Join-Path $WorkDir $Artifact
    $sumsPath = Join-Path $WorkDir 'SHA256SUMS'

    Write-Host "Downloading $Artifact from $Version..."
    try {
        Invoke-GhRequest -Uri "$baseUrl/$Artifact" -OutFile $artifactPath | Out-Null
    } catch {
        Write-RateLimitHint
        Write-InstallError ("Failed to download $Artifact from $baseUrl. " +
            'The release may not include this platform yet.')
    }

    Write-Host 'Downloading SHA256SUMS...'
    try {
        Invoke-GhRequest -Uri "$baseUrl/SHA256SUMS" -OutFile $sumsPath | Out-Null
    } catch {
        Write-RateLimitHint
        Write-InstallError 'Failed to download SHA256SUMS. Cannot verify binary integrity.'
    }

    Write-Host 'Verifying checksum...'
    $line = Select-ChecksumLine -Artifact $Artifact -Lines (Get-Content -Path $sumsPath)
    if (-not $line) {
        Write-InstallError "Checksum verification failed for ${Artifact}: no entry found in SHA256SUMS."
    }

    $expected = Get-ExpectedHash -ChecksumLine $line
    $actual = (Get-FileHash -Path $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($expected -ne $actual) {
        Write-InstallError ("Checksum verification failed for $Artifact " +
            "(expected $expected, got $actual).")
    }

    Write-Host 'Checksum verified.'

    # Invoke-WebRequest does not attach a mark-of-the-web, but a proxy or a
    # mirror might. Cheap insurance against SmartScreen blocking the exec.
    if (Get-Command Unblock-File -ErrorAction SilentlyContinue) {
        Unblock-File -Path $artifactPath -ErrorAction SilentlyContinue
    }

    return $artifactPath
}

# Reject install targets that belong to Windows itself. The counterpart of the
# /bin, /sbin, /usr/bin, /etc guard at install.sh:354-359.
#
# %ProgramFiles% is deliberately NOT rejected - it is a legitimate, if
# elevation-requiring, choice. Install-Binary surfaces the access error instead.
function Test-SystemPath {
    param([Parameter(Mandatory)][string]$Path)

    $normalized = ''
    try {
        $normalized = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    } catch {
        Write-InstallError "Not a usable install directory: $Path"
    }

    # A bare drive root (C:\) is never a sane install target.
    if ($normalized -match '^[A-Za-z]:$') { return $true }

    $sep = [IO.Path]::DirectorySeparatorChar
    $forbidden = @($env:SystemRoot, $env:windir) | Where-Object { $_ }
    foreach ($root in $forbidden) {
        $f = $root.TrimEnd('\', '/')
        # Compare on a separator boundary, not a raw string prefix: a sibling
        # directory such as C:\WindowsApps starts with "C:\Windows" but is a
        # perfectly legitimate install target.
        if ($normalized.Equals($f, [StringComparison]::OrdinalIgnoreCase) -or
            $normalized.StartsWith("$f$sep", [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

# Persist the install dir on the user PATH, and make it usable in the shell
# running right now.
#
# Named Register-* rather than Set-*: PSScriptAnalyzer's
# PSUseShouldProcessForStateChangingFunctions fires on the Set/New/Remove verbs,
# and the analyzer is a blocking CI gate.
#
# The registry is written directly rather than through
# [Environment]::SetEnvironmentVariable(...,'User'). On .NET Framework - which
# is what Windows PowerShell 5.1 runs on - that API rewrites the value as
# REG_SZ, permanently converting a REG_EXPAND_SZ user PATH into a literal one
# and breaking every %USERPROFILE%-style entry already in it. Writing
# ExpandString ourselves preserves them.
function Register-GarraiaPath {
    param([Parameter(Mandatory)][string]$Directory)

    $target = $Directory.TrimEnd('\')

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if (-not $key) {
        Write-Host 'warning: could not open HKCU\Environment; PATH not modified.'
        return
    }

    try {
        $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($null -eq $raw) { $raw = '' }

        $present = $raw -split ';' |
            Where-Object { $_ } |
            Where-Object { $_.Trim().TrimEnd('\') -eq $target }

        if ($present) {
            Write-Host "PATH already contains $target"
        } else {
            $updated = if ($raw.Trim()) { $raw.TrimEnd(';') + ';' + $target } else { $target }
            $key.SetValue('Path', $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString)
            Write-Host "Added $target to your user PATH."
            Write-Host '  (Terminals already open need a restart to see it.)'
        }
    } finally {
        $key.Dispose()
    }
}

function Install-Binary {
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$Version,
        [string]$RequestedDir,
        [bool]$NoModifyPath = $false
    )

    if ($RequestedDir) {
        if (Test-SystemPath -Path $RequestedDir) {
            Write-InstallError "Refusing to install into a Windows system path: $RequestedDir"
        }
        $installDir = $RequestedDir
    } else {
        # Per-user, writable without UAC - a one-liner install must never
        # require elevation. This is the Windows counterpart of install.sh
        # preferring ~/.local/bin over /usr/local/bin.
        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\GarraIA'
    }

    try {
        New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    } catch [UnauthorizedAccessException] {
        Write-InstallError ("No permission to create $installDir. Re-run from an elevated " +
            'PowerShell, or pass -InstallDir a per-user path.')
    }

    $installPath = Join-Path $installDir "$script:Binary.exe"

    try {
        Copy-Item -Path $SourcePath -Destination $installPath -Force
    } catch [IO.IOException] {
        Write-InstallError ("Could not replace $installPath - the file is in use. " +
            "Run 'garraia stop', close any running GarraIA, then try again.")
    }

    if ($NoModifyPath) {
        Write-Host "Skipping PATH registration (-NoModifyPath). Add $installDir yourself."
    } else {
        Register-GarraiaPath -Directory $installDir
    }

    # $env:Path is a snapshot taken when this process started, so the registry
    # write above is invisible to it. Without this line `garraia` is not found
    # in the very terminal that just ran the installer - which reads as a
    # failed install - and the init/start chaining below would break too.
    if (@($env:Path -split ';' | Where-Object { $_.TrimEnd('\') -eq $installDir.TrimEnd('\') }).Count -eq 0) {
        $env:Path = "$env:Path;$installDir"
    }

    Write-Host ''
    Write-Host "GarraIA $Version installed to $installPath" -ForegroundColor Green
    return $installPath
}

function Write-NextStepsLegacy {
    Write-Host ''
    Write-Host 'Next steps:'
    Write-Host '  garraia init    # interactive setup wizard'
    Write-Host '  garraia start   # start the gateway'
}

# Is there a human at a console to answer the wizard?
#
# Split out from Invoke-BootstrapPhase so the test suite can override it: the
# real probe depends on how the CI runner attaches stdio, which makes the
# bootstrap branches untestable if the check is inlined.
#
# Note: under `irm | iex` the pipeline carries objects, not stdin, so
# IsInputRedirected is false and the console is genuinely interactive. The
# wizard correctly runs, which is the intended behavior.
function Test-InteractiveSession {
    if (-not [Environment]::UserInteractive) { return $false }
    if ($env:CI) { return $false }
    try {
        if ([Console]::IsInputRedirected) { return $false }
    } catch {
        # No console attached at all (e.g. a service host) - treat as
        # non-interactive rather than letting the probe itself fail the install.
        return $false
    }
    return $true
}

# Interactive bootstrap after Install-Binary - the Windows half of plan 0127.
#
#   * both SkipInit and SkipStart -> print next steps and return.
#   * no interactive session (scheduled task, service, CI) -> print next steps
#     and return; never hang waiting for input the caller cannot give.
#   * otherwise -> `garraia init`, then `garraia start`.
#
# install.sh:428 uses `exec` for the final start so Ctrl+C reaches the gateway
# directly. PowerShell has no exec; `&` runs the child in the foreground of the
# same console, where Ctrl+C is delivered to it natively - same outcome.
function Invoke-BootstrapPhase {
    param(
        [Parameter(Mandatory)][string]$InstallPath,
        [bool]$SkipInit = $false,
        [bool]$SkipStart = $false
    )

    if ($SkipInit -and $SkipStart) {
        Write-NextStepsLegacy
        return
    }

    if (-not (Test-InteractiveSession)) {
        Write-Host ''
        Write-Host 'Non-interactive install detected - skipping wizard + start.'
        Write-NextStepsLegacy
        return
    }

    if (-not $SkipInit) {
        Write-Host ''
        Write-Host 'Running interactive setup wizard...'
        & $InstallPath init
        if ($LASTEXITCODE -ne 0) {
            Write-Host ''
            Write-Host 'Wizard exited non-zero - your config may need manual edits.'
            Write-NextStepsLegacy
            return
        }
    }

    if (-not $SkipStart) {
        Write-Host ''
        Write-Host 'Starting GarraIA in the foreground. Press Ctrl+C to stop.'
        Write-Host '  To run later in background: garraia start -d'
        Write-Host "  Either way, 'garraia status' and 'garraia stop' manage the process."
        Write-Host '  Windows may raise a Firewall prompt the first time the gateway binds.'
        & $InstallPath start
        return
    }

    Write-NextStepsLegacy
}

function Invoke-Main {
    param(
        [switch]$SkipSetup,
        [switch]$SkipInit,
        [switch]$SkipStart,
        [switch]$NoLocal,
        [string]$Version,
        [string]$InstallDir,
        [switch]$NoModifyPath,
        [switch]$Help
    )

    # Set inside the function, not at file scope: PowerShell reverts these when
    # the function returns, so an `irm | iex` run cannot leave the user's shell
    # reconfigured. $ProgressPreference matters for more than tidiness - 5.1's
    # progress renderer makes a 45 MB Invoke-WebRequest roughly an order of
    # magnitude slower.
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    if ($Help) {
        Show-Usage
        return
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        Write-InstallError ('Windows PowerShell 5.1 or later is required (found ' +
            "$($PSVersionTable.PSVersion)). Upgrade, or download the binary manually from " +
            "https://github.com/$script:Repo/releases/latest")
    }

    $opts = Get-GarraiaConfig -SkipSetup:$SkipSetup -SkipInit:$SkipInit `
        -SkipStart:$SkipStart -NoLocal:$NoLocal -Version $Version `
        -InstallDir $InstallDir -NoModifyPath:$NoModifyPath

    # `garraia init` reads this from the environment (plan 0126), so unlike the
    # other options it has to be exported rather than passed along.
    if ($opts.NoLocal) { $env:GARRAIA_BOOTSTRAP_LOCAL = '0' }

    Enable-Tls12

    Write-Host 'GarraIA installer (Windows)'
    $artifact = Get-GarraiaPlatform
    $resolved = Resolve-GarraiaVersion -PinnedVersion $opts.Version

    $workDir = Join-Path ([IO.Path]::GetTempPath()) ('garraia-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $workDir | Out-Null
    try {
        $downloaded = Get-VerifiedArtifact -Version $resolved -Artifact $artifact -WorkDir $workDir
        $installPath = Install-Binary -SourcePath $downloaded -Version $resolved `
            -RequestedDir $opts.InstallDir -NoModifyPath $opts.NoModifyPath
    } finally {
        # The analogue of `trap ... EXIT INT TERM` at install.sh:288.
        Remove-Item -Path $workDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Invoke-BootstrapPhase -InstallPath $installPath `
        -SkipInit $opts.SkipInit -SkipStart $opts.SkipStart
}

# Library mode: when dot-sourced by the Pester suites with
# GARRAIA_INSTALL_PS1_LIBRARY=1, return before Invoke-Main so each function can
# be exercised in isolation. Mirrors install.sh:464-468.
if ($env:GARRAIA_INSTALL_PS1_LIBRARY -eq '1') { return }

try {
    Invoke-Main -SkipSetup:$SkipSetup -SkipInit:$SkipInit -SkipStart:$SkipStart `
        -NoLocal:$NoLocal -Version $Version -InstallDir $InstallDir `
        -NoModifyPath:$NoModifyPath -Help:$Help
} catch {
    Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red

    # MyCommand.Path is populated only when running as a real script file
    # (powershell -File install.ps1). Under `irm | iex` it is empty, and calling
    # `exit` there would terminate the user's session instead of the install.
    if ($MyInvocation.MyCommand.Path) { exit 1 }
    $global:LASTEXITCODE = 1
}
