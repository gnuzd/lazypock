import type { RequestHandler } from "./$types";
import { nav } from "$lib/nav";
import { SITE_URL } from "$lib/seo";

// Prerendered at build time, so Cloudflare serves it as a plain static asset
// instead of waking the worker for every crawler request.
export const prerender = true;

/**
 * Pages reachable only from the top navbar — they have no sidebar entry, so
 * they can't be derived from `nav`.
 */
const NAVBAR_ONLY = ["/sdk"];

/**
 * Derived from `nav` — the sidebar *is* the site map, so a new page shows up
 * here without a second list to maintain.
 *
 * Anchors (`/server#images`) collapse to their page, and entries still marked
 * "coming soon" are left out until they have real content.
 */
const paths = [
	...new Set([
		...NAVBAR_ONLY,
		...nav
			.flatMap((section) => section.items)
			.filter((item) => !item.badge)
			.map((item) => item.href.split("#")[0]),
	]),
].sort();

export const GET: RequestHandler = () => {
	const urls = paths
		.map((path) => `  <url><loc>${new URL(path, SITE_URL).href}</loc></url>`)
		.join("\n");

	return new Response(
		`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`,
		{ headers: { "content-type": "application/xml; charset=utf-8" } },
	);
};
