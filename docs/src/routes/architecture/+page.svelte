<script lang="ts">
	import ArchDiagram from '$lib/components/ArchDiagram.svelte';
	import FlowSteps, { type FlowStep } from '$lib/components/FlowSteps.svelte';

	const lifecycleSteps: FlowStep[] = [
		{
			title: 'A request arrives at the Phoenix endpoint',
			lines: [
				{ code: 'POST /api/posts   (via the SDK, JSON body)' },
				{
					text: 'The SDK sends the request over HTTPS. If the user logged in, the JWT it received is attached as a Bearer token — unauthenticated requests are allowed too, because access is decided per rule, not per endpoint.',
				},
			],
		},
		{
			title: 'Plugs run — every request passes through the same chain',
			lines: [
				{ code: 'RequestLogger → Auth.Plug → ConnectionId → CustomRoutes' },
				{ text: 'RequestLogger records the request to _request_logs (visible in Studio → Logs).' },
				{
					text: 'Auth.Plug verifies the JWT and sets the current actor: a superuser, an auth-collection user, or nobody (public).',
				},
				{
					text: 'CustomRoutes gives on_before_serve hooks the chance to serve their own endpoints before the built-in routes are evaluated.',
				},
			],
		},
		{
			title: 'The router dispatches to a controller',
			lines: [
				{
					code: '…/:collection        → DynamicController (list · create)',
				},
				{
					code: '…/:collection/:id    → DynamicController (view · update · delete)',
				},
				{
					text: 'Dynamic routes are declared last, so static prefixes (/api/files, /api/collections, /api/settings, /api/logs, /api/crons) and auth endpoints (/api/:collection/auth-*) always win. Collection names are resolved at runtime from _collections.',
				},
			],
		},
		{
			title: 'The rule enforcer and request hooks decide access',
			lines: [
				{
					text: 'The actor is checked against the collection’s rule for that action — listRule, viewRule, createRule, updateRule, deleteRule — with superuser and manageRule bypass.',
				},
				{
					text: 'On approval, before-hooks fire (file-based Elixir modules, in registry order). A hook can mutate the input or abort the whole request by returning an error.',
				},
			],
		},
		{
			title: 'The generic data layer executes the query',
			lines: [
				{
					text: 'Filter/sort/expand/pagination are compiled into one Ecto query over the collection’s real Postgres table. Rule filter strings become SQL WHERE clauses with bound parameters; relation fields can be expanded with joins.',
				},
			],
		},
		{
			title: 'Write, broadcast, respond',
			lines: [
				{ text: 'After the write, after-hooks run, then the Realtime Broadcaster publishes the change to the collection’s channel (skipping the originating connection via its connectionId).' },
				{ code: '{"action":"create","record":{…}}  →  topic collection:posts' },
				{ text: 'The JSON response — the saved or updated record, expanded fields included — goes back to the SDK.' },
			],
		},
	];

	const realtimeSteps: FlowStep[] = [
		{
			title: 'Open the WebSocket',
			lines: [
				{ code: 'wss://…/socket?token=JWT&connectionId=…' },
				{
					text: 'The socket authenticates the token — superuser, auth-collection user, or anonymous (allowed for public collections). The client-generated connectionId lets the server exclude the sender from its own broadcasts.',
				},
			],
		},
		{
			title: 'Subscribe to a collection topic',
			lines: [
				{ code: 'collection:posts         → all changes to “posts”' },
				{ code: 'collection:posts:abc123  → changes to one record' },
				{
					text: 'Subscriptions are rule-filtered: listRule/viewRule are enforced per topic, so a client can only subscribe to records it is allowed to read.',
				},
			],
		},
		{
			title: 'Every write fans out through one broadcaster',
			lines: [
				{ code: 'Phoenix PubSub → collection:posts → record_change' },
				{
					text: 'Writes from the SDKs, the Studio, hooks, cron jobs and imports all go through the same broadcaster — a single source of truth, so every client sees the same stream regardless of who wrote.',
				},
			],
		},
		{
			title: 'Subscribers receive the event',
			lines: [
				{ code: '{"action":"create|update|delete","record":{…}}' },
				{
					text: 'The writing client is excluded via connectionId (no self-echo). The Studio listens on the admin “collections” channel to live-refresh its schema sidebar when collections change. Server-initiated topics (e.g. chat:room1) map to the custom channel and are broadcast from hooks.',
				},
			],
		},
	];

	// Keep step.code and step.text in sync with the line counts the diagram shows.
