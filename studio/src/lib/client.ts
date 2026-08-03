import { LazypockClient } from 'lazypock';
import { dev } from '$app/environment';
import { createClient } from './lazypock.types';

export const client = createClient({ baseUrl: '/api' });

/** Connect realtime WebSocket using the current auth token */
export function connectRealtime(): void {
	if (!client.authStore.isValid) return;
	// In development, connect directly to the Phoenix backend on port 4000
	// (the Vite/SvelteKit dev server can't reliably proxy WebSocket upgrades).
	// In production, the socket is served from the same origin.
	const host = window.location.hostname;
	const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
	const port = dev ? 4000 : window.location.port;
	const wsUrl = `${protocol}://${host}:${port}/socket/websocket`;
	client.realtime.connect({
		url: wsUrl,
		token: client.authStore.token || undefined
	});
}

/** Disconnect realtime WebSocket */
export function disconnectRealtime(): void {
	client.realtime.disconnect();
}
