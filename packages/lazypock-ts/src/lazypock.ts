// ── LazypockClient — Root Client ───────────────────────
// Extracted into its own module to avoid a circular import with
// the typed-client factory (src/client.ts).

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
	type RecordShape,
	type CreateData,
	type UpdateData,
	type SystemFields,
	type RequestOptions,
} from "./types";
import { RealtimeService, wsUrlFromBaseUrl } from "./realtime";
import { FilesService, getFileUrl, type FileRecord } from "./files";
import type { CollectionSchema, SchemaField } from "./schema";
import { generateTypes, collectionTypeName } from "./codegen";
import { fieldTypeScriptType, fieldTypeKind, schemaFieldType } from "./typegen";

export {
	AuthStore,
	ApiError,
	RealtimeService,
	wsUrlFromBaseUrl,
	FilesService,
	getFileUrl,
	CollectionService,
	generateTypes,
	collectionTypeName,
	fieldTypeScriptType,
	fieldTypeKind,
	schemaFieldType,
};
export type {
	StorageAdapter,
	AuthModel,
	ApiRecord,
	ListResult,
	RecordShape,
	CreateData,
	UpdateData,
	SystemFields,
	RequestOptions,
	FileRecord,
	CollectionSchema,
	SchemaField,
};

/** Options for constructing a {@link LazypockClient}. */
export interface LazypockClientOptions {
	/** API base URL (e.g. 'http://localhost:4000/api') */
	baseUrl: string;
	/** Custom storage adapter (default: localStorage fallback) */
	storage?: StorageAdapter;
	/** Explicit auth store instance (for sharing across modules) */
	authStore?: AuthStore;
	/** Real-time service for Phoenix Channel WebSocket subscriptions */
	realtime?: RealtimeService;
	/**
	 * Optional schema types for generating typed services at runtime.
	 * When provided, `collection()` returns a service whose create/update
	 * inputs are validated against the mapped field types.
	 *
	 * @experimental
	 */
	types?: {
		/** Collection schemas fetched from the API (e.g. via `GET /collections`). */
		schemas?: CollectionSchema[];
	};
}

/**
 * Lazypock API client.
 *
 * Provides methods for authentication, CRUD operations on dynamic collections,
 * file management, and real-time subscriptions.
 *
 * @example
 * ```ts
 * const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });
 * await client.login('admin@example.com', 'password');
 * const posts = await client.collection('posts').list();
 * ```
 */
export class LazypockClient {
	readonly http: HttpClient;
	readonly authStore: AuthStore;
	readonly realtime: RealtimeService;
	readonly files: FilesService;
	private collectionCache = new Map<string, CollectionService>();
	private schemaByName?: Map<string, CollectionSchema>;

	/**
	 * Create a new Lazypock client.
	 * @param options Configuration options.
	 */
	constructor(options: LazypockClientOptions) {
		const baseUrl = options.baseUrl.replace(/\/+$/, "");
		this.authStore =
			options.authStore ?? new AuthStore(options.storage ?? memoryStorage);
		this.http = new HttpClient(baseUrl, this.authStore);
		this.realtime = options.realtime ?? new RealtimeService();
		this.files = new FilesService(this.http);
		if (options.types?.schemas) {
			this.schemaByName = new Map(
				options.types.schemas.map((s) => [s.name, s]),
			);
		}
	}

	/**
	 * Get or create a service for the given collection.
	 * Services are cached after first access.
	 *
	 * For typed CRUD, either:
	 *   - cast at the call site: `client.collection("posts").typed<Post>()`
	 *   - or use the typed factory: `createClient<{ posts: Post }>()`
	 *
	 * @param name The collection name.
	 * @returns A {@link CollectionService} instance.
	 */
	collection(name: string): CollectionService<unknown> {
		let svc = this.collectionCache.get(name);
		if (!svc) {
			svc = new CollectionService(this.http, name, this.authStore);
			this.collectionCache.set(name, svc);
		}
		return svc;
	}

	/**
	 * Get a typed service whose record shape is derived from the schema
	 * passed via `options.types.schemas` (if available), or fall back to
	 * the untyped service otherwise.
	 *
	 * @experimental
	 */
	collectionFor<TRecord = ApiRecord>(name: string): CollectionService<TRecord> {
		return this.collection(name) as unknown as CollectionService<TRecord>;
	}

	/**
	 * Generate TypeScript types from the schemas provided to this client
	 * (via `options.types.schemas`). Returns a string ready to write to a
	 * `lazypock.types.ts` file.
	 */
	generateTypes(options?: { packageName?: string }): string {
		const schemas = this.schemaByName ? [...this.schemaByName.values()] : [];
		return generateTypes(schemas, options);
	}

	// ── Auth ──

	/** Check whether any superuser exists (for login vs setup screen routing). */
	async checkSuperuser(): Promise<{ has_superuser: boolean } | null> {
		return this.http.get<{ has_superuser: boolean }>("/superusers/check");
	}

	/**
	 * Create the initial superuser account.
	 * Only works when no superuser exists yet.
	 * Stores the returned token in the auth store.
	 * @param email Superuser email.
	 * @param password Superuser password (min 8 chars).
	 */
	async setup(
		email: string,
		password: string,
	): Promise<({ token: string } & Record<string, unknown>) | null> {
		const data = await this.http.post<
			{ token: string } & Record<string, unknown>
		>("/superusers/setup", { email, password });
		if (data) {
			this.authStore.setCollectionName(null);
			this.authStore.set(data.token, null);
		}
		return data;
	}

