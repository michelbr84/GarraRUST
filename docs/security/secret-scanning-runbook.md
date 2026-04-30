# Secret Scanning Runbook

> Status: established 2026-04-30 as part of the Green Security Baseline
> sprint (plan `personal-api-key-revogada-vectorized-matsumoto`,
> Linear umbrella GAR-XXX).
> Scope: how GarraRUST detects, prevents, mitigates, and documents secret
> leaks across the working tree, commits, and historical patches.

## Layered defenses

GarraRUST relies on four overlapping checks — failure in any one gets
caught by the others:

| Layer | Where | Tool | What it catches |
|---|---|---|---|
| **L1** | Local working tree, before commit | `pre-commit` + `gitleaks` (`.pre-commit-config.yaml`) | New secrets a developer is about to commit |
| **L2** | Local working tree, anytime | `gitleaks detect --no-git --redact --source .` | Currently-staged secrets in working tree files |
| **L3** | GitHub Actions (every push/PR) | `gitleaks-action@v2` (job `Secret Scan (gitleaks)` in `.github/workflows/ci.yml`) | Secrets in PR diffs that escaped local hooks |
| **L4** | GitHub-native (continuous) | Secret scanning + push protection (`secret_scanning_push_protection: enabled` per repo `security_and_analysis`) | Token-shaped strings in pushed commits, with provider-side validation when supported |

## Configuration files (repo root)

- **`.gitleaks.toml`** — extends gitleaks default rules with an allow-list
  for known-safe paths and placeholder strings (test fixtures, example
  env files, doc snippets). See file header for line-by-line rationale.
- **`.gitleaksignore`** — lists fingerprints of historical findings that
  have been revoked upstream and are documented here. Each entry has a
  comment with the revocation date, the GitHub alert number (if any), and
  why it stays in history.
- **`.pre-commit-config.yaml`** — pinned `gitleaks` hook version. Match
  the `rev:` to the gitleaks-action version used in CI when bumping.

## Policy: do we rewrite history when a secret leaks?

**No, by default.** Once a credential is revoked upstream, the only
remaining harm is scanner noise. We optimize for operational stability:

| Trade-off | Why we choose "no rewrite" |
|---|---|
| Force-push clobbers all clones, forks, and open PR refs | High operational cost |
| `git-filter-repo` requires re-running on every fork that ever cloned us | Expensive coordination |
| Any subsequent leak in an old patch puts us right back where we started | Doesn't actually prevent recurrence |
| Token revocation has already neutralized the risk | The actual security posture is already restored |

Exceptions where we **would** rewrite history:

- Token has not been revoked AND cannot be revoked upstream (rare).
- Material is regulatory-sensitive (PII/PHI) and legal counsel requires
  it gone from objects, not just from view.
- Pre-public release: we have not pushed the repo yet, no clones exist.

For all other cases, the canonical mitigation is the 5-step procedure
below.

## Standard mitigation procedure (revoked-secret playbook)

When a secret-scanning alert fires (or you discover one yourself):

1. **Revoke the credential upstream** *immediately*. The alert is the
   trigger; revocation is the action. Do this BEFORE filing tickets,
   pushing fixes, or messaging the team. For Linear PATs:
   `https://linear.app/settings/api`. Anthropic, OpenAI, AWS — same
   pattern: provider settings page first.
2. **Confirm revocation** by attempting an authenticated request with
   the old token. If it returns 401/403, you're done with step 1.
3. **Remove the live token from working tree** (`rm` the file or
   replace the string with an env-var lookup). Stage the change.
4. **Capture the gitleaks fingerprint** for historical commits where
   the token still lives in patches:
   ```bash
   gitleaks detect --redact --report-format json \
     --report-path /tmp/leaks.json --source .
   ```
   Each finding has a `Fingerprint: <commit>:<file>:<rule>:<line>` —
   copy each into `.gitleaksignore` with a `#`-comment block above
   that includes:
   - revocation date,
   - GitHub alert number (if applicable),
   - why this stays in history (link to this runbook).
5. **Open a PR with**: `.gitleaksignore` updates + working-tree fix +
   any pre-commit/runbook adjustments. Do NOT include the token in
   the PR body, commit messages, or comments.
6. **After PR merge**, close the GitHub alert as `revoked`:
   ```bash
   gh api -X PATCH \
     repos/michelbr84/GarraRUST/secret-scanning/alerts/<NUMBER> \
     -f state=resolved -f resolution=revoked \
     -f resolution_comment="Revoked upstream YYYY-MM-DD; fingerprint in .gitleaksignore."
   ```

## Local developer setup (one-time)

```bash
# Install gitleaks (Windows: choco install gitleaks  /  macOS: brew install gitleaks)
# Or grab a release directly: https://github.com/gitleaks/gitleaks/releases

# Wire up pre-commit
pip install pre-commit
pre-commit install

# Verify
pre-commit run --all-files
```

If `pre-commit` is missing, `gitleaks detect --no-git` still works as a
manual check before pushing. Match the version pinned in
`.pre-commit-config.yaml` to keep behavior identical to CI.

## When CI gitleaks fails on a PR

If `Secret Scan (gitleaks)` fails on a PR you authored:

1. Pull the report from the failing job logs (job's "Run gitleaks" step).
2. Determine: is it a real new secret OR a historical revoked one
   surfacing for the first time?
3. **Real new secret**: revoke upstream, follow the 5-step procedure,
   re-push.
4. **Historical revoked**: only allowed if you can show the token is
   already revoked. Add the fingerprint to `.gitleaksignore` with the
   comment block, link the alert number, and re-push.

Never `--no-verify` past the gate. Never edit `.gitleaks.toml` to
allow-list a real secret.

## Open improvements (tracked separately)

- Enable `secret_scanning_validity_checks` (currently `disabled`) once
  GitHub stabilizes provider validation for our active provider mix.
- Enable `secret_scanning_non_provider_patterns` (currently `disabled`)
  to catch high-entropy strings outside known provider formats. Trade-off:
  more false positives. Worth a follow-up sub-issue under the umbrella.
- Consider GitHub Advanced Security custom patterns for project-internal
  token formats (e.g., `GARRAIA_*` env vars) once we're past the green
  baseline. See `docs/security/threat-model.md` for the broader context.

## See also

- `.gitleaks.toml` — extension rules + allow-list.
- `.gitleaksignore` — fingerprint exemptions with rationale.
- `.pre-commit-config.yaml` — local hook pinning.
- `.github/workflows/ci.yml` job `Secret Scan (gitleaks)` (~L101–119).
- `docs/security/threat-model.md` — overall security model.
- `docs/security/codeql-setup.md` *(coming in PR C)* — companion CodeQL
  configuration runbook.
