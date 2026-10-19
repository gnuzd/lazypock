import { createClient } from './lazypock.types';
import { sseUrlFromBaseUrl, type HttpClient } from 'lazypock';
import { ensureFreshSession, refreshAfterUnauthorized } from './session';

// The studio SPA is served by the core Phoenix app, which proxies the
// Lazypock API under `/api`.
export const client = createClient({ baseUrl: '/api' });

// ── Per-tab connection id (realtime origin-exclusion) ────────────────────
// A stable id per browser tab/device. It travels on every API request
// (X-Connection-Id header) and with the realtime socket (?connectionId=)
// so the server can exclude the *originating* connection from its own
// realtime broadcasts — while the same user's other tabs/devices (which
// have a different id) still receive them.
export const connectionId = createConnectionId();

function createConnectionId(): string {
	try {
		if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
			return crypto.randomUUID();
		}
	} catch {
		// fall through to the fallback below
	}
	return 'conn-' + Math.random().toString(36).slice(2) + Date.now().toString(36);
}

// Attach the connection id to every SDK request by wrapping the HTTP
// request method — fetch itself is left untouched, so same-origin and
// cross-origin requests outside the SDK are unaffected (no CORS issues).
// ── Keep the session alive ───────────────────────────────────────────────
// The server's access token lives 15 minutes and the SDK assumes 7 days, so a
// long-lived Studio session needs the refresh token (see `session.ts`). Wrap the
// request method: refresh just before the token lapses, and retry once with a
// fresh token if a request is refused anyway.
{
	const http = client.http as HttpClient;
	const origRequest = http.request.bind(http);

	/** Requests whose own 401 is the answer (a bad password is not a stale session). */
	const AUTH_PATHS = /\/(auth-[a-z-]+|superusers\/login|superusers\/setup)$/;

	function isUnauthorized(error: unknown): boolean {
		return (
			typeof error === 'object' &&
			error !== null &&
			(error as { status?: unknown }).status === 401
		);
	}

	http.request = (async (method, path, body, options) => {
		const send = () =>
			origRequest(method, path, body, {
				...options,
				headers: { ...(options?.headers ?? {}), 'X-Connection-Id': connectionId }
			});

		await ensureFreshSession();
		try {
			return await send();
		} catch (err) {
			if (isUnauthorized(err) && !AUTH_PATHS.test(path) && (await refreshAfterUnauthorized())) {
				return await send();
			}
			throw err;
		}
	}) as typeof http.request;
}

// Advertise the connection id on the realtime stream so the server can
// match stream ↔ HTTP requests from the same tab. The SDK sends the
// bearer token and the connection id itself on (re)connect.
client.realtime.setUrl(sseUrlFromBaseUrl('/api'));
client.realtime.setConnectionId(connectionId);
