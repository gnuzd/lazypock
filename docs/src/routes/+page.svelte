<script lang="ts">
	import CodeBlock from '$lib/components/CodeBlock.svelte';
	import Seo from '$lib/components/Seo.svelte';
	import { sdkNav } from '$lib/sdk-nav.generated';
	import { softwareApplicationJsonLd, websiteJsonLd } from '$lib/json-ld';

	const title = 'Lazypock — Self-hosted backend-as-a-service in Rust + Postgres';
	const description =
		'A self-hosted backend-as-a-service on Postgres: define collections in the Studio and every one gets a REST API, realtime over SSE, auth, files and permissions. One Rust binary, no sidecars.';
</script>

<Seo
	kind="website"
	{title}
	{description}
	jsonLd={[websiteJsonLd(description), softwareApplicationJsonLd(description)]}
/>

<div class="prose-doc w-full">
	<!-- HERO -->
	<section class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium">Backend · Rust + PostgreSQL</span>
			{#each sdkNav as sdk}
				<span class="rounded-field bg-success/10 text-success px-2 py-0.5 text-xs font-medium">SDK · {sdk.name}</span>
			{/each}
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">What is Lazypock?</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Lazypock is a <strong>self-hosted backend-as-a-service</strong>. It gives you a production
			REST API, realtime subscriptions, authentication, file storage, jobs and an admin Studio UI —
			without writing a line of backend code. Define your schema in the Studio and every collection
			instantly gets a full CRUD API, permissions and a realtime channel.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The backend is a <strong>single Rust binary on PostgreSQL</strong> — the API, auth, realtime,
			the TypeScript hooks runtime, the job queue and the Studio all ship in one artifact. Your app
			talks to it through one of the official <a class="text-primary underline" href="/sdk">SDKs</a>.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The REST API is deliberately <strong>PocketBase-canonical</strong>, so existing PocketBase
			clients and SDKs point at it unchanged. Underneath it is real Postgres: real tables, real
			columns, and permissions enforced by <strong>Postgres row-level security</strong>.
		</p>
	</section>

	<!-- FEATURES -->
	<section class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">What you get</h2>
		<div class="mt-4 grid gap-3 sm:grid-cols-2">
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🗄️ Dynamic collections</p>
				<p class="mt-1 text-sm text-base-content/70">Create collections in Studio — real Postgres tables with real columns, instantly exposed over REST.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🔐 Auth</p>
				<p class="mt-1 text-sm text-base-content/70">EdDSA tokens with refresh sessions and replay detection, OAuth2 providers, email verification, password reset, and lockout after repeated failures.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🛡️ Permissions as RLS</p>
				<p class="mt-1 text-sm text-base-content/70">Collection rules compile to Postgres row-level security policies. Every request runs under a restricted role, denied by default.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">⚡ Realtime</p>
				<p class="mt-1 text-sm text-base-content/70">Subscriptions over SSE at <code class="doc-inline px-1 py-0.5">/api/realtime</code>, rule-filtered per subscriber, resumable from the last event id.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">📁 File storage</p>
				<p class="mt-1 text-sm text-base-content/70">Validated uploads, local + S3 backends, presigned direct upload, named image presets, and a media library with an orphan sweeper.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🪝 Hooks, cron & mail</p>
				<p class="mt-1 text-sm text-base-content/70">TypeScript hooks in a sandboxed QuickJS runtime, hot-reloaded without a rebuild; a persisted cron scheduler and job queue; an SMTP mailer with templates.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">📦 One binary, no sidecars</p>
				<p class="mt-1 text-sm text-base-content/70">No Redis, no worker process, no image sidecar — Postgres is the source of truth and the message bus, so a node is one process.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🎨 Studio admin UI</p>
				<p class="mt-1 text-sm text-base-content/70">Record browser, field editor, rules, indexes, API keys, logs, and import/export with backups — embedded in the binary.</p>
			</div>
		</div>
	</section>

	<!-- PICK YOUR PATH -->
	<section class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Get started</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Pick the path that matches what you're doing:
		</p>
		<div class="mt-4 grid gap-3 sm:grid-cols-2">
			<a href="/server" class="rounded-box border border-base-300 p-4 hover:border-primary hover:bg-base-200/60 transition-colors">
				<p class="font-semibold text-primary">🚀 Run the server</p>
				<p class="mt-1 text-sm text-base-content/70">
					Deploy Lazypock with Docker, a prebuilt binary, or from source. Covers Studio, production, and env vars.
				</p>
			</a>
			{#each sdkNav as sdk}
				<a href="/sdk/{sdk.slug}" class="rounded-box border border-base-300 p-4 hover:border-primary hover:bg-base-200/60 transition-colors">
					<p class="font-semibold text-primary">📦 Use the {sdk.name} SDK</p>
					<p class="mt-1 text-sm text-base-content/70">
						Install, quick start, type safety, queries, realtime, files, and auth — in {sdk.name}.
					</p>
				</a>
			{/each}
		</div>
	</section>

	<!-- CODE SNIPPET -->
	<section class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">It looks like this</h2>
		<CodeBlock
			lang="typescript" code={`import { LazypockClient } from 'lazypock';

const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });
await client.login('admin@example.com', 'password');

const posts = await client.collection('posts').getList(1, 30);
await client.collection('posts').create({ title: 'Hello', published: true });

client.collection('posts').subscribe((e) => console.log(e.action, e.record));`}
		/>
	</section>

	<!-- LICENSE -->
	<section id="license" class="scroll-mt-20 mb-24">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">License</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Backend and Studio: <a class="text-primary underline" href="https://github.com/gnuzd/lazypock/blob/main/LICENSE" target="_blank" rel="noreferrer">MIT</a>
			© 2024-2025 Chris Nguyen (gnuzd). The TypeScript SDK is
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock-ts/blob/main/LICENSE" target="_blank" rel="noreferrer">MIT</a>
			too.
		</p>
		<div class="mt-8 flex flex-wrap gap-3">
			<a
				class="rounded-field bg-primary text-primary-content px-4 py-2 text-sm font-medium hover:opacity-90"
				href="https://github.com/gnuzd/lazypock"
				target="_blank"
				rel="noreferrer">Backend — gnuzd/lazypock</a
			>
			{#each sdkNav as sdk}
				<a
					class="rounded-field border border-base-300 px-4 py-2 text-sm font-medium hover:bg-base-200"
					href="/sdk/{sdk.slug}"
					rel="noreferrer">SDK — {sdk.name}</a
				>
			{/each}
		</div>
	</section>
</div>
