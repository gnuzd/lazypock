// ── Record & Collection types ───────────────────────────

export interface ApiRecord {
	id: string;
	collectionId: string;
	collectionName: string;
	created: string;
	updated: string;
	[key: string]: unknown;
}

export interface ListResult<T = ApiRecord> {
	items: T[];
	page: number;
	perPage: number;
	totalItems: number;
	totalPages: number;
}

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
}

export class ApiError extends Error {
	readonly data: unknown;
	readonly status: number;

	constructor(message: string, data: unknown, status: number) {
		super(message);
		this.name = "ApiError";
		this.data = data;
		this.status = status;
	}
}
