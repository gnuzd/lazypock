<script lang="ts">
	import { page } from '$app/state';
	import Seo from '$lib/components/Seo.svelte';
	import { sdkNav } from '$lib/sdk-nav.generated';
	import { docTitle, sdkDescription } from '$lib/seo';

	let { children } = $props();

	// e.g. ['sdk', 'typescript'] — each SDK is a single scrollable page now.
	const segments = $derived(page.url.pathname.split('/').filter(Boolean));
	const sdk = $derived(sdkNav.find((s) => s.slug === segments[1]));

	const crumbs = $derived.by(() => {
		const parts: { label: string; href?: string }[] = [{ label: 'SDKs', href: '/sdk' }];
		if (sdk) parts.push({ label: sdk.name, href: `/sdk/${sdk.slug}` });
		return parts;
	});

	// Markdown pages can't set <title> from frontmatter, so the title/description
	// come from `sdkNav` (generated) plus the prose map in $lib/seo — rendered
	// server-side, so crawlers and link previews see them. Pages not in `sdkNav`
	// (the "coming soon" SDKs) set their own metadata.
	const title = $derived(sdk ? docTitle(`${sdk.name} SDK`) : '');
	const description = $derived(sdk ? sdkDescription(sdk.slug, sdk.name) : '');
</script>

{#if sdk}
	<Seo {title} {description} />
{/if}

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
