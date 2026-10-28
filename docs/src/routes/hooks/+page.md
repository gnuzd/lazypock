# TypeScript hooks

A hook is a TypeScript file in `LAZYPOCK_HOOKS_DIR` (default `hooks/`) that runs around a
record write. The server strips the types with `oxc`, loads the result into QuickJS, and
calls it — no build step, no restart, no npm.

```ts
// hooks/main.pb.ts
onRecordBeforeCreate("posts", (e) => {
  e.record.slug = e.record.title.toLowerCase().replaceAll(" ", "-");
});

onRecordAfterCreate("orders", (e) => {
  $app.save("audit", { note: `order ${e.record.id} created` });
  $http.post("https://hooks.example.com/orders", { id: e.record.id });
});
```

Files are re-read when their contents change; a file that fails to compile is reported and
the **previous version keeps serving**, so an edit in progress never takes an installation
down. With no hook files the runtime is never created at all.

## Events

| Hook | When | Can refuse the write |
| --- | --- | --- |
| `onRecordBeforeCreate` | before validation, inside the write's transaction | yes (throw) |
| `onRecordBeforeUpdate` | before validation, inside the transaction | yes |
| `onRecordBeforeDelete` | inside the transaction, with the row about to go | yes |
| `onRecordAfterCreate` | after the commit | no (the write is durable) |
| `onRecordAfterUpdate` | after the commit | no |
| `onRecordAfterDelete` | after the commit | no |

A before-hook runs *before* validation, so it can supply a field the caller did not send;
whatever it produces is then validated like any other input. `e.record` may be mutated in
place or replaced by the return value — both are honoured.

Hooks run for **every** caller, not only superusers: an anonymous write fires the same
hooks a superuser's write does. What changes is the context they run in — see `$app`
below.

Batch requests (`POST /api/batch`) run the before-hooks for each step inside the batch
transaction, including a before-delete hook (which can refuse the step and roll the whole
batch back), and run the after-hooks once the batch has committed.

## The sandbox

| Limit | Value |
| --- | --- |
| Heap per context | 32 MB |
| Stack | 512 KB |
| Execution deadline | 2 s per dispatch |
| Concurrency | one dispatch at a time (the runtime owns a thread) |
| Async | refused: a handler that returns a promise is an error, not silently skipped |

A hook that throws fails the request; a before-hook's throw also rolls the write back. A
hook that loops forever or allocates past its limit is interrupted and reported, and the
runtime is rebuilt rather than reused.

There is **no** `fs`, `process`, `env`, `require` or Node API: the only way out of the
sandbox is `$http`, and the only way into the database is `$app`.

## `$http` — outbound requests

Off by default. Nothing is reachable until an operator names hosts in
`LAZYPOCK_HOOK_HTTP_ALLOWLIST`:

```bash
# exact hosts, wildcard subdomains, and explicit ports
LAZYPOCK_HOOK_HTTP_ALLOWLIST=api.example.com,*.hooks.example.com,internal.example.com:8443
```

`example.com` grants **that host on port 443 only**; another port on the same host is a
different grant (`example.com:8443`). `*.example.com` matches subdomains but **not** the
apex. A scheme, a path, credentials or an IP literal in an entry is refused at boot, so a
typo cannot be interpreted as something wider than it looks.

```ts
const res = $http.get("https://api.example.com/thing");
res.status;              // 200
res.ok;                  // 200-299
res.headers["content-type"];
res.body;                // as text
res.text(); res.json();  // json() throws if the body is not JSON

$http.post("https://api.example.com/orders", { id: e.record.id });        // object → JSON body
$http.post("https://api.example.com/orders", "raw body", { headers });    // string as-is
$http.send({ method: "PUT", url, headers: {}, body, timeoutMs: 500 });
```

Every call is checked, in this order:

1. **HTTPS only.** A plaintext URL is refused, so a request (and whatever it carries) is
   never on the wire in the clear.
2. **No credentials in the URL.** Send an `Authorization` header instead; a URL ends up in
   logs.
3. **The host and port must be allowlisted**, exactly as configured.
4. **The resolved address must be public.** The hostname is resolved once, *every* answer is
   checked against loopback, private, link-local (including `169.254.169.254`), CGNAT,
   benchmarking, documentation, multicast and reserved blocks — IPv4, IPv6, IPv4-mapped,
   NAT64 and 6to4 — and the connection is **pinned to the address that was checked**, so a
   second DNS answer cannot redirect it. One private answer among several refuses the call.
5. **The method** must be `GET`, `POST`, `PUT`, `PATCH`, `DELETE` or `HEAD`.
6. **Headers** are limited to 32, refused if they are `Host`, `Content-Length`,
   `Transfer-Encoding`, `Connection`, `Upgrade` or a proxy header, and refused if they carry
   a control character (header injection).
7. **A body** is only accepted on `POST`/`PUT`/`PATCH`, and at most 256 KiB.
8. **The timeout** is 1 s by default, and whatever the hook asks for is clamped to 50 ms –
   2 s, so a call can never outlive the dispatch.