</script>

<svelte:head>
	<title>Architecture — Lazypock Docs</title>
</svelte:head>

<div class="prose-doc max-w-4xl">
	<!-- INTRO -->
	<section id="overview" class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium"
				>Architecture · Elixir + Phoenix + PostgreSQL</span
			>
			<span class="rounded-field bg-success/10 text-success px-2 py-0.5 text-xs font-medium"
				>one binary</span
			>
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">Architecture</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Lazypock is a single <strong>BEAM application</strong> (Elixir + Phoenix) that ships as one
			Burrito binary — backend plus the compiled Studio SPA — and speaks
			PocketBase-compatible REST and realtime on top of <strong>PostgreSQL</strong>. You point it at
			a Postgres URL, it registers every public table, and each collection instantly gets a dynamic
			<code class="doc-inline px-1 py-0.5">/api/:collection</code> API, per-action rules, files, and
			a realtime channel — no backend code to write.
		</p>
		<p class="mt-3 text-sm text-base-content/70 leading-relaxed">
			The repo is a monorepo: <code class="doc-inline px-1 py-0.5">core/</code> (the Elixir app this
			page is about), <code class="doc-inline px-1 py-0.5">studio/</code> (the SvelteKit admin UI,
			built into the binary and served at <code class="doc-inline px-1 py-0.5">/_/</code>), and
			<code class="doc-inline px-1 py-0.5">docs/</code> (this site). Official SDKs live in their own
			repos and speak to the same HTTP + WebSocket surface.
		</p>
	</section>

	<!-- LAYERS -->
	<section id="layers" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">System layers</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Read top to bottom — every arrow is a request or event boundary. The dashed box is the
			deliverable you download: one BEAM app with the Studio assets bundled. Postgres is the only
			thing outside it.
		</p>

		<figure class="mt-6 rounded-box border border-base-300 p-2 sm:p-4">
			<ArchDiagram />
			<figcaption class="mt-2 px-2 pb-1 text-xs text-base-content/60 leading-relaxed">
				Each arrow is a request/event boundary. Studio and SDK apps are external clients; the BEAM
				app + Studio assets ship as one binary (dashed box); PostgreSQL is a separate service
				connected with <code class="doc-inline px-1 py-0.5">DATABASE_URL</code>.
			</figcaption>
		</figure>

		<div class="mt-6 grid gap-3 sm:grid-cols-2">
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🗂️ Schema / DDL engine</p>
				<p class="mt-1 text-sm text-base-content/70">
					Collection definitions in <code class="doc-inline px-1 py-0.5">_collections</code> /
					<code class="doc-inline px-1 py-0.5">_fields</code> become real tables, columns, indexes
					and relations through the TypeMapper. Existing tables are auto-registered on boot.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🪝 Hooks engine</p>
				<p class="mt-1 text-sm text-base-content/70">
					~80 lifecycle events with before/after semantics and custom API routes
					(<code class="doc-inline px-1 py-0.5">on_before_serve</code>). Hook modules are loaded
					from a user-writable directory — no rebuild to add one.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🔐 Rule enforcer</p>
				<p class="mt-1 text-sm text-base-content/70">
					PocketBase-compatible rules per action, evaluated three-state: absent = superuser only,
					empty = public, filter string = conditional. Compiled to SQL WHERE with bound
					parameters.
				</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">⚡ Data layer &amp; realtime</p>
				<p class="mt-1 text-sm text-base-content/70">
					One generic Ecto layer serves every collection, and one Broadcaster publishes every
					write to Phoenix Channels — rule-filtered topics with origin exclusion.
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
			Realtime is Phoenix Channels over a single WebSocket (long-poll fallback disabled). The same
			JWT used for REST authenticates the socket, so rules apply consistently to both surfaces:
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
						<td class="px-3 py-2 font-medium whitespace-nowrap">Schema approach</td>
						<td class="px-3 py-2">Real Postgres columns, queried dynamically (no codegen)</td>
						<td class="px-3 py-2 text-base-content/70">Full Postgres power — types, indexes, JSONB, relations — with no runtime module compilation</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Tenancy</td>
						<td class="px-3 py-2">Single tenant — one database is one app</td>
						<td class="px-3 py-2 text-base-content/70">Same mental model as PocketBase’s “one SQLite file = one app”; keeps rules and auth simple</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Access rules</td>
						<td class="px-3 py-2">PocketBase-compatible DSL stored per collection</td>
						<td class="px-3 py-2 text-base-content/70">Declarative and safe — filters compile to SQL WHERE with bound parameters, no string interpolation</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Hooks</td>
						<td class="px-3 py-2">~80 lifecycle events; Elixir hook modules + custom routes</td>
						<td class="px-3 py-2 text-base-content/70">PocketBase-style extensibility — drop a module in a user-writable directory, no binary rebuild</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Auth</td>
						<td class="px-3 py-2">Custom JWT provider — dual token (superuser + auth user)</td>
						<td class="px-3 py-2 text-base-content/70">One token format verified in a single plug, shared by HTTP and the realtime socket</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Realtime</td>
						<td class="px-3 py-2">Phoenix Channels + PubSub, one WebSocket</td>
						<td class="px-3 py-2 text-base-content/70">Native to the platform, scales on the BEAM; rule-filtered topics and origin exclusion built in</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">File storage</td>
						<td class="px-3 py-2">Adapter-based store — local disk (default) or S3 / R2</td>
						<td class="px-3 py-2 text-base-content/70">Pluggable backends selected per file; thumbnails and on-demand scaling via ImageMagick when present</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Packaging</td>
						<td class="px-3 py-2">One Burrito binary — BEAM app + Studio SPA + migrations</td>
						<td class="px-3 py-2 text-base-content/70">PocketBase-like “download one file and run”, while keeping Postgres external for real deployments</td>
					</tr>
					<tr>
						<td class="px-3 py-2 font-medium whitespace-nowrap">Roadmap</td>
						<td class="px-3 py-2">Sandboxed runtime hook evaluation (Phase 10)</td>
						<td class="px-3 py-2 text-base-content/70">Not shipped yet — tracked in PLAN.md alongside release-polish items</td>
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
				— the full architecture &amp; development plan: per-phase breakdowns of the DDL engine,
				rule compiler, hook system, and release pipeline.
			</li>
			<li>
				<a
					class="text-primary underline"
					href="https://github.com/gnuzd/lazypock/blob/main/README.md"
					target="_blank"
					rel="noreferrer">Repository README</a
				>
				— feature overview and the CLI reference (<code class="doc-inline px-1 py-0.5"
					>lazypock migrate</code
				>, <code class="doc-inline px-1 py-0.5">seed</code>, …).
			</li>
			<li>
				<a
					class="text-primary underline"
					href="https://github.com/gnuzd/lazypock/blob/main/core/README.md"
					target="_blank"
					rel="noreferrer">core/README.md</a
				>
				— developer setup for the Elixir app: <code class="doc-inline px-1 py-0.5">mix setup</code>,
				env vars, tests.
			</li>
			<li>
				Module map inside the binary: <code class="doc-inline px-1 py-0.5"
					>Lazypock.Schema</code
				>
				(DDL/TypeMapper) · <code class="doc-inline px-1 py-0.5">Lazypock.Rules.Enforcer</code> ·
				<code class="doc-inline px-1 py-0.5">Lazypock.Hooks.*</code> ·
				<code class="doc-inline px-1 py-0.5">Lazypock.Realtime.Broadcaster</code> ·
				<code class="doc-inline px-1 py-0.5">Lazypock.Files.Store</code> ·
				<code class="doc-inline px-1 py-0.5">LazypockWeb.Router</code>.
			</li>
		</ul>
	</section>
</div>
