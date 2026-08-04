// ── Collections Service ─────────────────────────────────
// Registry-level service for collections (CRUD + realtime registry events),
// mirroring PocketBase's `pb.collections` service.
//
// Get an instance via {@link LazypockClient.collections}.
//
// This is distinct from `CollectionService` (single collection record
// events) — it operates on the *collections* themselves: listing / creating /
// updating / deleting collections via `/api/collections`, and subscribing to
// the `collections` registry channel (AdminChannel), which fires when
// collections themselves are created / updated / deleted.

import type { HttpClient } from "./http";
import type { RealtimeService } from "./realtime";
import type { ListResult, RequestOptions, ApiRecord } from "./types";

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
 * Registry service for collections (list/create/update/delete + realtime
 * registry events). Mirrors PocketBase's `pb.collections`.
 */
export class CollectionsService {
	private http?: HttpClient;
	private realtime?: RealtimeService;

	/** @internal */
	constructor(http?: HttpClient, realtime?: RealtimeService) {
		this.http = http;
		this.realtime = realtime;
	}

	/**
	 * Fetch a paginated list of collections (admin).
	 *
	 * @param params Optional query params (`page`, `perPage`, `filter`, `sort`).
	 * @param options Optional request options.
	 */
	async getList<T = ApiRecord>(
		params?: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ListResult<T> | null> {
		const qs = params
			? "?" +
				new URLSearchParams(
					Object.fromEntries(
						Object.entries(params).map(([k, v]) => [k, String(v)]),
					),
				).toString()
			: "";
		return (
			this.http?.get<ListResult<T>>("/collections" + qs, options) ?? null
		);
	}

	/**
	 * Fetch all collections at once (auto-paginates). Mirrors PocketBase's
	 * `pb.collections.getFullList()` — defaults to listing everything.
	 *
	 * @param options Query params (`sort`, `batch`, etc.) or request options.
	 */
	async getFullList<T = ApiRecord>(
		options?: Record<string, unknown> & RequestOptions,
	): Promise<Array<T>> {
		if (!this.http) return [];
		const { batch = 1000, ...rest } = options ?? {};
		const items: T[] = [];
		let page = 1;
		// Auto-paginate until empty (bounded by perPage and totalPages).
		for (;;) {
			const res = await this.getList<T>(
				{ ...rest, page, perPage: batch } as Record<string, unknown>,
			);
			if (!res || !res.items || res.items.length === 0) break;
			items.push(...(res.items as T[]));
			if (page >= (res.totalPages ?? page)) break;
			page += 1;
		}
		return items;
	}

	/**
	 * Get a single collection by ID or name (admin).
	 *
	 * @param id Collection ID or name.
	 * @param options Optional request options.
	 */
	async getOne<T = ApiRecord>(
		id: string,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http?.get<T>(
			"/collections/" + encodeURIComponent(id),
			options,
		) ?? null;
	}

	/**
	 * Create a new collection (admin).
	 *
	 * @param data Collection definition (name, type, fields, options, rules, etc.).
	 * @param options Optional request options.
	 */
	async create<T = ApiRecord>(
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http?.post<T>("/collections", data, options) ?? null;
	}

	/**
	 * Update an existing collection (admin).
	 *
	 * @param id Collection ID or name.
	 * @param data Updated collection fields.
	 * @param options Optional request options.
	 */
	async update<T = ApiRecord>(
		id: string,
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http?.patch<T>(
			"/collections/" + encodeURIComponent(id),
			data,
			options,
		) ?? null;
	}

	/**
	 * Delete a collection (admin).
	 *
	 * @param id Collection ID or name.
	 * @param options Optional request options.
	 */
	async delete(id: string, options?: RequestOptions): Promise<boolean> {
		const res = this.http?.delete("/collections/" + encodeURIComponent(id), options);
		return res == null ? false : true;
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