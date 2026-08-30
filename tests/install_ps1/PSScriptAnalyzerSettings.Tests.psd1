# PSScriptAnalyzer configuration for the test suites themselves.
#
# The suites are test doubles and assertion helpers, not cmdlets anyone imports,
# so a handful of style rules written for shipping modules do not apply. Each
# exclusion is listed with why, and `install.ps1` itself is still held to the
# stricter PSScriptAnalyzerSettings.psd1 next door.
@{
    # The suites run on Windows PowerShell 5.1 as well, so they are held to the
    # same compatibility bar as the shipping scripts.
    Rules = @{
        PSUseCompatibleSyntax = @{
            Enable         = $true
            TargetVersions = @('5.1', '7.0')
        }
        PSUseCompatibleCommands = @{
            Enable = $true
            TargetProfiles = @(
                'win-8_x64_10.0.17763.0_5.1.17763.316_x64_4.0.30319.42000_framework'
            )
        }
    }

    ExcludeRules = @(
        # Same reason as the installer: assertion output is the point.
        'PSAvoidUsingWriteHost',

        # The mocks must match the signature of the function they replace
        # (Invoke-GhRequest), including the parameters this particular test
        # never exercises. Trimming them would make the double diverge from the
        # real contract, which is exactly the bug a mock is supposed to avoid.
        'PSReviewUnusedParameter',

        # Fires on New-/Reset- verbs. These build in-memory fixtures and reset
        # local test state -- there is nothing for -WhatIf to protect.
        'PSUseShouldProcessForStateChangingFunctions',

        # Get-StubInvocations and Assert-Throws genuinely return/describe
        # plurals. Renaming them to singular would make the tests read worse.
        'PSUseSingularNouns',

        # Assert-Equal and friends are called positionally on purpose: the
        # assertions read as prose that way, and they are local to this folder.
        'PSAvoidUsingPositionalParameters'
    )
}
