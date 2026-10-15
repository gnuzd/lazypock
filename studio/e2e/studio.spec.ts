// End-to-end tests against the real server and the Studio dev server.
//
// These exercise the contract between the Studio (through `lazypock-ts`) and this
// server: the flat rule payload, field order, delete semantics, codegen with an API
// key, and a browser login. The server and Studio are booted by
// `playwright.config.ts`.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, test, type Page } from '@playwright/test';

const API = 'http://127.0.0.1:4000/api';
const EMAIL = process.env.LAZYPOCK_E2E_EMAIL ?? 'e2e@example.com';
const PASSWORD = process.env.LAZYPOCK_E2E_PASSWORD ?? 'e2e-password-123';
const CLI = join(process.cwd(), '..', '..', 'lazypock-ts', 'dist', 'cli.js');

let token = '';

/** The superuser's bearer headers. */
function authed(): Record<string, string> {
	return {
		authorization: `Bearer ${token}`,
		'content-type': 'application/json'
	};
}

/** Create the first superuser if there is none, then log in. */
async function loginOrSetup(): Promise<string> {
	const check = (await fetch(`${API}/superusers/check`).then((r) => r.json())) as {
		has_superuser?: boolean;
	};
	if (check.has_superuser === false) {
		const setup = await fetch(`${API}/superusers/setup`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ email: EMAIL, password: PASSWORD })
		});
		if (!setup.ok && setup.status !== 409) {
			throw new Error(`setup failed (${setup.status}): ${await setup.text()}`);
		}
	}

	const login = await fetch(`${API}/collections/_superusers/auth-with-password`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ identity: EMAIL, password: PASSWORD })
	});
	if (!login.ok) {
		throw new Error(
			`login failed (${login.status}); set LAZYPOCK_E2E_EMAIL/PASSWORD to an ` +
				`existing superuser of this database`
		);
	}
	return ((await login.json()) as { token: string }).token;
}

/**
 * Sign in through the UI, creating the first superuser when the database is
 * fresh. Leaves the browser on `/collections`.
 */
async function signIn(page: Page): Promise<void> {
	// The Studio is served under SvelteKit's `paths.base` of `/_`.
	await page.goto('/_/login');

	if (await page.locator('#setup-email').count()) {
		await page.fill('#setup-email', EMAIL);
		await page.fill('#setup-password', PASSWORD);
		await page.fill('#setup-confirm', PASSWORD);
		await page.getByRole('button', { name: 'Create Superuser' }).click();
	} else {
		await page.fill('#email', EMAIL);
		await page.fill('#password', PASSWORD);
		await page.getByRole('button', { name: 'Sign in' }).click();
	}

	await page.waitForURL(/\/collections/);
}

/** Create a collection (one text field, public rules) through the superuser API. */
async function createCollectionViaApi(name: string, field = 'title'): Promise<void> {
	const response = await fetch(`${API}/collections`, {
		method: 'POST',
		headers: authed(),
		body: JSON.stringify({
			name,
			type: 'base',
			fields: [{ name: field, type: 'text' }],
			listRule: '',
			viewRule: '',
			createRule: '',
			updateRule: '',
			deleteRule: '',
			indexes: []
		})
	});
	expect(response.status, await response.text()).toBe(201);
}

/** Remove a collection through the superuser API. */
async function deleteCollectionViaApi(name: string): Promise<void> {
	await fetch(`${API}/collections/${name}`, { method: 'DELETE', headers: authed() });
}

test.beforeAll(async () => {
	token = await loginOrSetup();
});

