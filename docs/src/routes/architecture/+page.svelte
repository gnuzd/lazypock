<script lang="ts">
	import ArchDiagram from '$lib/components/ArchDiagram.svelte';
	import FlowSteps, { type FlowStep } from '$lib/components/FlowSteps.svelte';
	import Seo from '$lib/components/Seo.svelte';

	const title = 'Architecture — Lazypock Docs';
	const description =
		'How Lazypock works: the schema engine, the Postgres row-level-security permission model, the filter compiler, realtime over SSE, and the TypeScript hooks runtime.';

	const lifecycleSteps: FlowStep[] = [
		{
			title: 'A request arrives at the axum router',
			lines: [
				{ code: 'POST /api/collections/posts/records   (via the SDK, JSON body)' },
				{
					text: 'The SDK sends the request over HTTPS. If the user logged in, the EdDSA access token it received is attached as a Bearer token — an unauthenticated request is allowed too, because access is decided per collection rule, not per endpoint.',
				},
			],
		},
		{
			title: 'Middleware runs — every request passes through the same stack',
			lines: [
				{ code: 'request-log → auth → rate limit → connection id' },
				{ text: 'The request log records the call to _logs, with credential-shaped keys redacted (visible in Studio → Logs).' },
				{
					text: 'Auth verifies the token from its signature alone — no database round trip — and establishes the actor: a superuser, an auth-collection account, or nobody (public).',
				},
				{
					text: 'Rate limits are applied per route class (API, auth, per-account failures). Trusted proxy headers are honoured only from configured proxies.',
				},
			],
		},
		{
			title: 'The router dispatches to a handler',
			lines: [
				{ code: '…/collections/:collection/records      → list · create' },
				{ code: '…/collections/:collection/records/:id  → view · update · delete' },
				{
					text: 'The record routes are PocketBase-canonical, so an existing PocketBase client addresses this server unchanged. The admin routes (/api/collections, /api/settings, /api/logs, /api/crons, /api/files, /api/sql) and the auth endpoints (…/auth-with-password, …/auth-refresh, …/request-password-reset) are declared alongside them.',
				},
			],
		},
		{
			title: 'Permissions are compiled into Postgres, not evaluated in the server',
			lines: [
				{
					text: 'A collection rule is three-state, exactly as in PocketBase: absent means superuser-only, an empty string means public, and a filter becomes the predicate.',
				},
				{
					text: 'The filter is compiled into a CREATE POLICY and the request then runs under a restricted role — lazypock_anon or lazypock_authenticated — with its claims set per request. Postgres itself enforces the rule, so a missed check in application code cannot leak rows.',
				},
				{
					text: 'On approval, before-hooks fire. A hook can mutate the input or refuse the request outright by throwing.',
				},
			],
		},
		{
			title: 'The query plane builds one parameterized statement',
			lines: [
				{
					text: 'filter, sort, fields, expand and paging are compiled by an in-house builder: identifiers are checked against the schema cache and everything else is a bound parameter. There is no string interpolation of user input into SQL.',
				},
				{
					text: 'Postgres builds the response JSON itself (json_agg / to_jsonb), so the server streams bytes rather than serializing row by row.',
				},
			],
		},
		{
			title: 'Write, publish, respond',
			lines: [
				{ text: 'After the write commits, after-hooks run, then the change is published for realtime.' },
				{ code: '_rt_log ← change row   +   NOTIFY lazypock_change' },
				{ text: 'The response — the stored record, expanded fields included — goes back to the SDK. It is read back after the write rather than via RETURNING, because Postgres applies select policies to a RETURNING row and would refuse a caller allowed to create a record but not to see it.' },
			],
		},
	];

	const realtimeSteps: FlowStep[] = [
		{
			title: 'Open the event stream',
			lines: [
				{ code: 'GET /api/realtime   (Accept: text/event-stream, Bearer token)' },
				{
					text: 'Realtime is Server-Sent Events, not a WebSocket. The same access token used for REST authenticates the stream, so the rules apply identically to both surfaces. A client-supplied connection id lets the server exclude a subscriber from its own writes.',
				},
			],
		},
		{
			title: 'Subscribe to a collection',
			lines: [
				{ code: 'collection:posts          → every change to “posts”' },
				{ code: 'collection:posts:abc123   → one record' },
				{
					text: 'Subscriptions are filtered by the same rules: a subscriber only receives rows it would be allowed to read, evaluated per subscriber rather than once per node.',
				},
			],
		},
		{
			title: 'Every write reaches the bus the same way',
			lines: [
				{ code: 'trigger → _rt_log + NOTIFY  →  LISTEN loop  →  EventBus  →  SSE' },
				{
					text: 'A trigger on every collection appends to _rt_log and NOTIFYs a deliberately small payload — NOTIFY caps at 8000 bytes, so ids are sent and the row is read separately. Writes from the SDKs, the Studio, hooks, cron jobs and imports all take this one path, which is why every client sees the same stream.',
				},
			],
		},
		{
			title: 'Subscribers receive the event',
			lines: [
				{ code: '{"action":"create|update|delete","record":{…}}' },
				{
					text: 'The originating connection is excluded, so a writer does not echo to itself. Because each event is numbered, a reconnect sends Last-Event-ID and replays what it missed from _rt_log instead of silently dropping changes.',
				},
			],
		},
	];

	// Keep step.code and step.text in sync with the line counts the diagram shows.
