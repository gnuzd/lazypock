// End-to-end tests against the real server and the Studio dev server.
//
// These exercise the contract between the Studio (through `lazypock-ts`) and this
// server: the flat rule payload, field order, delete semantics, codegen with an API
// key, and a browser login. The server and Studio are booted by
// `playwright.config.ts`.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, test } from "@playwright/test";

const API = "http://127.0.0.1:4000/api";
const EMAIL = process.env.LAZYPOCK_E2E_EMAIL ?? "e2e@example.com";
const PASSWORD = process.env.LAZYPOCK_E2E_PASSWORD ?? "e2e-password-123";
const CLI = join(process.cwd(), "..", "..", "lazypock-ts", "dist", "cli.js");

let token = "";

/** The superuser's bearer headers. */
function authed(): Record<string, string> {
	return {
		authorization: `Bearer ${token}`,
		"content-type": "application/json",
	};
}

/** Create the first superuser if there is none, then log in. */
async function loginOrSetup(): Promise<string> {
	const check = (await fetch(`${API}/superusers/check`).then((r) => r.json())) as {
		has_superuser?: boolean;
	};
	if (check.has_superuser === false) {
		const setup = await fetch(`${API}/superusers/setup`, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ email: EMAIL, password: PASSWORD }),
		});
		if (!setup.ok && setup.status !== 409) {
			throw new Error(`setup failed (${setup.status}): ${await setup.text()}`);
		}
	}

	const login = await fetch(`${API}/collections/_superusers/auth-with-password`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ identity: EMAIL, password: PASSWORD }),
	});
	if (!login.ok) {
		throw new Error(
			`login failed (${login.status}); set LAZYPOCK_E2E_EMAIL/PASSWORD to an ` +
				`existing superuser of this database`,
		);
	}
	return ((await login.json()) as { token: string }).token;
}

test.beforeAll(async () => {
	token = await loginOrSetup();
});

test("the collection contract the Studio relies on works end to end", async () => {
	const name = `e2e_${Date.now().toString(36)}`;

	// Create with the flat rule keys Studio sends, not the nested shape.
	const created = await fetch(`${API}/collections`, {
		method: "POST",
		headers: authed(),
		body: JSON.stringify({
			name,
			type: "base",
			fields: [
				{ name: "title", type: "text" },
				{ name: "views", type: "number" },
			],
			listRule: "",
			createRule: "",
			indexes: [],
		}),
	});
	expect(created.status).toBe(201);
	const collection = (await created.json()) as {
		rules: Record<string, unknown>;
		fields: { name: string; sort_order: number }[];
	};
	expect(collection.rules.listRule).toBe("");
	expect(collection.fields[0].sort_order).toBe(0);

	// A public rule is enforced, not merely stored: an anonymous create and list
	// both succeed.
	const record = await fetch(`${API}/collections/${name}/records`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ title: "hello" }),
	});
	expect(record.status).toBe(201);
	const listed = await fetch(`${API}/collections/${name}/records`);
	expect(listed.status).toBe(200);
	expect((await listed.json()).totalItems).toBe(1);

	// Field order round-trips as `sort_order`.
	const reordered = await fetch(`${API}/collections/${name}`, {
		method: "PATCH",
		headers: authed(),
		body: JSON.stringify({
			fields: [
				{ name: "views", type: "number" },
				{ name: "title", type: "text" },
			],
		}),
	});
	expect(reordered.status).toBe(200);
	const updated = (await reordered.json()) as {
		fields: { name: string; sort_order: number }[];
	};
	expect(updated.fields[0].name).toBe("views");
	expect(updated.fields[0].sort_order).toBe(0);

	// Delete drops the table, so the same name can be created again.
	expect(
		(await fetch(`${API}/collections/${name}`, { method: "DELETE", headers: authed() })).status,
	).toBe(200);
	const recreated = await fetch(`${API}/collections`, {
		method: "POST",
		headers: authed(),
		body: JSON.stringify({
			name,
			type: "base",
			fields: [{ name: "title", type: "text" }],
			rules: {},
		}),
	});
	expect(recreated.status).toBe(201);
	await fetch(`${API}/collections/${name}`, { method: "DELETE", headers: authed() });
});

test("codegen runs with an API key", async () => {
	const name = `e2e_gen_${Date.now().toString(36)}`;
	const created = await fetch(`${API}/collections`, {
		method: "POST",
		headers: authed(),
		body: JSON.stringify({
			name,
			type: "base",
			fields: [{ name: "title", type: "text" }],
			rules: {},
		}),
	});
	expect(created.status).toBe(201);

	const keyRes = await fetch(`${API}/settings/api-keys`, {
		method: "POST",
		headers: authed(),
		body: JSON.stringify({}),
	});
	expect(keyRes.status).toBe(200);
	const { api_key: apiKey } = (await keyRes.json()) as { api_key: string };
	expect(apiKey).toMatch(/^lazypock_/);

	// The SDK's codegen CLI, authenticated only by the API key.
	const out = join(mkdtempSync(join(tmpdir(), "lazypock-codegen-")), "lazypock.types.ts");
	execFileSync("node", [CLI, "--url", API, "--apikey", apiKey, "--output", out], {
		encoding: "utf8",
	});
	expect(readFileSync(out, "utf8")).toContain(name);

	await fetch(`${API}/collections/${name}`, { method: "DELETE", headers: authed() });
});

test("the Studio loads, logs in, and renders the collections page", async ({ page }) => {
	// The Studio is served under SvelteKit's `paths.base` of `/_`.
	await page.goto("/_/login");

	// A fresh database shows the setup form; otherwise the login form.
	if (await page.locator("#setup-email").count()) {
		await page.fill("#setup-email", EMAIL);
		await page.fill("#setup-password", PASSWORD);
		await page.fill("#setup-confirm", PASSWORD);
		await page.getByRole("button", { name: "Create Superuser" }).click();
	} else {
		await page.fill("#email", EMAIL);
		await page.fill("#password", PASSWORD);
		await page.getByRole("button", { name: "Sign in" }).click();
	}

	await page.waitForURL(/\/collections/);
	await expect(page.getByRole("button", { name: /New Collection/ })).toBeVisible();
});
