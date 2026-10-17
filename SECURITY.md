# Security Policy

## Supported Versions

LazyPock is in beta. Security fixes are applied to the latest released
minor version on the default branch.

| Version | Supported          |
| ------- | ------------------ |
| 0.23.x  | :white_check_mark: |
| < 0.23  | :x:                |

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

Known gaps and the order they are being worked through are in
[`SECURITY_ROADMAP.md`](SECURITY_ROADMAP.md), with the code each finding was
verified against.

## Scope notes

- **Access-control rules are the security boundary.** `listRule`, `viewRule`,
  `createRule`, `updateRule`, `deleteRule` and `manageRule` decide who can read
  and write every record. They are compiled to Postgres policies
  (`crates/db/src/rls/`), so the database — not the request handler — refuses a
  write. The fail-closed behaviour for rules that cannot be evaluated is
  covered by [`crates/db/tests/rls.rs`](crates/db/tests/rls.rs) and the rule
  cases in [`crates/api/tests/records.rs`](crates/api/tests/records.rs).
- **Row-level security is required, not optional.** At boot the server probes
  that it can switch to `lazypock_anon` and `lazypock_authenticated` and
  refuses to start if it cannot (`LAZYPOCK_REQUIRE_RLS`, on by default): a
  server that cannot switch role would answer 500 to every end-user request
  while the Studio kept working, which is the worst possible failure mode.
- **Realtime delivery is authorized per event.** A subscriber is checked when it joins
  *and* every event is filtered through the collection's `SELECT` policy under that
  subscriber's own context before it is sent, so a record the caller could not read is not
  delivered even to a subscription it holds. A `DELETE` cannot be re-checked — the row is
  gone — so it is delivered only to a subscriber that was sent that record before. This is
  stronger than PocketBase, which evaluates rules once at join time; an earlier version of
  this note said the opposite, and the code was right.
- **CORS / WebSocket origin checking** is configured separately via the server
  settings; see the CORS notes in the documentation.
- **File storage** has its own surface, with its own rules
  (`files.rules.listRule` / `deleteRule`) and — unlike records — a serving path
  that is public: a file id is an unguessable, non-expiring capability URL.
  A caller may only attach a file they uploaded, and only to a record they may
  write. See [the security roadmap](SECURITY_ROADMAP.md) for the open decision
  on file-read authorization.
- **No default credentials.** There is no built-in superuser and no default
  password. The first superuser is created either by the Studio's first-run
  Setup (which works only while no superuser exists) or by
  `lazypock superuser create --email … --password …`
  (`LAZYPOCK_SUPERUSER_EMAIL` / `LAZYPOCK_SUPERUSER_PASSWORD`).
- **Hook code runs with full server privileges.** Only load hooks you trust:
  a hook file is executed as the server itself, with the storage, database and
  environment it has.
