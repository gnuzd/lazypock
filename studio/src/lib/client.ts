import { createClient } from './lazypock.types';
import { wsUrlFromBaseUrl, type HttpClient } from 'lazypock';

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
{
	const http = client.http as HttpClient;
	const origRequest = http.request.bind(http);
	http.request = ((method, path, body, options) => {
		const headers = { ...(options?.headers ?? {}), 'X-Connection-Id': connectionId };
		return origRequest(method, path, body, { ...options, headers });
	}) as typeof http.request;
}

// Advertise the connection id on the realtime socket so the server can
// match socket ↔ HTTP requests from the same tab. The SDK appends
// ?token=... itself on (re)connect.
client.realtime.setUrl(
	wsUrlFromBaseUrl('/api') + '?connectionId=' + encodeURIComponent(connectionId)
);
