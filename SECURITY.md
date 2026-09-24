# Security Policy

## Supported Versions

LazyPock is in beta. Security fixes are applied to the latest released
minor version on the default branch.

| Version | Supported          |
| ------- | ------------------ |
| 0.12.x  | :white_check_mark: |
| < 0.12  | :x:                |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for a security problem.

Report privately via GitHub's
[security advisories](https://github.com/gnuzd/lazypock/security/advisories/new)
("Report a vulnerability"), or email the maintainer listed on the
[GitHub profile](https://github.com/gnuzd).

Please include:

- the affected version (or commit),
- a minimal reproduction (collection rules, request, and observed result),
- the impact you believe it has (data exposure, privilege escalation, DoS).

You can expect an acknowledgement within a few days. Once a fix is released we
will credit you in the changelog unless you prefer to stay anonymous.

## Scope notes

- **Access-control rules are the security boundary.** `listRule`, `viewRule`,
  `createRule`, `updateRule`, `deleteRule` and `manageRule` decide who can read
  and write every record. Their behaviour — including the fail-closed
  guarantees for rules that cannot be evaluated — is covered by a dedicated test
  suite and CI workflow; see
  [README → What the rule-engine tests cover](README.md#what-the-rule-engine-tests-cover)
  and [`TEST_COVERAGE_AUDIT.md`](TEST_COVERAGE_AUDIT.md).
- **Realtime subscriptions are authorized once, at join time.** A subscriber
  that passes `listRule` receives every record change for the collection; there
  is no per-record rule evaluation on delivery. This matches PocketBase.
- **CORS / WebSocket origin checking** is configured separately via the server
  settings; see the CORS notes in the documentation.
- **File storage** has its own URL-based surface and is tracked separately in
  `TEST_COVERAGE_AUDIT.md`.
- **Hook code runs with full server privileges.** Only load hooks you trust; see
  [`HOOKS_SECURITY.md`](HOOKS_SECURITY.md).
