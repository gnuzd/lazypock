// A Studio session must outlive the access token.
//
// The server issues a 15-minute access token and a 30-day refresh token. The
// published SDK stores no refresh token and assumes a 7-day token, so before this
// the Studio's session died at the 15-minute mark: the next request 401'd, the
// layout's `client.me()` failed, and the operator was sent to `/login`.
//
// Waiting 15 minutes is not a test. Instead the session's recorded expiry is pushed
// into the past (what the passage of time does) and, in the second case, the access
// token is replaced with a worthless one (what the server rejecting it looks like).
// Both must end with the session still working and a *rotated* refresh token, which
// only happens if the server's `auth-refresh` really ran.

import { expect, test, type Page } from '@playwright/test';

const EMAIL = process.env.LAZYPOCK_E2E_EMAIL ?? 'e2e@example.com';
const PASSWORD = process.env.LAZYPOCK_E2E_PASSWORD ?? 'e2e-password-123';
const EXPIRES_KEY = 'lazypock_access_expires_at';
const REFRESH_KEY = 'lazypock_refresh_token';
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

	// The access token is past its margin: exactly what 15 minutes of idling does.
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
	await page.evaluate((key) => localStorage.setItem(key, String(Date.now() + 600_000)), EXPIRES_KEY);

	await reloadCollections(page);

	await expect
		.poll(() => stored(page, REFRESH_KEY), { message: 'the refresh token must rotate' })
		.not.toBe(refreshToken);
	await expect(page).not.toHaveURL(/\/login/);
});
