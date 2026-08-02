#!/usr/bin/env node
// ── Codegen CLI ─────────────────────────────────────────
// `lazypock-gen` — fetches the live collection schema from a Lazypock
// API and writes a fully-typed `lazypock.types.ts` module.
//
// Usage:
//   npx lazypock-gen --url http://localhost:4000/api --email admin@... --password ...
//
// Or via env vars (no flags):
//   LAZYPOCK_URL=... LAZYPOCK_EMAIL=... LAZYPOCK_PASSWORD=... npx lazypock-gen

import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { generateTypes } from "./codegen";
import type { CollectionsResponse } from "./schema";

interface CliOptions {
	url: string;
	email: string;
	password: string;
	out: string;
	packageName: string;
	skipSystem: boolean;
}

function fail(msg: string): never {
	console.error(`\n❌ ${msg}\n`);
	process.exit(1);
}

function parseArgs(argv: string[]): CliOptions {
	const args = [...argv];
	const get = (flag: string, envKey: string, def = ""): string => {
		const i = args.indexOf(flag);
		if (i !== -1 && i + 1 < args.length) return args[i + 1];
		return process.env[envKey] ?? def;
	};
	const has = (flag: string): boolean => args.includes(flag);

	const url = get("--url", "LAZYPOCK_URL");
	const email = get("--email", "LAZYPOCK_EMAIL");
	const password = get("--password", "LAZYPOCK_PASSWORD");

	if (!url) fail("Missing API URL. Pass --url or set LAZYPOCK_URL.");
	if (!email)
		fail("Missing superuser email. Pass --email or set LAZYPOCK_EMAIL.");
	if (!password)
		fail("Missing password. Pass --password or set LAZYPOCK_PASSWORD.");

	const out = get("--out", "LAZYPOCK_OUT", "lazypock.types.ts");
	const packageName = get("--package", "LAZYPOCK_PACKAGE", "lazypock");

	return {
		url,
		email,
		password,
		out,
		packageName,
		skipSystem: has("--skip-system"),
	};
}

async function fetchCollections(
	opts: CliOptions,
): Promise<CollectionsResponse> {
	// Step 1: login as superuser to get a token
	const loginUrl = opts.url.replace(/\/+$/, "") + "/superusers/login";
	const loginRes = await fetch(loginUrl, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ email: opts.email, password: opts.password }),
	});
	if (!loginRes.ok) {
		const text = await loginRes.text();
		fail(`Superuser login failed (${loginRes.status}). ${text.slice(0, 200)}`);
	}
	const loginData = (await loginRes.json()) as { token?: string };
	if (!loginData.token) fail("Login response did not include a token.");

	// Step 2: fetch collections
	const collUrl = opts.url.replace(/\/+$/, "") + "/collections";
	const collRes = await fetch(collUrl, {
		headers: { Authorization: "Bearer " + loginData.token },
	});
	if (!collRes.ok) {
		const text = await collRes.text();
		fail(
			`Failed to fetch collections (${collRes.status}). ${text.slice(0, 200)}`,
		);
	}
	return (await collRes.json()) as CollectionsResponse;
}

async function main(): Promise<void> {
	const opts = parseArgs(process.argv.slice(2));
	console.log(`\n🔌 Connecting to ${opts.url} as ${opts.email} …`);

	const { items } = await fetchCollections(opts);
	console.log(`📦 Found ${items.length} collection(s).`);

	const source = generateTypes(items, {
		packageName: opts.packageName,
		skipSystem: opts.skipSystem,
	});

	const outPath = resolve(process.cwd(), opts.out);
	await writeFile(outPath, source, "utf8");
	console.log(`✅ Wrote ${outPath} (${source.length} bytes).`);
	console.log(
		`\nImport it in your app:\n  import { createClient } from './${opts.out.replace(/\.ts$/, "")}';\n`,
	);
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
