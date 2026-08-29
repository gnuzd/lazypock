<script lang="ts">
	import { sdkNav } from '$lib/sdk-nav.generated';

	const ts = $derived(sdkNav.find((s) => s.slug === 'typescript'));
</script>

<svelte:head>
	<title>TypeScript SDK — Lazypock Docs</title>
</svelte:head>

<h1>TypeScript SDK</h1>

<p>
	<code>lazypock</code> — the official TypeScript client for Lazypock. Fully typed via codegen,
	PocketBase-compatible API surface, and runtime schema support.
</p>

{#if ts}
	<p>
		<strong>Start here:</strong> <a href="/sdk/typescript/{ts.pages[0].slug}">{ts.pages[0].label}</a>
	</p>
	{#each ts.pages as p}
		<a href="/sdk/typescript/{p.slug}" class="card-link">
			<p class="font-semibold">{p.label}</p>
		</a>
	{/each}
{:else}
	<p>SDK pages are not available yet — run <code>node scripts/sync-sdks.mjs</code> in the docs folder.</p>
{/if}