Response: at most 1 MiB of body and 16 KiB of headers; past either, the call fails rather
than handing a hook a payload it cannot see the end of.

**Redirects are not followed.** A 3xx comes back as a response; following it is the hook's
decision, and the next hop is checked again. **No proxy is used** — `HTTP_PROXY` in the
environment is ignored, because a proxy would route around the address check. **Nothing
from the triggering request is forwarded**: no cookies, no `Authorization`, no client
headers. What the hook sets is what is sent.

Error messages carry the URL with the query string removed: a query is where a hook puts a
token, and a hook failure is logged.

> **Limits worth knowing.** DNS resolution happens through the system resolver and is not
> covered by the per-call timeout — a broken resolver can stall a dispatch past its
> deadline. Only allowlisted hosts are affected. If several addresses are returned, the
> first is used; a host whose first address is dead fails rather than falling back.

## `$app` — database and mailer

`$app` runs a query on its own connection, because a hook is on another thread from the
request and cannot share its transaction. What it *does* share is the request's identity:
every operation switches Postgres to the same role and the same `auth.uid()` the triggering
request ran under, so the generated policies apply exactly as they would to that request.

```ts
$app.findRecordById("users", e.record.author);   // record or null
$app.save("audit", { note: "created" });          // create (no id) or update (with id)
$app.delete("audit", "abc123");                   // { deleted: true }
$app.mailer.send({ to: "a@example.com", template: "receipt", assigns: { total: 42 } });
```

### Whose rules apply

| The write that fired the hook | `$app` runs as |
| --- | --- |
| Anonymous request | `anon` — only what the public rules allow |
| Auth record user | that user — `authenticated`, their `auth.uid()` and claims |
| Superuser | the owning role, i.e. the same bypass that superuser's own request has |
| API key | `anon` — an API key carries no identity, by design |

The superuser row is a deliberate carve-out: a superuser's own requests already bypass the
rules, so a hook they trigger does too. It grants nothing they did not already have, and an
anonymous or user write can never reach it.

An auth record created by an OAuth2 signup runs its hook as `anon`, because that request is
anonymous even though the insert itself happens on the owning connection.

### Rules `$app` enforces on top of the database

* **A write to the collection whose hook is running is refused.** That write has not
  committed yet; acting on the same collection from inside it is a race the hook cannot
  reason about. Use the record the hook was given.
* **A `$app` write does not fire hooks.** Nested dispatch is not "bounded" — it is absent,
  so recursion is impossible rather than limited.
* **Views are refused.** A view carries no Postgres policies, so `$app` could not apply the
  caller's rules to it.
* **An auth collection's password is hashed** through the same path the record endpoint
  uses, so `$app.save` cannot store a plaintext password.
* **`findRecordById` answers `null`** for a row the caller's `viewRule` does not allow — the
  same answer as a row that does not exist.

### `$app.mailer.send`

Queues the same `mail` job `POST /api/mail` queues — it never speaks SMTP inside the
dispatch, which would hold a hook open for as long as the provider takes. The message is
validated by the endpoint's own rules: `{ to, toName? }` plus either a `template` with
`assigns`, or a `subject` with `html` and/or `text`. Sending is refused when SMTP is not
configured, with the same message the endpoint gives. The queue entry is audited with the
triggering caller as the actor (`hook:anonymous` for an anonymous write).

Because a template is resolved at delivery, a template name that does not exist fails the
job (visible in `GET /api/logs` and the job's `dead` state) rather than the request.

### The transaction, stated plainly

A `$app` write commits on its own. If the surrounding record write then fails and rolls
back, the `$app` write **stays**. A before-hook that must not leave a trace behind on a
failed write should not write through `$app` — do it in an after-hook, where the write it
reacts to is already durable.

## What hooks cannot do yet

Stated here so the gaps are not discovered in production:

* **No raw SQL and no `$db`.** `$app` is the only database access, and it goes through the
  record layer (validation, rules, types) rather than around it.
* **No filters in `$app`.** There is `findRecordById`, not `findRecords`. Reading many rows
  from a hook is not available today.
* **No custom routes.** `routerAdd`-style endpoints are not implemented; the only triggers
  are the record events.
* **No file storage access, no job queue access, no cron registration.**
* **No async handlers and no timers.** The runtime has no job queue; a promise is an error.

## A worked example

```ts
interface Order {
  id: string;
  total: number;
  customer: string;
}

onRecordAfterCreate("orders", (e) => {
  const order = e.record as Order;

  // Allowed: another collection, under the caller's rules.
  $app.save("order_events", { order: order.id, kind: "created" });

  // Allowed: a public endpoint that was allowlisted.
  $http.post("https://hooks.example.com/orders", { id: order.id, total: order.total });

  // Refused: the collection whose hook is running.
  // $app.save("orders", { id: order.id, total: 0 });

  // Queued, not sent.
  $app.mailer.send({ to: "ops@example.com", template: "receipt", assigns: { total: order.total } });
});
```