test('the collection contract the Studio relies on works end to end', async () => {
	const name = `e2e_${Date.now().toString(36)}`;

	// Create with the flat rule keys Studio sends, not the nested shape.
	const created = await fetch(`${API}/collections`, {
		method: 'POST',
		headers: authed(),
		body: JSON.stringify({
			name,
			type: 'base',
			fields: [
				{ name: 'title', type: 'text' },
				{ name: 'views', type: 'number' }
			],
			listRule: '',
			createRule: '',
			indexes: []
		})
	});
	expect(created.status).toBe(201);
	const collection = (await created.json()) as {
		rules: Record<string, unknown>;
		fields: { name: string; sort_order: number }[];
	};
	expect(collection.rules.listRule).toBe('');
	expect(collection.fields[0].sort_order).toBe(0);

	// A public rule is enforced, not merely stored: an anonymous create and list
	// both succeed.
	const record = await fetch(`${API}/collections/${name}/records`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ title: 'hello' })
	});
	expect(record.status).toBe(201);
	const listed = await fetch(`${API}/collections/${name}/records`);
	expect(listed.status).toBe(200);
	expect((await listed.json()).totalItems).toBe(1);

	// Field order round-trips as `sort_order`.
	const reordered = await fetch(`${API}/collections/${name}`, {
		method: 'PATCH',
		headers: authed(),
		body: JSON.stringify({
			fields: [
				{ name: 'views', type: 'number' },
				{ name: 'title', type: 'text' }
			]
		})
	});
	expect(reordered.status).toBe(200);
	const updated = (await reordered.json()) as {
		fields: { name: string; sort_order: number }[];
	};
	expect(updated.fields[0].name).toBe('views');
	expect(updated.fields[0].sort_order).toBe(0);

	// Delete drops the table, so the same name can be created again.
	expect(
		(await fetch(`${API}/collections/${name}`, { method: 'DELETE', headers: authed() })).status
	).toBe(200);
	const recreated = await fetch(`${API}/collections`, {
		method: 'POST',
		headers: authed(),
		body: JSON.stringify({
			name,
			type: 'base',
			fields: [{ name: 'title', type: 'text' }],
			rules: {}
		})
	});
	expect(recreated.status).toBe(201);
	await fetch(`${API}/collections/${name}`, { method: 'DELETE', headers: authed() });
});

test('codegen runs with an API key', async () => {
	const name = `e2e_gen_${Date.now().toString(36)}`;
	const created = await fetch(`${API}/collections`, {
		method: 'POST',
		headers: authed(),
		body: JSON.stringify({
			name,
			type: 'base',
			fields: [{ name: 'title', type: 'text' }],
			rules: {}
		})
	});
	expect(created.status).toBe(201);

	const keyRes = await fetch(`${API}/settings/api-keys`, {
		method: 'POST',
		headers: authed(),
		body: JSON.stringify({})
	});
	expect(keyRes.status).toBe(200);
	const { api_key: apiKey } = (await keyRes.json()) as { api_key: string };
	expect(apiKey).toMatch(/^lazypock_/);

	// The SDK's codegen CLI, authenticated only by the API key.
	const out = join(mkdtempSync(join(tmpdir(), 'lazypock-codegen-')), 'lazypock.types.ts');
	execFileSync('node', [CLI, '--url', API, '--apikey', apiKey, '--output', out], {
		encoding: 'utf8'
	});
	expect(readFileSync(out, 'utf8')).toContain(name);

	await fetch(`${API}/collections/${name}`, { method: 'DELETE', headers: authed() });
});

test('the Studio loads, logs in, and renders the collections page', async ({ page }) => {
	await signIn(page);
	await expect(page.getByRole('button', { name: /New Collection/ })).toBeVisible();
});