</script>

<Seo {title} {description} />

<div class="prose-doc max-w-4xl">
	<!-- INTRO -->
	<section id="overview" class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium"
				>Architecture · Rust + PostgreSQL</span
			>
			<span class="rounded-field bg-success/10 text-success px-2 py-0.5 text-xs font-medium"
				>one binary</span
			>
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">Architecture</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Lazypock is a single <strong>Rust binary</strong> — an axum/tokio server with the compiled
			Studio SPA embedded — that speaks PocketBase-canonical REST and realtime on top of
			<strong>PostgreSQL</strong>. Define a collection in the Studio and it becomes a real table,
			exposed at <code class="doc-inline px-1 py-0.5"
				>/api/collections/&#123;collection&#125;/records</code
			>, with per-action permissions, files and a realtime stream. There is no backend code to
			write.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			What makes it more than a REST wrapper is where the guardrails live. Permissions are
			<strong>Postgres row-level security policies</strong> generated from the rules, so the
			database is the security boundary rather than a check the application is trusted to
			remember. Realtime, jobs and cron all use Postgres itself as the message bus, so a node is
			one process with no Redis and no worker deployment.
		</p>
		<p class="mt-3 text-sm text-base-content/70 leading-relaxed">
			The repo is a Cargo workspace: <code class="doc-inline px-1 py-0.5">crates/api</code> (the
			axum router and handlers), <code class="doc-inline px-1 py-0.5">crates/db</code> (pool,
			migrations, schema engine, RLS, query building), <code class="doc-inline px-1 py-0.5"
				>crates/auth</code
			>,
			<code class="doc-inline px-1 py-0.5">crates/realtime</code>,
			<code class="doc-inline px-1 py-0.5">crates/files</code>,
			<code class="doc-inline px-1 py-0.5">crates/hooks</code>,
			<code class="doc-inline px-1 py-0.5">crates/mail</code>,
			<code class="doc-inline px-1 py-0.5">crates/studio-embed</code> and
			<code class="doc-inline px-1 py-0.5">crates/app</code> (the
			<code class="doc-inline px-1 py-0.5">lazypock</code> binary).
			<code class="doc-inline px-1 py-0.5">studio/</code> is the SvelteKit admin UI, built into the
			binary and served at <code class="doc-inline px-1 py-0.5">/</code>; official SDKs live in
			their own repos and speak the same HTTP surface.
		</p>
	</section>

	<!-- LAYERS -->
	<section id="layers" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">System layers</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Read top to bottom — every arrow is a request or event boundary. The dashed box is the
			deliverable you deploy: one statically linked binary with the Studio assets inside it.
			Postgres is the only thing outside.
		</p>

		<figure class="mt-6 rounded-box border border-base-300 p-2 sm:p-4">
			<ArchDiagram />
			<figcaption class="mt-2 px-2 pb-1 text-xs text-base-content/60 leading-relaxed">
				Each arrow is a request/event boundary. Studio and SDK apps are external clients; the Rust
				server + Studio assets ship as one binary (dashed box); PostgreSQL is a separate service
				reached over <code class="doc-inline px-1 py-0.5">LAZYPOCK_DATABASE_URL</code>.
			</figcaption>
		</figure>

		<div class="mt-6 grid gap-3 sm:grid-cols-2">
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🗂️ Schema engine</p>
				<p class="mt-1 text-sm text-base-content/70">
					Collection definitions in <code class="doc-inline px-1 py-0.5">_collections</code> /
					<code class="doc-inline px-1 py-0.5">_fields</code> become real tables, columns,
					indexes and relations through the DDL engine. Each node holds the schema as an
					<code class="doc-inline px-1 py-0.5">ArcSwap</code> cache; a schema change
					<code class="doc-inline px-1 py-0.5">NOTIFY</code>s every node to reload, so the read
					path stays lock-free. An existing ordinary database is reflected on boot: every table
					becomes a collection and every column a field.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🛡️ Permissions as RLS</p>
				<p class="mt-1 text-sm text-base-content/70">
					Rules are three-state: absent = superuser only, empty = public, filter = conditional.
					Each rule compiles to a <code class="doc-inline px-1 py-0.5">CREATE POLICY</code>, and
					user-facing requests run under
					<code class="doc-inline px-1 py-0.5">lazypock_anon</code> /
					<code class="doc-inline px-1 py-0.5">lazypock_authenticated</code> with claims set per
					request. Policies are rebuilt on every change, so a removed rule stops applying, and
					an unreadable rule denies.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🪝 Hooks engine</p>
				<p class="mt-1 text-sm text-base-content/70">
					TypeScript hooks run in a sandboxed QuickJS context with hard limits, on a worker
					thread of their own, and reload on file change without a rebuild. Six record events:
					before/after create, update and delete. Before-hooks run before validation so a hook
					can supply a field; after-hooks run post-commit.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">⚡ Query plane &amp; realtime</p>
				<p class="mt-1 text-sm text-base-content/70">
					One generic query plane serves every collection: the filter parser feeds a
					schema-whitelisted, fully parameterized SQL builder, and Postgres builds the JSON.
					Realtime fans the same change events out over SSE, RLS-filtered per subscriber.
				</p>
			</div>
		</div>
	</section>

	<!-- REQUEST LIFECYCLE -->
	<section id="lifecycle" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Request lifecycle (REST CRUD)</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			What happens when an SDK creates or updates a record — e.g.
			<code class="doc-inline px-1 py-0.5"
				>client.collection('posts').create(&#123;title:'Hello'&#125;)</code
			>:
		</p>

		<div class="mt-6">
			<FlowSteps steps={lifecycleSteps} />
		</div>
	</section>

	<!-- REALTIME -->
	<section id="realtime" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Realtime flow</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Realtime is <strong>Server-Sent Events</strong> over a single long-lived HTTP response.
			Postgres is the event source: a trigger on each collection writes the change and notifies
			every node, so no broker sits in the middle and events survive a node restart. The same
			access token used for REST authenticates the stream, so rules apply consistently:
		</p>

		<div class="mt-6">
			<FlowSteps steps={realtimeSteps} />
		</div>
	</section>

	<!-- DESIGN DECISIONS -->
	<section id="design" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Key design decisions</h2>
		<div class="mt-3 overflow-x-auto rounded-box border border-base-300">
			<table class="w-full text-sm">
				<thead class="bg-base-200 text-left">
					<tr>
						<th class="px-3 py-2 font-semibold">Decision</th>
						<th class="px-3 py-2 font-semibold">Choice</th>
						<th class="px-3 py-2 font-semibold">Why</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-base-300 align-top">
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Language &amp; runtime</td>
						<td class="px-3 py-2">Rust — tokio, axum, tokio-postgres</td>
						<td class="px-3 py-2 text-base-content/70">Small binary, low idle RAM, predictable latency, and no GC pause on the realtime path</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Schema approach</td>
						<td class="px-3 py-2">Real Postgres columns, queried dynamically (no codegen)</td>
						<td class="px-3 py-2 text-base-content/70">Full Postgres power — types, indexes, JSONB, relations — with no runtime module compilation</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Access rules</td>
						<td class="px-3 py-2">Rules compiled to Postgres RLS policies</td>
						<td class="px-3 py-2 text-base-content/70">The database enforces the rule, not the application — a forgotten check in a handler cannot leak rows</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Tenancy</td>
						<td class="px-3 py-2">Single tenant — one database is one app</td>
						<td class="px-3 py-2 text-base-content/70">Same mental model as PocketBase’s “one file = one app”; keeps rules and auth simple. The schema is never hard-coded to <code class="doc-inline px-1 py-0.5">public</code>, so a shared-database mode stays open</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Message bus</td>
						<td class="px-3 py-2">Postgres <code class="doc-inline px-1 py-0.5">LISTEN</code>/<code class="doc-inline px-1 py-0.5">NOTIFY</code>, behind an EventBus trait</td>
						<td class="px-3 py-2 text-base-content/70">No Redis to run or pay for, and events outlive a node restart. NOTIFY is capped at 8000 bytes, so payloads carry ids and the row is read separately</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Realtime transport</td>
						<td class="px-3 py-2">SSE with numbered events and <code class="doc-inline px-1 py-0.5">Last-Event-ID</code> resume</td>
						<td class="px-3 py-2 text-base-content/70">One direction, proxied and buffered cleanly by ordinary HTTP infrastructure, and a reconnect recovers missed events instead of dropping them</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Hooks</td>
						<td class="px-3 py-2">TypeScript in QuickJS, hot-reloaded, six record events</td>
						<td class="px-3 py-2 text-base-content/70">PocketBase-style extensibility without a rebuild. A sandbox with limits keeps a hook from taking the server down</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Auth</td>
						<td class="px-3 py-2">argon2id + EdDSA access tokens, refresh sessions with rotation and replay detection, published JWKS</td>
						<td class="px-3 py-2 text-base-content/70">Tokens verify from the signature alone, with no database round trip on the hot path; a stolen refresh token is detected by its reuse</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Images</td>
						<td class="px-3 py-2">Resized in-process by default, offloadable to a service</td>
						<td class="px-3 py-2 text-base-content/70">No sidecar to deploy in the common case; <code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_SERVICE_URL</code> moves the work out, and a build without <code class="doc-inline px-1 py-0.5">local-image-codecs</code> leaves the codecs out of the binary</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Packaging</td>
						<td class="px-3 py-2">One binary — server + Studio SPA + migrations</td>
						<td class="px-3 py-2 text-base-content/70">“Download one file and run”, like PocketBase, while keeping Postgres external where a real deployment wants it</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Roadmap</td>
						<td class="px-3 py-2">Webhooks, MFA/TOTP, and the <code class="doc-inline px-1 py-0.5">$db</code>/<code class="doc-inline px-1 py-0.5">$http</code> hook bindings</td>
						<td class="px-3 py-2 text-base-content/70">Tracked in PLAN.md. <code class="doc-inline px-1 py-0.5">$db</code> is a semantics decision rather than plumbing — the hook thread cannot join the request’s transaction, so it would commit independently of the record write</td>
					</tr>
				</tbody>
			</table>
		</div>
	</section>

	<!-- DEEP DIVE -->
	<section id="deep-dive" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Going deeper</h2>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li>
				<a
					class="text-primary underline"
					href="https://github.com/gnuzd/lazypock/blob/main/PLAN.md"
					target="_blank"
					rel="noreferrer">PLAN.md</a
				>
				— the full architecture &amp; development plan: the DDL engine, the rule-to-policy
				compiler, the hook system, the realtime design and the release pipeline.
			</li>
			<li>
				<a
					class="text-primary underline"
					href="https://github.com/gnuzd/lazypock/blob/main/README.md"
					target="_blank"
					rel="noreferrer">Repository README</a
				>
				— feature overview and the CLI reference (<code class="doc-inline px-1 py-0.5"
					>lazypock serve</code
				>, <code class="doc-inline px-1 py-0.5">migrate</code>,
				<code class="doc-inline px-1 py-0.5">superuser</code>,
				<code class="doc-inline px-1 py-0.5">healthcheck</code>).
			</li>
			<li>
				Crate docs — each crate carries its own module documentation:
				<code class="doc-inline px-1 py-0.5">cargo doc --workspace --open</code>.
			</li>
			<li>
				Module map inside the binary: <code class="doc-inline px-1 py-0.5"
					>lazypock_db::schema</code
				>
				(DDL + schema cache) · <code class="doc-inline px-1 py-0.5">lazypock_db::rls</code> (policy
				generation) · <code class="doc-inline px-1 py-0.5">lazypock_hooks</code> (QuickJS runtime
				and loader) · <code class="doc-inline px-1 py-0.5">lazypock_realtime</code> (EventBus and
				fan-out) · <code class="doc-inline px-1 py-0.5">lazypock_files</code> ·
				<code class="doc-inline px-1 py-0.5">lazypock_api::router</code>.
			</li>
		</ul>
	</section>
</div>
