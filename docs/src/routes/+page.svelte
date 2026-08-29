<script lang="ts">
	import CodeBlock from '$lib/components/CodeBlock.svelte';
	import { sdkNav } from '$lib/sdk-nav.generated';
</script>

<svelte:head>
	<title>Lazypock — Open-source, PocketBase-compatible backend</title>
</svelte:head>

<div class="prose-doc max-w-3xl">
	<!-- HERO -->
	<section class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium">Backend · Elixir + Phoenix + PostgreSQL</span>
			{#each sdkNav as sdk}
				<span class="rounded-field bg-success/10 text-success px-2 py-0.5 text-xs font-medium">SDK · {sdk.name}</span>
			{/each}
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">What is Lazypock?</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Lazypock is an <strong>open-source, PocketBase-compatible backend</strong>. It gives you a
			production-grade REST API, realtime subscriptions, authentication, file storage, and an admin
			Studio UI — without writing a single line of backend code. Define your schema in the Studio and
			every collection instantly gets a full CRUD API, rules, and a realtime channel.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The backend is a single binary built on Elixir + Phoenix + PostgreSQL. Your app talks to it
			through one of the official <a class="text-primary underline" href="/sdk">SDKs</a>.
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
				<p class="mt-1 text-sm text-base-content/70">Superuser JWTs, auth collections, OAuth2 (Google / GitHub / generic), rules per collection.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">⚡ Realtime</p>
				<p class="mt-1 text-sm text-base-content/70">Phoenix Channels subscriptions, rule-enforced, anonymous access on public collections.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">📁 File storage</p>
				<p class="mt-1 text-sm text-base-content/70">Uploads, local + S3 adapters, image thumbnails and on-demand scaling.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🪝 Hooks &amp; cron</p>
				<p class="mt-1 text-sm text-base-content/70">PocketBase-style event hooks (~80 hook points), custom API routes, persisted cron jobs.</p>
			</div>
			<div class="rounded-box border border-base-300 p-4">
				<p class="font-semibold">🎨 Studio admin UI</p>
				<p class="mt-1 text-sm text-base-content/70">Record browser, field editor, rules, indexes, API keys, import/export, backups.</p>
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
