# PSScriptAnalyzer configuration for the test suites themselves.
#
# The suites are test doubles and assertion helpers, not cmdlets anyone imports,
# so a handful of style rules written for shipping modules do not apply. Each
# exclusion is listed with why, and `install.ps1` itself is still held to the
# stricter PSScriptAnalyzerSettings.psd1 next door.
@{
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
