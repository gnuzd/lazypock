/**
 * JSON-LD (schema.org) builders for the docs site.
 *
 * Kept apart from `seo.ts` (site identity and per-page prose) because these are
 * structured data: the landing page describes the product, every other page
 * describes itself as a technical article. `Seo.svelte` picks the right one and
 * injects it into `<svelte:head>`.
 */

import { SITE_NAME, SITE_URL } from "./seo";

/**
 * A JSON-LD node. The shape is open by design (schema.org has hundreds of
 * properties), so callers read through it rather than against a fixed type.
 */
export type JsonLd = Record<string, unknown>;

const LICENSE = "https://opensource.org/licenses/MIT";

export function websiteJsonLd(description: string): JsonLd {
	return {
		"@context": "https://schema.org",
		"@type": "WebSite",
		name: SITE_NAME,
		alternateName: "Lazypock Docs",
		url: SITE_URL,
		description,
	};
}

export function softwareApplicationJsonLd(description: string): JsonLd {
	return {
		"@context": "https://schema.org",
		"@type": "SoftwareApplication",
		name: SITE_NAME,
		applicationCategory: "DeveloperApplication",
		operatingSystem: "Linux, macOS, Docker",
		description,
		url: SITE_URL,
		codeRepository: "https://github.com/gnuzd/lazypock",
		license: LICENSE,
		offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
	};
}

export function techArticleJsonLd(
	title: string,
	description: string,
	path: string,
): JsonLd {
	const url = `${SITE_URL}${path}`;

	return {
		"@context": "https://schema.org",
		"@type": "TechArticle",
		headline: title,
		description,
		url,
		mainEntityOfPage: url,
		isPartOf: { "@type": "WebSite", name: SITE_NAME, url: SITE_URL },
		license: LICENSE,
	};
}

/**
 * Serializes a JSON-LD block for `<svelte:head>`.
 *
 * It lives here rather than in `Seo.svelte` on purpose: a literal `</script>`
 * inside a Svelte component's `<script>` block would close that block early.
 * Escaping `<` also keeps any embedded markup from terminating the tag.
 */
export function jsonLdScript(data: JsonLd): string {
	return `<script type="application/ld+json">${JSON.stringify(data).replace(
		/</g,
		"\\u003c",
	)}</script>`;
}
