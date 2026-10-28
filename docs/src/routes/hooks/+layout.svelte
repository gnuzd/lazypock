<script lang="ts">
	import Seo from '$lib/components/Seo.svelte';

	let { children } = $props();

	const title = 'TypeScript hooks — Lazypock Docs';
	const description =
		'Lazypock TypeScript hooks: the six record events, the sandbox and its limits, the $http capability with its allowlist and address checks, and $app — database and mailer access under the triggering request’s row-level security.';
</script>

<Seo {title} {description} />

<div class="max-w-3xl">
	<div class="prose-md">
		{@render children()}
	</div>
</div>