test('the collections sidebar counts columns and lists the system collections', async ({ page }) => {
	await signIn(page);
	const name = `e2e_cols_${Date.now().toString(36)}`;
	// One text field, so the sidebar count is deterministic.
	await createCollectionViaApi(name);
	try {
		await page.goto(`/_/collections?collection=${name}`);

		// The sidebar row is the collection name followed by its column count.
		// The server has to serve the field list for that count: before it did,
		// every collection rendered "0". Asserting a positive number is what
		// catches the regression, so the check is on the number, not the name.
		const row = page.locator('div[role="button"]').filter({ hasText: name }).first();
		await expect(row).toContainText(name);
		await expect(row.locator('span').last()).toHaveText(/^[1-9]\d*$/);

		// The server presents the PocketBase system collections under System.
		await page.getByText(/^System \(/).click();
		for (const system of ['_superusers', '_auth_origins', '_external_auths', '_mfas', '_otps']) {
			await expect(page.getByText(system, { exact: true })).toBeVisible();
		}
		// The registry is not one of them.
		await expect(page.getByText('_collections', { exact: true })).toHaveCount(0);
	} finally {
		await deleteCollectionViaApi(name);
	}
});

test('every Studio page loads for a superuser', async ({ page }) => {
	await signIn(page);

	// Each settings page carries a heading with its name, so a broken page (a
	// failed data load, a template error) fails the assertion rather than a
	// generic "did something render" check.
	const pages: [string, string][] = [
		['/settings/application', 'Application Settings'],
		['/settings/mail', 'Mail Settings'],
		['/settings/files', 'Files Storage'],
		['/settings/media', 'Media Library'],
		['/settings/api-keys', 'API Keys'],
		['/settings/backups', 'Backups'],
		['/settings/cron', 'Cron Jobs'],
		['/settings/export', 'Export Collections'],
		['/settings/import', 'Import Collections'],
		['/settings/sql', 'SQL Console'],
		['/logs', 'Request Logs']
	];

	for (const [route, heading] of pages) {
		await test.step(route, async () => {
			await page.goto(`/_${route}`);
			await expect(page.getByRole('heading', { name: heading })).toBeVisible();
		});
	}
});

test('a collection can be created through the UI', async ({ page }) => {
	await signIn(page);
	const name = `e2e_ui_${Date.now().toString(36)}`;
	try {
		await page.goto('/_/collections/new');
		await page.fill('#coll-name', name);
		await page.getByRole('button', { name: 'Create', exact: true }).click();

		// Saving closes the editor and the new collection becomes active.
		await expect(page.getByText(name, { exact: true }).first()).toBeVisible();
	} finally {
		await deleteCollectionViaApi(name);
	}
});

test('a record can be created through the UI', async ({ page }) => {
	await signIn(page);
	const name = `e2e_rec_${Date.now().toString(36)}`;
	await createCollectionViaApi(name);
	try {
		await page.goto(`/_/collections?collection=${name}`);
		await page
			.getByRole('button', { name: /New Record/ })
			.first()
			.click();
		await page.locator('#f_title').fill('hello from the tour');
		await page.getByRole('button', { name: 'Create', exact: true }).click();

		await expect(page.getByText('hello from the tour')).toBeVisible();
	} finally {
		await deleteCollectionViaApi(name);
	}
});

test('an API key can be generated and revoked through the UI', async ({ page }) => {
	await signIn(page);
	await page.goto('/_/settings/api-keys');

	await page.getByRole('button', { name: 'Generate API key' }).click();
	await expect(page.locator('code').filter({ hasText: /^lazypock_/ })).toBeVisible();

	await page.getByRole('button', { name: 'Revoke' }).first().click();
	await expect(page.getByText('revoked').first()).toBeVisible();
});

// `fixme`: the Studio's SQL console posts to `/sql/query`, but the Rust server
// has no SQL endpoint at all — the page renders and any query 404s ("The
// requested resource was not found"). Implementing it (superuser-only, and
// read-only as the page promises) is a separate change; PLAN.md records the
// gap. Drop the `fixme` with the endpoint.
test.fixme('the SQL console runs a query', async ({ page }) => {
	await signIn(page);
	await page.goto('/_/settings/sql');

	await page.locator('textarea').fill('select 1 as the_answer');
	await page.getByRole('button', { name: 'Run Query' }).click();

	await expect(page.getByRole('columnheader', { name: 'the_answer' })).toBeVisible();
	await expect(page.getByRole('cell', { name: '1', exact: true })).toBeVisible();
});

test('the import page parses a pasted collection', async ({ page }) => {
	await signIn(page);
	const name = `e2e_imp_${Date.now().toString(36)}`;
	await page.goto('/_/settings/import');

	await page
		.locator('#import-schemas')
		.fill(JSON.stringify([{ name, type: 'base', fields: [{ name: 'title', type: 'text' }] }]));

	await expect(page.getByText('Detected changes')).toBeVisible();
	await expect(page.getByText('Added', { exact: true })).toBeVisible();
	await expect(page.getByText(name, { exact: true })).toBeVisible();
});

test('a cron job can be created and deleted through the UI', async ({ page }) => {
	await signIn(page);
	const name = `e2e_cron_${Date.now().toString(36)}`;
	await page.goto('/_/settings/cron');

	await page.getByRole('button', { name: 'New cron job' }).click();
	await page.getByPlaceholder('e.g. nightly cleanup').fill(name);
	await page.getByPlaceholder('https://example.com/webhook').fill('https://example.com/hook');
	await page.getByRole('button', { name: 'Create job' }).click();

	const row = page.getByRole('row').filter({ hasText: name });
	await expect(row).toBeVisible();

	await row.getByTitle('Delete').click();
	await page.getByRole('dialog').getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(page.getByRole('row').filter({ hasText: name })).toHaveCount(0);
});
