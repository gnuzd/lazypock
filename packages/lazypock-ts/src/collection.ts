// ── Collection Service ──────────────────────────────────
// Typed CRUD for a single collection (like PocketBase's pb.collection(name))

import type { HttpClient } from "./http";
import type { ApiRecord, ListResult, RequestOptions } from "./types";

export class CollectionService {
	private http: HttpClient;
	private collectionName: string;

	constructor(http: HttpClient, collectionName: string) {
		this.http = http;
		this.collectionName = collectionName;
	}

	private encodeId(id: string): string {
		return encodeURIComponent(id);
	}

	/** List records with optional filter params */
	list<T = ApiRecord>(
		params?: Record<string, string>,
		options?: RequestOptions,
	): Promise<ListResult<T> | null> {
		const qs = params ? "?" + new URLSearchParams(params).toString() : "";
		return this.http.get<ListResult<T>>(
			"/" + this.encodeId(this.collectionName) + qs,
			options,
		);
	}

	/** Get a single record by id */
	getOne<T = ApiRecord>(
		id: string,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http.get<T>(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			options,
		);
	}

	/** Create a new record */
	create<T = ApiRecord>(
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http.post<T>(
			"/" + this.encodeId(this.collectionName),
			data,
			options,
		);
	}

	/** Update a record by id */
	update<T = ApiRecord>(
		id: string,
		data: Record<string, unknown>,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.http.patch<T>(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			data,
			options,
		);
	}

	/** Delete a record by id */
	delete(id: string, options?: RequestOptions): Promise<null> {
		return this.http.delete(
			"/" + this.encodeId(this.collectionName) + "/" + this.encodeId(id),
			options,
		);
	}
}
