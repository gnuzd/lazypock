// A Studio session must outlive the access token.
//
// `lazypock` 0.20 owns this: the auth store keeps the refresh token and the server's
// `expiresIn`, refreshes before a request when the access token is near expiry, and
// retries once after a 401. The Studio deliberately keeps no second copy of the
// refresh token — two rotations of the same token family make the second use a
// replay, and the server revokes the whole family, signing both tabs out.
//
// Waiting 15 minutes is not a test. Instead the session's recorded expiry is pushed
// into the past (what the passage of time does) and, in the second case, the access
// token is replaced with a worthless one (what the server rejecting it looks like).
// Both must end with the session still working and a *rotated* refresh token, which
// only happens if the server's `auth-refresh` really ran.

import { expect, test, type Page } from '@playwright/test';

const EMAIL = process.env.LAZYPOCK_E2E_EMAIL ?? 'e2e@example.com';
const PASSWORD = process.env.LAZYPOCK_E2E_PASSWORD ?? 'e2e-password-123';
/** The SDK's own storage keys — the Studio keeps no session state of its own. */
const EXPIRES_KEY = 'auth_expires_at';
const REFRESH_KEY = 'auth_refresh_token';
const TOKEN_KEY = 'auth_token';

/** Sign in through the UI, creating the first superuser when the database is fresh. */
async function signIn(page: Page): Promise<void> {
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

async function stored(page: Page, key: string): Promise<string> {
	return (await page.evaluate((name) => localStorage.getItem(name), key)) ?? '';
}

/** A request the Studio makes on every page, so "still signed in" is observable. */
async function reloadCollections(page: Page): Promise<void> {
	await page.goto('/_/collections');
	await expect(page).not.toHaveURL(/\/login/);
}

test('an expired access token is refreshed instead of signing out', async ({ page }) => {
	await signIn(page);

	const refreshToken = await stored(page, REFRESH_KEY);
	expect(refreshToken, 'the login response must be remembered for a refresh').not.toBe('');
	expect(await stored(page, EXPIRES_KEY)).not.toBe('');

	// The access token is past its expiry: exactly what 15 minutes of idling does.
	await page.evaluate((key) => localStorage.setItem(key, String(Date.now() - 1_000)), EXPIRES_KEY);

	await reloadCollections(page);

	await expect
		.poll(() => stored(page, REFRESH_KEY), { message: 'the refresh token must rotate' })
		.not.toBe(refreshToken);
	expect(await stored(page, TOKEN_KEY)).not.toBe('');
});

test('a refused access token is refreshed and the request retried', async ({ page }) => {
	await signIn(page);

	const refreshToken = await stored(page, REFRESH_KEY);
	expect(refreshToken).not.toBe('');

	// A token the server will reject: the 401 path, without waiting for expiry.
	await page.evaluate((key) => localStorage.setItem(key, 'not-a-valid-token'), TOKEN_KEY);
	// Expiry says "not due", so only the 401 can trigger the refresh.
	await page.evaluate(
		(key) => localStorage.setItem(key, String(Date.now() + 600_000)),
		EXPIRES_KEY
	);

	await reloadCollections(page);

	await expect
		.poll(() => stored(page, REFRESH_KEY), { message: 'the refresh token must rotate' })
		.not.toBe(refreshToken);
	await expect(page).not.toHaveURL(/\/login/);
});
