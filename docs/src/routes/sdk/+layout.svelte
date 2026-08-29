<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { sdkNav } from '$lib/sdk-nav.generated';

	let { children } = $props();

	// e.g. ['sdk', 'typescript'] — each SDK is a single scrollable page now.
	const segments = $derived(page.url.pathname.split('/').filter(Boolean));
	const sdk = $derived(sdkNav.find((s) => s.slug === segments[1]));

	const crumbs = $derived.by(() => {
		const parts: { label: string; href?: string }[] = [{ label: 'SDKs', href: '/sdk' }];
		if (sdk) parts.push({ label: sdk.name, href: `/sdk/${sdk.slug}` });
		return parts;
	});

	// Markdown pages can't set <title> from frontmatter, so derive one from the h1.
	onMount(() => {
		const h1 = document.querySelector('main h1');
		if (h1?.textContent) document.title = `${h1.textContent.trim()} — Lazypock Docs`;
	});
</script>

<div class="max-w-3xl">
	{#if crumbs.length > 1}
		<nav class="mb-4 text-sm text-base-content/50" aria-label="Breadcrumb">
			{#each crumbs as crumb, i}
				{#if i > 0}<span class="mx-1 text-base-content/30">/</span>{/if}
				{#if crumb.href}
					<a class="hover:text-primary hover:underline" href={crumb.href}>{crumb.label}</a>
				{:else}
					<span class="text-base-content/80">{crumb.label}</span>
				{/if}
			{/each}
		</nav>
	{/if}
	<div class="prose-md">
		{@render children()}
	</div>
</div>
