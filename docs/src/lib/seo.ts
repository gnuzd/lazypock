/**
 * Site-wide SEO constants and per-page prose for the docs site.
 *
 * The docs site is server-rendered (nothing sets `ssr = false`), so every tag
 * rendered by `Seo.svelte` lands in the HTML that crawlers and social scrapers
 * see. That is also why per-page metadata lives in `+page.svelte` /
 * `+layout.svelte` instead of being set from `onMount` — a client-side title is
 * invisible to both. Structured data lives in `json-ld.ts`.
 */

/** Canonical origin — the host used by robots.txt and llms.txt too. */
export const SITE_URL = "https://lazypock.gnuzd.dev";
export const SITE_NAME = "Lazypock";
export const SITE_TAGLINE = "PocketBase-compatible backend on PostgreSQL";
export const REPO_URL = "https://github.com/gnuzd/lazypock";
export const OG_IMAGE = `${SITE_URL}/og.png`;

/** `<title>` suffix shared by every docs page. */
export const docTitle = (name: string) => `${name} — Lazypock Docs`;

/**
 * One-line summaries for the SDK pages that have real docs. SDK names come from
 * `sdkNav` (generated), so only the prose lives here.
 */
const SDK_DESCRIPTIONS: Record<string, string> = {
	typescript:
		"The official TypeScript client for Lazypock: type-safe codegen from your schema, typed queries and filters, realtime subscriptions, file uploads and auth.",
};

export function sdkDescription(slug: string, name: string): string {
	return (
		SDK_DESCRIPTIONS[slug] ??
		`Client documentation for talking to a Lazypock backend from ${name}: install, authentication, queries, realtime subscriptions and file uploads.`
	);
}
