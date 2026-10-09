<script lang="ts">
	import { page } from "$app/state";
import {
		OG_IMAGE,
		SITE_NAME,
		SITE_TAGLINE,
		SITE_URL,
} from "$lib/seo";
import { jsonLdScript, techArticleJsonLd, type JsonLd } from "$lib/json-ld";

	let {
		title,
		description,
		/** `article` for content pages, `website` for the landing page. */
		kind = "article",
		/** Extra JSON-LD blocks (the landing page adds WebSite + SoftwareApplication). */
		jsonLd = [],
	}: {
		title: string;
		description: string;
		kind?: "article" | "website";
		jsonLd?: JsonLd[];
	} = $props();

	// Canonical drops the fragment and any trailing slash (`/sdk/` -> `/sdk`).
	const path = $derived(page.url.pathname.replace(/\/$/, ""));
	const canonical = $derived(`${SITE_URL}${path}`);

	const structuredData = $derived([
		...(kind === "article" ? [techArticleJsonLd(title, description, path)] : []),
		...jsonLd,
	]);
</script>

<svelte:head>
	<title>{title}</title>
	<meta name="description" content={description} />
	<link rel="canonical" href={canonical} />

	<meta property="og:type" content={kind} />
	<meta property="og:site_name" content={SITE_NAME} />
	<meta property="og:title" content={title} />
	<meta property="og:description" content={description} />
	<meta property="og:url" content={canonical} />
	<meta property="og:image" content={OG_IMAGE} />
	<meta property="og:image:width" content="1200" />
	<meta property="og:image:height" content="630" />
	<meta property="og:image:alt" content={`${SITE_NAME} — ${SITE_TAGLINE}`} />

	<meta name="twitter:card" content="summary_large_image" />
	<meta name="twitter:title" content={title} />
	<meta name="twitter:description" content={description} />
	<meta name="twitter:image" content={OG_IMAGE} />

	<meta name="theme-color" content="#2f5233" />

	{#each structuredData as data}
		{@html jsonLdScript(data)}
	{/each}
</svelte:head>