	/**
	 * Authenticate as a superuser or auth collection user.
	 *
	 * When `collection` is provided, authenticates against
	 * `/{collection}/auth-with-password`. Otherwise logs in as superuser.
	 * Stores the returned token in the auth store.
	 *
	 * @param email User email or identity.
	 * @param password User password.
	 * @param collection Optional auth collection name.
	 */
	async login(
		email: string,
		password: string,
		collection?: string,
	): Promise<({ token: string } & Record<string, unknown>) | null> {
		let data;
		if (collection) {
			data = await this.http.post<
				{ token: string; record: Record<string, unknown> } & Record<
					string,
					unknown
				>
			>("/" + encodeURIComponent(collection) + "/auth-with-password", {
				identity: email,
				password,
			});
			if (data && data.record) {
				this.authStore.setCollectionName(collection);
				this.authStore.set(data.token, data.record as unknown as AuthModel);
			}
		} else {
			data = await this.http.post<{ token: string } & Record<string, unknown>>(
				"/superusers/login",
				{ email, password },
			);
			if (data) {
				this.authStore.setCollectionName(null);
				this.authStore.set(data.token, null);
			}
		}
		return data;
	}

	/** Fetch the current superuser profile and refresh the auth model. */
	async me<T = ApiRecord>(options?: RequestOptions): Promise<T | null> {
		const data = await this.http.get<T>("/superusers/me", options);
		if (data) {
			// Update the auth model with fresh data
			this.authStore.set(this.authStore.token, data as unknown as AuthModel);
		}
		return data;
	}

	/**
	 * Authenticate against an auth collection with email/password.
	 * Stores the returned token and user record in the auth store.
	 *
	 * @param collection The auth collection name.
	 * @param identity Email or username.
	 * @param password Password.
	 * @param options Optional request options.
	 */
	async authWithPassword(
		collection: string,
		identity: string,
		password: string,
		options?: RequestOptions,
	): Promise<
		({ token: string; record: ApiRecord } & Record<string, unknown>) | null
	> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + encodeURIComponent(collection) + "/auth-with-password",
			{ identity, password },
			options,
		);
		if (data) {
			this.authStore.setCollectionName(collection);
			this.authStore.set(data.token, data.record as unknown as AuthModel);
		}
		return data;
	}

	/**
	 * Refresh an auth collection token.
	 * Uses the currently stored auth token.
	 *
	 * @param collection The auth collection name.
	 * @param options Optional request options.
	 */
	async authRefresh(
		collection: string,
		options?: RequestOptions,
	): Promise<
		({ token: string; record: ApiRecord } & Record<string, unknown>) | null
	> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + encodeURIComponent(collection) + "/auth-refresh",
			undefined,
			options,
		);
		if (data) {
			this.authStore.setCollectionName(collection);
			this.authStore.set(data.token, data.record as unknown as AuthModel);
		}
		return data;
	}

	/** Clear the current auth state and remove persisted tokens. */
	logout(): void {
		this.authStore.clear();
	}

	// ── Health ──

	/** Ping the API health endpoint. */
	health(options?: RequestOptions): Promise<Record<string, unknown> | null> {
		return this.http.get<Record<string, unknown>>("/health", options);
	}

	// ── Collection Management (admin) ──

	/**
	 * List all collections (admin).
	 * @param q URL query string (e.g. `page=1&perPage=200`).
	 * @param options Optional request options.
	 */
	listCollections(
		q?: string,
		options?: RequestOptions,
	): Promise<ListResult<ApiRecord> | null> {
		return this.http.get<ListResult<ApiRecord>>(
			"/collections" + (q ? "?" + q : ""),
			options,
		);
	}

	/**
	 * Get a single collection by ID or name.
	 * @param id Collection ID or name.
	 * @param options Optional request options.
	 */
	getCollection(
		id: string,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.get<ApiRecord>(
			"/collections/" + encodeURIComponent(id),
			options,
		);
	}

	/**
	 * Create a new collection (admin).
	 * @param data Collection definition (name, type, fields, options, rules, etc.).
	 * @param options Optional request options.
	 */
	createCollection(
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<ApiRecord | null> {
		return this.http.post<ApiRecord>("/collections", data, options);
	}

	/**
	 * Update an existing collection (admin).
	 * @param id Collection ID or name.
	 * @param data Updated collection fields.
	 * @param options Optional request options.
	 */
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

	/**
	 * Delete a collection (admin).
	 * @param id Collection ID or name.
	 * @param options Optional request options.
	 */
	deleteCollection(id: string, options?: RequestOptions): Promise<null> {
		return this.http.delete("/collections/" + encodeURIComponent(id), options);
	}

	// ── Records (dynamic collection) ──

	/**
	 * List records from a dynamic collection with optional filter/sort/pagination.
	 *
	 * @param coll Collection name.
	 * @param params Query parameters including:
	 *   - `filter` — PocketBase filter syntax (e.g. `title~'hello' && published=true`)
	 *   - `sort` — Comma-separated, `-` prefix for DESC (e.g. `-created,title`)
	 *   - `page` — Page number (default: 1)
	 *   - `perPage` — Items per page (default: 30, max: 200)
	 *   - `expand` — Comma-separated relation fields (e.g. `author,category`)
	 * @param options Optional request options.
	 */
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

	/**
	 * Get a single record by ID.
	 * @param coll Collection name.
	 * @param id Record ID.
	 * @param options Optional request options.
	 */
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

	/**
	 * Create a record in a dynamic collection.
	 * @param coll Collection name.
	 * @param data Record fields.
	 * @param options Optional request options.
	 */
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

	/**
	 * Update a record in a dynamic collection.
	 * @param coll Collection name.
	 * @param id Record ID.
	 * @param data Updated record fields.
	 * @param options Optional request options.
	 */
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

	/**
	 * Delete a record from a dynamic collection.
	 * @param coll Collection name.
	 * @param id Record ID.
	 * @param options Optional request options.
	 */
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
