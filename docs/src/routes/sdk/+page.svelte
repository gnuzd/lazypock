<script lang="ts">
	import { sdkNav } from '$lib/sdk-nav.generated';

	const comingSoon = [
		{ name: 'Swift', href: '/sdk/swift' },
		{ name: 'Android', href: '/sdk/android' },
		{ name: 'Godot', href: '/sdk/godot' }
	];
</script>

<svelte:head>
	<title>SDKs — Lazypock Docs</title>
</svelte:head>

<h1>Lazypock SDKs</h1>

<p>
	SDKs are client libraries that let your app talk to a Lazypock server without writing raw HTTP.
	Every SDK covers the same concepts — auth, queries, realtime, files — with the codegen, syntax, and
	API surface idiomatic to its language.
</p>

{#each sdkNav as sdk}
	<a href="/sdk/{sdk.slug}" class="card-link">
		<p class="font-semibold">{sdk.name}</p>
		<p class="text-sm text-base-content/70">
			{sdk.pages.length} pages: {sdk.pages.map((p) => p.label).join(' · ')}
		</p>
	</a>
{/each}

<h2>Coming soon</h2>

{#each comingSoon as sdk}
	<a href={sdk.href} class="card-link" aria-disabled="true">
		<p class="font-semibold">{sdk.name}</p>
		<p class="text-sm text-base-content/70">Not released yet — this slot will follow the same structure.</p>
	</a>
{/each}

<p>
	Adding a new SDK? It gets its own section under <code>/sdk/&lt;name&gt;</code> with a
	<code>docs/</code> folder in the SDK's repo — see the
	<a href="/sdk/typescript/install">TypeScript SDK</a> as the reference layout.
</p>
