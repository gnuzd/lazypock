// ── Record & Collection types ───────────────────────────

/**
 * Base shape every record returned from any collection satisfies.
 * Generated record interfaces extend this.
 */
export interface BaseRecordFields {
	id: string;
	collectionId: string;
	collectionName: string;
	created: string;
	updated: string;
}

/** Shape of a record returned from any collection. */
export interface ApiRecord extends BaseRecordFields {
	[key: string]: unknown;
}

/**
 * Structural marker for concrete record shapes (generated or hand-written).
 * Used to differentiate a typed collection service from the untyped default.
 */
export type RecordShape = Record<string, unknown>;

/** System fields every record carries — not user-provided on create. */
export type SystemFields =
	| "id"
	| "collectionId"
	| "collectionName"
	| "created"
	| "updated";

/**
 * Data accepted by `create()`: any subset of `T`'s fields, but
 * never the system fields. Unknown/extra keys are rejected at compile
 * time via excess-property checking (object literals).
 */
export type CreateData<T> = Partial<Omit<T, SystemFields>>;

/**
 * Data accepted by `update()`: any subset of `T`'s fields.
 * Unknown/extra keys are rejected at compile time.
 */
export type UpdateData<T> = Partial<Omit<T, SystemFields>>;

/** Paginated list response matching PocketBase format. */
export interface ListResult<T = ApiRecord> {
	items: T[];
	page: number;
	perPage: number;
	totalItems: number;
	totalPages: number;
}

/** HTTP method supported by the client. */
export type Method = "GET" | "POST" | "PATCH" | "DELETE";

export interface RequestOptions {
	/** Search/filter params */
	params?: Record<string, string>;
	/** Raw request headers to merge */
	headers?: Record<string, string>;
	/** Abort signal */
	signal?: AbortSignal;
	/** Custom fetch implementation (for RN or test mocking) */
	fetch?: typeof globalThis.fetch;
	/**
	 * Request identifier used by the auto-cancellation mechanism.
	 *
	 * Pending requests sharing the same key cancel each other — only the
	 * last one is executed (PocketBase `requestKey` semantics).
	 *
	 * - `string` — use this exact key instead of the default `METHOD + path`.
	 * - `null` — disable auto-cancellation for this request (never auto-cancelled).
	 *
	 * @default `${method} ${path}`
	 */
	requestKey?: string | null;
	/**
	 * Disable auto-cancellation for this request.
	 * Alias of `requestKey: null` (PocketBase `$autoCancel: false` compat).
	 */
	autoCancel?: boolean;
	/**
	 * Custom request key used for auto-cancellation.
	 * Alias of `requestKey` (PocketBase `$cancelKey` compat).
	 */
	cancelKey?: string;
}

export class ApiError extends Error {
	readonly data: unknown;
	readonly status: number;
	/**
	 * `true` when this error was caused by an aborted/cancelled request
	 * (auto-cancelled duplicate, or manually via `cancelRequest()` /
	 * `cancelAllRequests()` / an external AbortSignal).
	 */
	readonly isAbort: boolean;

	constructor(message: string, data: unknown, status: number, isAbort = false) {
		super(message);
		this.name = "ApiError";
		this.data = data;
		this.status = status;
		this.isAbort = isAbort;
	}
}
