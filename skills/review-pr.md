Review the GitHub PR using the code-reviewer and security-auditor agents.

Steps:
1. Run `gh pr view $ARGUMENTS --json number,title,body,files` to load PR metadata
2. Run `gh pr diff $ARGUMENTS` to get the full diff
3. Identify changed crates (Rust) and packages (Flutter) from the diff
4. Use the **code-reviewer** agent to evaluate correctness, architecture, and style
5. If diff touches `mobile_auth.rs`, `credentials.rs`, JWT handling, or DB queries — also use the **security-auditor** agent
6. Compile findings into a single structured comment
7. Post the review: `gh pr review $ARGUMENTS --comment --body "<review>"`
8. Report: PR number, veredicto, total findings by severity

Usage: /review-pr --pr <number> [--repo <owner/repo>]
