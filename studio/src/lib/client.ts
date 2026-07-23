import { LazypockClient, wsUrlFromBaseUrl } from 'lazypock';

export const client = new LazypockClient({
	baseUrl: '/api'
});

/** Connect realtime WebSocket using the current auth token */
export function connectRealtime(): void {
	if (!client.authStore.isValid) return;
	const wsUrl = wsUrlFromBaseUrl(window.location.origin + '/api');
	client.realtime.connect({
		url: wsUrl,
		token: client.authStore.token || undefined,
	});
}

/** Disconnect realtime WebSocket */
export function disconnectRealtime(): void {
	client.realtime.disconnect();
}
