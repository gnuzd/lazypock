// ── Collection Service ──────────────────────────────────
// Typed CRUD for a single collection (like PocketBase's pb.collection(name))

import type { HttpClient } from "./http";
import type { AuthStore, AuthModel } from "./auth";
import type {
	ApiRecord,
	ListResult,
	RequestOptions,
	CreateData,
	UpdateData,
} from "./types";
import type { RealtimeService } from "./realtime";

/**
 * A realtime record-change event delivered to subscription callbacks.
 * Mirrors PocketBase's RealtimeService result shape (`action` + `record`).
 */
export interface RealtimeMessage {
	action: "create" | "update" | "delete";
	record: Record<string, unknown>;
	topic?: string;
}

/** Subscription callback for a collection's realtime events. */
export type RealtimeCallback = (e: RealtimeMessage) => void;

/** Raw event passed by the low-level realtime service. */
interface RealtimeEventLike {
	event: string;
	topic: string;
	payload?: Record<string, unknown>;
}

/** Map a raw event/action to a normalised create/update/delete action. */
function normalizeAction(
	event: string,
	rawAction?: unknown,
): RealtimeMessage["action"] {
	if (typeof rawAction === "string") {
		const a = rawAction.toLowerCase();
		if (a === "create" || a === "update" || a === "delete") {
			return a;
		}
	}
	if (event === "record_change") return "update";
	if (event === "create" || event === "record_create") return "create";
	if (event === "delete" || event === "record_delete") return "delete";
	return "update";
}

/**
 * Typed CRUD service for a single dynamic collection.
 * Get an instance via {@link LazypockClient.collection}.
 *
 * @typeParam T — The record shape for this collection. Defaults to {@link ApiRecord}.
 */
export class CollectionService<T = ApiRecord> {
	private http: HttpClient;
	private collectionName: string;
	private authStore?: AuthStore;
	private realtime?: RealtimeService;

	/** @internal */
	constructor(
		http: HttpClient,
		collectionName: string,
		authStore?: AuthStore,
		realtime?: RealtimeService,
	) {
		this.http = http;
		this.collectionName = collectionName;
		this.authStore = authStore;
		this.realtime = realtime;
	}

	private encodeId(id: string): string {
		return encodeURIComponent(id);
	}

	/**
	 * List records with optional filter/sort/pagination.
	 * @param params Query parameters including `filter`, `sort`, `page`, `perPage`, `expand`.
	 * @param options Optional request options.
	 */
	list(
		params?: Record<string, string>,
		options?: RequestOptions,
	): Promise<ListResult<T> | null> {
		const qs = params ? "?" + new URLSearchParams(params).toString() : "";
		return this.http.get<ListResult<T>>(
			"/" + this.encodeId(this.collectionName) + qs,
			options,
		);
	}

	/**
	 * Get a single record by ID.
	 * @param id Record ID.
	 * @param options Optional request options.
	 */
	getOne(id: string, options?: RequestOptions): Promise<T | null> {
		return this.http.get<T>(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			options,
		);
	}

	/**
	 * Create a new record.
	 * @param data Record fields. When `T` is a concrete shape (e.g. a generated
	 * record type), excess/unknown fields are rejected at compile time.
	 * @param options Optional request options.
	 */
	create(
		data: T extends ApiRecord ? Record<string, unknown> : CreateData<T>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http.post<T>(
			"/" + this.encodeId(this.collectionName),
			data,
			options,
		);
	}

	/**
	 * Update a record by ID.
	 * @param id Record ID.
	 * @param data Updated record fields. When `T` is a concrete shape, `data`
	 * must be a partial of `T` — unknown fields are rejected.
	 * @param options Optional request options.
	 */
	update(
		id: string,
		data: T extends ApiRecord ? Record<string, unknown> : UpdateData<T>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http.patch<T>(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			data,
			options,
		);
	}

	/**
	 * Delete a record by ID.
	 * @param id Record ID.
	 * @param options Optional request options.
	 */
	delete(id: string, options?: RequestOptions): Promise<null> {
		return this.http.delete(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			options,
		);
	}
	// ── Expand / Relation Fields ──

