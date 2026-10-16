<script lang="ts">
	import Seo from '$lib/components/Seo.svelte';
	import { sdkNav } from '$lib/sdk-nav.generated';

	const title = 'SDKs — Lazypock Docs';
	const description =
		'The TypeScript SDK for the Lazypock backend — install, codegen, queries, realtime, files and auth.';
</script>

<Seo {title} {description} />

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

<p>
	Adding a new SDK? It gets its own section under <code>/sdk/&lt;name&gt;</code> with a
	<code>docs/</code> folder in the SDK's repo — see the
	<a href="/sdk/typescript/install">TypeScript SDK</a> as the reference layout.
</p>
