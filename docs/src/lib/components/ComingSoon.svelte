<script lang="ts">
	let { name }: { name: string } = $props();
</script>

<svelte:head>
	<title>{name} SDK — Lazypock Docs</title>
</svelte:head>

<h1>{name} SDK</h1>

<p>
	Coming soon — a {name} client for Lazypock isn't released yet. When it lands, its docs will appear
	under this section with the same structure as the <a href="/sdk/typescript">TypeScript SDK</a>.
</p>

<p>
	SDK docs are sourced from each SDK's own repo (a <code>docs/</code> folder, synced at build time),
	so adding a new SDK only means pointing the sync script at it.
</p>