	/**
	 * Get a list of expandable (relation) fields for this collection.
	 * Useful for constructing `expand` query parameters.
	 */
	async expandFields(
		options?: RequestOptions,
	): Promise<{ field: string; targetCollection: string }[] | null> {
		const data = await this.http.get<{
			fields?: {
				name: string;
				type: string;
				options?: Record<string, string>;
			}[];
		}>("/collections/" + this.encodeId(this.collectionName), options);
		if (!data?.fields) return null;
		return data.fields
			.filter((f) => f.type === "relation" && f.options?.collection)
			.map((f) => ({
				field: f.name,
				targetCollection: f.options!.collection,
			}));
	}

	/**
	 * Cast this service to a specific record shape.
	 * Use when you have a hand-written or generated interface for the
	 * collection and want compile-time checking of create/update/list.
	 *
	 * @example
	 * ```ts
	 * interface Post {
	 *   id: string;
	 *   title: string;
	 *   published: boolean;
	 * }
	 * const posts = client.collection("posts").typed<Post>();
	 * await posts.create({ title: "Hi", published: true }); // ✓
	 * await posts.create({ nope: 1 }); // ✗ compile error
	 * ```
	 */
	typed<TRecord = ApiRecord>(): CollectionService<TRecord> {
		return this as unknown as CollectionService<TRecord>;
	}

	// ── Realtime Subscriptions (PocketBase-style) ──

	/**
	 * Subscribe to realtime changes for this collection.
	 * The event's `action` is one of `"create" | "update" | "delete"`.
	 *
	 * Access is governed by the collection's `listRule` (PocketBase semantics):
	 * public collections allow anonymous subscriptions; other collections
	 * require a matching logged-in user or superuser.
	 *
	 * @param callback Received on every record change.
	 * @param recordId Optional — subscribe to a single record instead of `*`.
	 * @returns A function that unsubscribes this callback.
	 */
	subscribe(callback: RealtimeCallback, recordId?: string): () => void {
		if (!this.realtime) {
			console.warn("[lazypock] No realtime service configured.");
			return () => {};
		}
		const topic =
			"collection:" + this.collectionName + (recordId ? ":" + recordId : "");
		const handler = (raw: RealtimeEventLike) => {
			const record = (raw.payload?.["record"] ?? {}) as Record<string, unknown>;
			callback({
				action: normalizeAction(raw.event, raw.payload?.["action"]),
				record,
				topic: raw.topic,
			});
		};
		this.realtime.ensureConnected();
		this.realtime.subscribe(topic, handler as never);
		return () => this.realtime?.unsubscribe(topic, handler as never);
	}

	/**
	 * Unsubscribe all callbacks from this collection (or a specific record).
	 * @param recordId Optional record id; omitting it unsubs everything.
	 */
	unsubscribe(recordId?: string): void {
		const topic =
			"collection:" + this.collectionName + (recordId ? ":" + recordId : "");
		this.realtime?.unsubscribe(topic);
	}

	// ── Auth Collection Methods ──

	/**
	 * Authenticate with email/password against this auth collection.
	 * Stores the returned token and user model in the auth store.
	 */
	async authWithPassword(
		identity: string,
		password: string,
		options?: RequestOptions,
	): Promise<
		({ token: string; record: ApiRecord } & Record<string, unknown>) | null
	> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + this.encodeId(this.collectionName) + "/auth-with-password",
			{ identity, password },
			options,
		);
		if (data && this.authStore) {
			this.authStore.setCollectionName(this.collectionName);
			this.authStore.set(data.token, data.record as unknown as AuthModel);
		}
		return data;
	}

	/**
	 * Refresh the auth token for the currently authenticated user.
	 * Updates the stored token and user model.
	 */
	async authRefresh(
		options?: RequestOptions,
	): Promise<
		({ token: string; record: ApiRecord } & Record<string, unknown>) | null
	> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + this.encodeId(this.collectionName) + "/auth-refresh",
			undefined,
			options,
		);
		if (data && this.authStore) {
			this.authStore.setCollectionName(this.collectionName);
			this.authStore.set(data.token, data.record as unknown as AuthModel);
		}
		return data;
	}

	/**
	 * Get available auth methods for this collection.
	 */
	// ── end Realtime ──

	async authMethods(
		options?: RequestOptions,
	): Promise<Record<string, unknown> | null> {
		return this.http.get<Record<string, unknown>>(
			"/" + this.encodeId(this.collectionName) + "/auth-methods",
			options,
		);
	}
}
