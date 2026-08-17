# Hook Sandboxing & Security Review

> Date: 2026-08-16 · Scope: how `priv/hooks/*.ex` and `~/.lazypock/hooks/*.ex`
> user hooks are compiled and executed, what access they have, existing
> guardrails, and hardening options.

## 1. How hooks are compiled and loaded

Two hook sources, both plain Elixir modules using `use Lazypock.Hooks.Hook`:

| Source | Location | Compiled | Loaded by |
|---|---|---|---|
| Built-in hooks | `priv/hooks/*.ex` | **At build time** into the release binary (`elixirc_paths` includes `priv/hooks` for non-test envs) | `Lazypock.Hooks.Registry.discover!/0` at boot |
| **User hooks** | `~/.lazypock/hooks/*.ex` (override: `LAZYPOCK_HOOKS_DIR`) | **At runtime on boot** via `Code.compile_file/1` | `Lazypock.Hooks.User.load!/0` at boot; `reload!/0` available |

Key fact: **the user hooks directory is user-writable on disk** (PocketBase
`pb_hooks` style). Any `.ex` file placed there is compiled and executed inside
the server on the next boot/reload. `sync_builtins!/0` copies the example hook
files in on first boot but never overwrites user edits.

Registration: compiled modules that export `__hook_registrations__/0`
(new-style, self-registering via the `Hook` macro) or `__collection__/0`
(legacy lifecycle modules) are registered into a `:persistent_term`-backed
registry (`Lazypock.Hooks.Registry`).

## 2. Execution model

- Handlers run **in-process, in the same BEAM VM as the server**.
- Most `on_record_*`, `on_collection_*`, request and realtime hooks run
  **synchronously in the request process** (a slow or blocking hook blocks the
  request; there is no timeout).
- `on_record_after_*_success` post-funs run after commit, still in-process.
- The event chain contract: each handler receives an `Event` and returns
  `{:ok, e}` / `{:error, reason}` / `{:after, e, fun}`. Exceptions and throws
  are caught by `Registry.safe_call/2` and converted to chain aborts — a
  raising hook **does not crash the request process**.

## 3. What hook code can access — everything

Because hooks compile into the same VM, they have **the full privileges of the
server process**:

- **All application modules and data**: `Lazypock.Repo` (raw SQL via
  `Ecto.Adapters.SQL.query!`), every collection table, settings (incl. SMTP
  credentials, API keys, OAuth client secrets), `_files`, `_request_logs`,
  `_superusers`.
- **OS-level**: `System.cmd/3`, `:os.cmd/1`, `Port.open/2` (spawn arbitrary
  processes), `File.*` (read/write any path the OS user can), env vars.
- **Network**: `:gen_tcp`, `:httpc`, `:ssl`, any OTP app present in the release.
- **Secrets**: the JWT signing key (`LazypockWeb.Endpoint` secret_key_base via
  `Phoenix.Token`), DB credentials (via the running `Repo` pool), any secret
  stored in settings or env.
- **VM control**: spawn processes, create ETS tables, write `:persistent_term`,
  `System.halt(0)` / `:erlang.halt()` (kill the server), raise the VM's
  memory/CPU usage.
- **Hooks are also registered for request/email/realtime events**, so they can
  intercept, mutate, block, or impersonate any request path.

In short: **hook code is same-privilege code** — equivalent to PocketBase's
JS hooks (which also run in-process), but with *more* capability because it is
compiled Elixir with direct access to the BEAM, the DB adapter, and the OS.

## 4. Existing guardrails

