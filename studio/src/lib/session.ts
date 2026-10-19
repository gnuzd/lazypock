import type { AuthModel } from 'lazypock';

import { client } from '$lib/client';

/**
 * The Studio's session: the access token's short life, and the refresh token that
 * outlives it.
 *
 * The server issues a **15-minute** access token and a **30-day** refresh token
 * (`crates/auth/src/token.rs`, `crates/auth/src/sessions.rs`). The published SDK
 * stores neither refresh token nor the server's `expiresIn`: it assumes a 7-day
 * token (the old Phoenix server), and its own refresh path sends the *access* token
 * to `auth-refresh`, which the server refuses. So a Studio session died when the
 * 15 minutes ran out, and the layout's `client.me()` turned that into a redirect to
 * `/login` — "logged in, then it expired".
 *
 * This module keeps the refresh token, refreshes just before the access token
 * lapses, and retries a request once after a 401. The SDK owns the token and the
 * record; only the refresh material lives here.
 *
 * TODO(sdk): when `lazypock` (the SDK) stores the refresh token and refreshes from
 * `expiresIn` itself, this module can go away and the client's wrapper can simply
 * call `authStore.refresh()`. Until that version is published, the Studio needs it.
 */

const REFRESH_KEY = 'lazypock_refresh_token';
const EXPIRES_AT_KEY = 'lazypock_access_expires_at';
/** Refresh this long before the access token actually expires. */
const REFRESH_MARGIN_MS = 60_000;
/** The path the server serves superuser auth on, and the one used when a session
 * has no collection (a superuser login). */
const SUPERUSERS = '_superusers';
/** Cross-tab lock, so two tabs do not rotate the same refresh token at once (the
 * second use would be a replay and the server would revoke the whole family). */
const REFRESH_LOCK = 'lazypock-session-refresh';

/** The subset of the server's auth response this module needs. */
export type AuthResponse = {
	token?: unknown;
	record?: unknown;
	refreshToken?: unknown;
	expiresIn?: unknown;
};

function storage(): Storage | null {
	try {
		return localStorage;
	} catch {
		return null;
	}
}

function read(key: string): string | null {
	try {
		return storage()?.getItem(key) ?? null;
	} catch {
		return null;
	}
}

function write(key: string, value: string): void {
	try {
		storage()?.setItem(key, value);
	} catch {
		// storage unavailable (private mode); the session simply does not survive a reload
	}
}

function remove(key: string): void {
	try {
		storage()?.removeItem(key);
	} catch {
		// ignore
	}
}

/**
 * Remember the refresh half of a successful login, setup or refresh.
 *
 * The token and record are already in the SDK's auth store by the time this is
 * called; `expiresIn` is the server's answer (seconds), which is the only reliable
 * lifetime — the SDK's own constant is the old server's.
 */
export function rememberSession(body: AuthResponse | null | undefined): void {
	if (!body || typeof body.token !== 'string' || !body.token) return;

	const refreshToken = typeof body.refreshToken === 'string' ? body.refreshToken : '';
	if (refreshToken) write(REFRESH_KEY, refreshToken);

	const expiresIn = typeof body.expiresIn === 'number' ? body.expiresIn : 0;
	if (expiresIn > 0) {
		write(EXPIRES_AT_KEY, String(Date.now() + expiresIn * 1000 - REFRESH_MARGIN_MS));
	}
}

/** Drop the refresh material (logout, or a refresh the server refused). */
export function forgetSession(): void {
	remove(REFRESH_KEY);
	remove(EXPIRES_AT_KEY);
}

/** Whether the access token is at, or past, its refresh margin. */
function needsRefresh(): boolean {
	if (!read(REFRESH_KEY)) return false;
	const expiresAt = Number(read(EXPIRES_AT_KEY) ?? 0);
	// No recorded expiry means a session from before this module (or a login response
	// without `expiresIn`): let the server answer with a 401 and use that path.
	return Number.isFinite(expiresAt) && expiresAt > 0 && Date.now() >= expiresAt;
}

/** The auth collection a refresh must be addressed to. */
function refreshCollection(): string {
	const store = client.authStore;
	if (store.collectionName) return store.collectionName;
	const model = store.model as { collectionName?: unknown } | null;
	if (model && typeof model.collectionName === 'string' && model.collectionName) {
		return model.collectionName;
	}
	// A superuser session has no collection; the server serves it on this path.
	return SUPERUSERS;
}

/** Authenticate a refresh response in the SDK store, keeping its collection set. */
function adoptSession(body: Record<string, unknown>, collection: string): void {
	const record =
		body.record && typeof body.record === 'object'
			? (body.record as Record<string, unknown>)
			: null;
	const name =
		record && typeof record.collectionName === 'string' && record.collectionName
			? record.collectionName
			: collection === SUPERUSERS
				? null
				: collection;
	client.authStore.setCollectionName(name);
	// SAFETY: the server's auth response record is a superset of `AuthModel`, the same
	// cast the SDK's own login path makes.
	client.authStore.set(body.token as string, record as unknown as AuthModel);
	rememberSession(body);
}

let inFlight: Promise<boolean> | null = null;

/**
 * Exchange the refresh token for a new access token.
 *
 * Serialised across tabs with the Web Locks API when it exists: two tabs rotating
 * the same refresh token at once would make the second use a replay, and the server
 * revokes the whole family — logging both tabs out. Inside the lock the expiry is
 * re-read, so a tab that lost the race simply sees the fresh session.
 */
export async function refreshSession(): Promise<boolean> {
	if (inFlight) return inFlight;

	const locks = (
		globalThis as {
			navigator?: { locks?: { request?: (name: string, fn: () => Promise<boolean>) => Promise<boolean> } };
		}
	).navigator?.locks;
	const run = () => refreshLocked();

	inFlight = (locks?.request ? locks.request(REFRESH_LOCK, run) : run()).finally(() => {
		inFlight = null;
	});
	return inFlight;
}

async function refreshLocked(): Promise<boolean> {
	if (!needsRefresh()) {
		// Another tab refreshed while this one waited for the lock.
		return !!client.authStore.token;
	}

	const refreshToken = read(REFRESH_KEY);
	if (!refreshToken) return false;

	const collection = refreshCollection();
	try {
		const response = await fetch(
			`${client.http.baseUrl}/collections/${encodeURIComponent(collection)}/auth-refresh`,
			{
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ refreshToken })
			}
		);

		if (!response.ok) {
			// The refresh token is gone, expired, revoked or replayed. There is no
			// session left to keep, and pretending otherwise would leave the Studio on a
			// token every request rejects.
			if (response.status === 400 || response.status === 401) {
				forgetSession();
				client.authStore.clear();
			}
			return false;
		}

		const body = (await response.json()) as Record<string, unknown>;
		if (typeof body.token !== 'string' || !body.token) {
			forgetSession();
			client.authStore.clear();
			return false;
		}
		adoptSession(body, collection);
		return true;
	} catch {
		// A network failure is not a rejection: keep the session so the next request can
		// try again rather than signing the operator out over a blip.
		return false;
	}
}

/** Refresh first if the access token is about to lapse. */
export async function ensureFreshSession(): Promise<void> {
	if (needsRefresh()) await refreshSession();
}

/** Refresh after a 401, so the caller can retry the request once. */
export async function refreshAfterUnauthorized(): Promise<boolean> {
	if (!read(REFRESH_KEY)) return false;
	// The 401 already proves the token is dead, so ignore the recorded expiry.
	write(EXPIRES_AT_KEY, '1');
	return refreshSession();
}
