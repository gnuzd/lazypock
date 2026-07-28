// ── Lazypock SDK — Root Client ─────────────────────────
// Usage:
//   const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });
//   await client.authStore.init();
//   await client.login('admin@example.com', 'password');
//   const records = await client.collection('articles').list();

import { HttpClient } from "./http";
import {
	AuthStore,
	memoryStorage,
	type StorageAdapter,
	type AuthModel,
} from "./auth";
import { CollectionService } from "./collection";
import {
	ApiError,
	type ApiRecord,
	type ListResult,
	type RequestOptions,
} from "./types";
import { RealtimeService, wsUrlFromBaseUrl } from "./realtime";

export { AuthStore, ApiError, RealtimeService, wsUrlFromBaseUrl };
export type {
	StorageAdapter,
	AuthModel,
	ApiRecord,
	ListResult,
	RequestOptions,
};

export interface LazypockClientOptions {
	/** API base URL (e.g. 'http://localhost:4000/api') */
	baseUrl: string;
	/** Custom storage adapter (default: localStorage fallback) */
	storage?: StorageAdapter;
	/** Explicit auth store instance (for sharing across modules) */
	authStore?: AuthStore;
	/** Real-time service for Phoenix Channel WebSocket subscriptions */
	realtime?: RealtimeService;
}

export class LazypockClient {
	readonly http: HttpClient;
	readonly authStore: AuthStore;
	readonly realtime: RealtimeService;
	private collectionCache = new Map<string, CollectionService>();

	constructor(options: LazypockClientOptions) {
		const baseUrl = options.baseUrl.replace(/\/+$/, "");
		this.authStore =
			options.authStore ?? new AuthStore(options.storage ?? memoryStorage);
		this.http = new HttpClient(baseUrl, this.authStore);
		this.realtime = options.realtime ?? new RealtimeService();
	}

	/** Get or create a typed collection service */
	collection(name: string): CollectionService {
		let svc = this.collectionCache.get(name);
		if (!svc) {
			svc = new CollectionService(this.http, name);
			this.collectionCache.set(name, svc);
		}
		return svc as unknown as CollectionService;
	}

	// ── Auth ──

	async checkSuperuser(): Promise<{ has_superuser: boolean } | null> {
		return this.http.get<{ has_superuser: boolean }>("/superusers/check");
	}

	async setup(
		email: string,
		password: string,
	): Promise<({ token: string } & Record<string, unknown>) | null> {
		const data = await this.http.post<
			{ token: string } & Record<string, unknown>
		>("/superusers/setup", { email, password });
		if (data) {
			this.authStore.set(data.token, null);
		}
		return data;
	}

	async login(
		email: string,
		password: string,
	): Promise<({ token: string } & Record<string, unknown>) | null> {
		const data = await this.http.post<
			{ token: string } & Record<string, unknown>
		>("/superusers/login", { email, password });
		if (data) {
			this.authStore.set(data.token, null);
		}
		return data;
	}

	async me<T = ApiRecord>(options?: RequestOptions): Promise<T | null> {
		const data = await this.http.get<T>("/superusers/me", options);
		if (data) {
			// Update the auth model with fresh data
			this.authStore.set(this.authStore.token, data as unknown as AuthModel);
		}
		return data;
	}

	logout(): void {
		this.authStore.clear();
	}

	// ── Health ──

	health(options?: RequestOptions): Promise<Record<string, unknown> | null> {
		return this.http.get<Record<string, unknown>>("/health", options);
	}

	// ── Collection Management (admin) ──

	listCollections(
		q?: string,
		options?: RequestOptions,
	): Promise<ListResult<ApiRecord> | null> {
		return this.http.get<ListResult<ApiRecord>>(
			"/collections" + (q ? "?" + q : ""),
			options,
		);
	}

	getCollection(
		id: string,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.get<ApiRecord>(
			"/collections/" + encodeURIComponent(id),
			options,
		);
	}

	createCollection(
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.post<ApiRecord>("/collections", data, options);
	}

	updateCollection(
		id: string,
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.patch<ApiRecord>(
			"/collections/" + encodeURIComponent(id),
			data,
			options,
		);
	}

	deleteCollection(id: string, options?: RequestOptions): Promise<null> {
		return this.http.delete("/collections/" + encodeURIComponent(id), options);
	}

	// ── Records (dynamic collection) ──

	listRecords(
		coll: string,
		params?: Record<string, string>,
		options?: RequestOptions,
	): Promise<ListResult<ApiRecord> | null> {
		const qs = params ? "?" + new URLSearchParams(params).toString() : "";
		return this.http.get<ListResult<ApiRecord>>(
			"/" + encodeURIComponent(coll) + qs,
			options,
		);
	}

	getRecord(
		coll: string,
		id: string,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.get<ApiRecord>(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
			options,
		);
	}

	createRecord(
		coll: string,
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.post<ApiRecord>(
			"/" + encodeURIComponent(coll),
			data,
			options,
		);
	}

	updateRecord(
		coll: string,
		id: string,
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.patch<ApiRecord>(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
			data,
			options,
		);
	}

	deleteRecord(
		coll: string,
		id: string,
		options?: RequestOptions,
	): Promise<null> {
		return this.http.delete(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
			options,
		);
	}
}