| Guardrail | What it prevents | Gap |
|---|---|---|
| `Registry.safe_call/2` catches exceptions/throws | A raising hook doesn't crash the request process; chain aborts with `{:error, {:hook_exception, …}}` | Doesn't limit *what* the hook does — only that it can't take down the request process by raising. |
| `collect_only` dispatch mode (enrichment) | Handler results are collected, aborts ignored | Enrichment only; mutations can still do anything. |
| Boot-time registration + `Registry.clear/0` | Handlers can be unloaded | No runtime kill-switch exposed; clearing doesn't undo side effects already performed. |
| `User.load!/0` rescues compile errors | A broken `.ex` file doesn't abort boot | The file is still compiled — arbitrary code *runs* at compile time too (module attributes, `Application.put_env`, `System.cmd` inside the module body, etc.). |
| Hooks run with full privileges, documented here | — | **No sandbox exists.** There is no module allowlist, no filesystem jail, no network restriction, no CPU/memory cap, no timeout. |

## 5. Attack scenarios

1. **Arbitrary code execution via file write** — an attacker who can write to
   `~/.lazypock/hooks/` (compromised deploy user, shared hosting, configurable
   `LAZYPOCK_HOOKS_DIR` pointing somewhere writable) gains full control of the
   server: read secrets, exfiltrate the DB, kill the VM.
2. **Supply-chain / malicious hook in a distribution** — a delivered
   `priv/hooks/*.ex` or user hook is malicious; nothing checks it.
3. **Accidental footguns** — `:timer.sleep(:infinity)` blocks the request
   process; `System.cmd` shelling out on every record create kills throughput;
   a hook that queries the DB re-entrantly inside a transaction deadlocks.
4. **Secret exfiltration** — a hook can `POST` settings/DB contents to any
   endpoint at any time (including in `after` hooks, which the request process
   cannot cancel).

## 6. Hardening options (in increasing order of effort)

1. **Document the trust model** (this document) + **boot warning** when hooks
   load, stating that hooks run with full server privileges. (Done here.)
2. **`LAZYPOCK_DISABLE_HOOKS=1` env var** — skip user-hook loading *and*
   built-in discovery for hardened deployments (e.g. multitenant or
   customer-hosted instances). (Implemented in this pass.)
3. **Runtime kill-switch** — `Lazypock.Hooks.Registry.disable!/0` +
   `LAZYPOCK_DISABLE_HOOKS` at runtime; hooks stop firing after a restart.
4. **Request-path timeouts** — run each handler in a supervised task with a
   timeout (e.g. 5s) and treat timeout as a chain abort. Event mutation works
   across processes (events are immutable data), but `after` semantics and
   handle_in replies need care.
5. **Module allowlist / deny-list at compile time** — a compile-time or
   `Code.compile_file` wrapper that refuses `System`, `:os`, `Port`, `File`
   writes, `Repo` outside approved wrappers. **This is not a real sandbox**:
   compiled BEAM code can always reach these via `:erlang`/`:code`/`apply`
   tricks, but it raises the bar and catches accidental footguns.
6. **Separate OS process (port runner)** — hooks run in a second BEAM with a
   restricted `RELEASE_DISTRIBUTION`/network and an RPC-style protocol, so a
   malicious hook is contained to that process. High effort; breaks the simple
   "just a .ex file" DX and the `e.next()` mutation model.
7. **Code review / signing** — require hooks to be reviewed and signed (e.g.
   checked into the repo, CI-reviewed) before deployment; only ship trusted
   hooks. This is the PocketBase model's actual control: hooks are authored by
   the deployer, not by app end-users.

## 7. Recommendation

- **Default posture**: hooks are **trusted, same-privilege code** authored by
  the operator (like PocketBase). Do not allow app end-users to write hook
  files; treat `~/.lazypock/hooks/` as sensitive as the server binary.
- **Quick wins (implemented)**: boot warning + `LAZYPOCK_DISABLE_HOOKS=1`.
- **Next best (recommend, not implemented)**: request-path hook timeouts (#4)
  and a deny-list that refuses `System.halt`/`:os.cmd`/`Port.open` at compile
  time (#5) — both meaningfully reduce footgun/DoS blast radius.
- **If untrusted hooks are a requirement** (multi-tenant hook marketplaces
  etc.), plan for the separate-process runner (#6) as its own project.
