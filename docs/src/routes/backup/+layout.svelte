<script lang="ts">
	let { children } = $props();
</script>

<svelte:head>
	<title>Import / backup JSON format — Lazypock Docs</title>
	<meta
		name="description"
		content="The JSON format LazyPock accepts for import and restore (Studio Settings → Import / Backups, or POST /api/import): envelope, collections, fields, rules, indexes and records — with a JSON Schema and examples."
	/>
</svelte:head>

<div class="max-w-3xl">
	<div class="prose-md">
		{@render children()}
	</div>
</div>
