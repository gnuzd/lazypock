// ── Collections Service ─────────────────────────────────
// Registry-level service for the `collections` admin channel,
// mirroring PocketBase's `pb.collections.subscribe()`.
//
// This is distinct from `CollectionService` (single collection record
// events) — it subscribes to the *registry* topic (`collections`), which
// fires when collections themselves are created / updated / deleted.

import type { RealtimeService } from "./realtime";

const REGISTRY_TOPIC = "collections";

/**
 * Registry-level realtime events for the collections admin channel.
 * The backend (AdminChannel) broadcasts the action as the *event name*,
 * with the collection JSON as the payload.
 */
export interface CollectionsMessage {
	action: "create" | "update" | "delete";
	/** The collection payload (id + metadata), or {} when unavailable. */
	collection: Record<string, unknown>;
	topic?: string;
}

/**
 * Registry service for collection-level events (create/update/delete of
 * the collections themselves). Get an instance via
 * {@link LazypockClient.collections}.
 */
export class CollectionsService {
	private realtime?: RealtimeService;

	/** @internal */
	constructor(realtime?: RealtimeService) {
		this.realtime = realtime;
	}

	/**
	 * Subscribe to collection registry changes.
	 *
	 * @param callback Received on every collection create/update/delete.
	 * @returns A function that unsubscribes this callback.
	 */
	subscribe(callback: (e: CollectionsMessage) => void): () => void {
		if (!this.realtime) {
			console.warn("[lazypock] No realtime service configured.");
			return () => {};
		}
		const handler = (raw: {
			event: string;
			topic: string;
			payload?: Record<string, unknown>;
		}) => {
			callback({
				action: normalizeAction(raw.event),
				collection: (raw.payload ?? {}) as Record<string, unknown>,
				topic: raw.topic,
			});
		};
		this.realtime.ensureConnected();
		this.realtime.subscribe(REGISTRY_TOPIC, handler as never);
		return () => this.realtime?.unsubscribe(REGISTRY_TOPIC, handler as never);
	}

	/**
	 * Unsubscribe all callbacks from the registry channel.
	 */
	unsubscribe(): void {
		this.realtime?.unsubscribe(REGISTRY_TOPIC);
	}
}

/** Map a registry event name (create/update/delete) to an action. */
function normalizeAction(event: string): CollectionsMessage["action"] {
	const e = event.toLowerCase();
	if (e === "create") return "create";
	if (e === "update") return "update";
	if (e === "delete") return "delete";
	return "update";
}
