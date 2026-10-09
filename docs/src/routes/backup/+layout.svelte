<script lang="ts">
	import Seo from '$lib/components/Seo.svelte';

	let { children } = $props();

	const title = 'Import / backup JSON format — Lazypock Docs';
	const description =
		'The JSON format Lazypock accepts for import and restore (Studio Settings → Import / Backups, or POST /api/import): envelope, collections, fields, rules, indexes and records — with a JSON Schema and examples.';
</script>

<Seo {title} {description} />

<div class="max-w-3xl">
	<div class="prose-md">
		{@render children()}
	</div>
</div>
