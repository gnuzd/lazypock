// ── Phoenix Channel WebSocket Client ──────────────────
//
// Implements the Phoenix Channels protocol over WebSocket
// to subscribe to collection realtime updates.
//
// The backend CollectionSocket uses the path /socket and
// authenticates via a `token` query parameter (JWT).

interface RealtimeEvent {
	event: string;
	topic: string;
	payload: Record<string, unknown>;
}

interface SubEntry {
	topic: string;
	callback: (e: RealtimeEvent) => void;
}

export type RealtimeConnectOpts = {
	/** WebSocket URL (e.g. ws://localhost:4000/socket) */
	url: string;
	/** Auth token to pass as query param */
	token?: string;
};

/**
 * Derive a WebSocket URL from an HTTP base URL.
 * http://localhost:4000/api → ws://localhost:4000/socket
 */
export function wsUrlFromBaseUrl(baseUrl: string): string {
	try {
		const url = new URL(baseUrl);
		const protocol = url.protocol === "https:" ? "wss:" : "ws:";
		return `${protocol}//${url.host}/socket`;
	} catch {
		return `${baseUrl.replace(/^http/, "ws").replace(/\/api$/, "")}/socket`;
	}
}

export class RealtimeService {
	private ws: WebSocket | null = null;
	private refCounter = 0;
	private subscriptions = new Map<string, SubEntry[]>();
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private reconnectAttempt = 0;
	private maxReconnectDelay = 5000;

	// Callbacks for connection state
	onReconnect?: () => void;
	onDisconnect?: () => void;
	onError?: (err: Event) => void;

	private url: string = "";
	private token: string | undefined;

	connect(opts: RealtimeConnectOpts): void {
		this.url = opts.url;
		this.token = opts.token;
		this.reconnectAttempt = 0;
		this.doConnect();
	}

	disconnect(): void {
		this.clearReconnectTimer();
		this.ws?.close();
		this.ws = null;
	}

	/**
	 * Subscribe to a topic (e.g. "collection:posts" or "collection:posts:*").
	 * The backend Channel authorizes via listRule on join.
	 */
	subscribe(topic: string, callback: (e: RealtimeEvent) => void): void {
		const subs = this.subscriptions.get(topic) || [];
		subs.push({ topic, callback });
		this.subscriptions.set(topic, subs);

		if (this.ws?.readyState === WebSocket.OPEN) {
			this.joinTopic(topic);
		}
	}

	/**
	 * Unsubscribe a specific callback from a topic.
	 */
	unsubscribe(topic: string, callback?: (e: RealtimeEvent) => void): void {
		if (!callback) {
			this.subscriptions.delete(topic);
			return;
		}
		const subs = this.subscriptions
			.get(topic)
			?.filter((s) => s.callback !== callback);
		if (subs && subs.length > 0) {
			this.subscriptions.set(topic, subs);
		} else {
			this.subscriptions.delete(topic);
		}
	}

	private resubscribeAll(): void {
		for (const topic of this.subscriptions.keys()) {
			if (this.ws?.readyState === WebSocket.OPEN) {
				this.joinTopic(topic);
			}
		}
	}

	private doConnect(): void {
		if (typeof WebSocket === "undefined") {
			console.warn(
				"[lazypock] WebSocket not available — realtime subscriptions disabled",
			);
			return;
		}

		let url = this.url;
		if (this.token) {
			url +=
				(url.includes("?") ? "&" : "?") +
				"token=" +
				encodeURIComponent(this.token);
		}

		this.ws = new WebSocket(url);
		this.ws.onopen = () => {
			this.reconnectAttempt = 0;
			this.resubscribeAll();
			this.startHeartbeat();
		};

		this.ws.onmessage = (msg: MessageEvent) => {
			this.handleMessage(msg.data);
		};

		this.ws.onclose = () => {
			this.stopHeartbeat();
			this.onDisconnect?.();
			this.scheduleReconnect();
		};

		this.ws.onerror = (err: Event) => {
			this.onError?.(err);
		};
	}

	private handleMessage(data: string): void {
		let parsed: unknown[];
		try {
			parsed = JSON.parse(data) as unknown[];
		} catch {
			return;
		}
		if (!Array.isArray(parsed) || parsed.length < 4) return;

		const [, , topic, event, payload] = parsed as [
			string | null,
			string | null,
			string,
			string,
			Record<string, unknown>,
		];

		// Ignore phx_reply (join/heartbeat responses)
		if (event === "phx_reply") return;

		// Relay incoming events to subscribers
		if (event === "record_change") {
			const subs = this.subscriptions.get(topic);
			if (subs) {
				const e: RealtimeEvent = {
					event,
					topic,
					payload: payload as Record<string, unknown>,
				};
				for (const s of subs) {
					try {
						s.callback(e);
					} catch {
						// swallow callback errors
					}
				}
			}
		}
	}

	private joinTopic(topic: string): void {
		const ref = this.nextRef();
		const msg = JSON.stringify([null, ref, topic, "phx_join", {}]);
		this.ws?.send(msg);
	}

	private nextRef(): string {
		this.refCounter++;
		return this.refCounter.toString();
	}

	// ── Heartbeat ──

	private heartbeatInterval: ReturnType<typeof setInterval> | null = null;

	private startHeartbeat(): void {
		this.stopHeartbeat();
		this.heartbeatInterval = setInterval(() => {
			if (this.ws?.readyState === WebSocket.OPEN) {
				const ref = this.nextRef();
				const msg = JSON.stringify([null, ref, "phoenix", "heartbeat", {}]);
				this.ws.send(msg);
			}
		}, 30_000);
	}

	private stopHeartbeat(): void {
		if (this.heartbeatInterval) {
			clearInterval(this.heartbeatInterval);
			this.heartbeatInterval = null;
		}
	}

	// ── Reconnect ──

	private scheduleReconnect(): void {
		this.clearReconnectTimer();
		const delay = Math.min(
			1000 * 2 ** this.reconnectAttempt,
			this.maxReconnectDelay,
		);
		this.reconnectAttempt++;
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.onReconnect?.();
			this.doConnect();
		}, delay);
	}

	private clearReconnectTimer(): void {
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
	}
}
