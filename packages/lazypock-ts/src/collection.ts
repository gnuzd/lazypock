// ── Collection Service ──────────────────────────────────
// Typed CRUD for a single collection (like PocketBase's pb.collection(name))

import type { HttpClient } from "./http";
import type { AuthStore, AuthModel } from "./auth";
import type { ApiRecord, ListResult, RequestOptions } from "./types";

export class CollectionService {
	private http: HttpClient;
	private collectionName: string;
	private authStore?: AuthStore;

	constructor(http: HttpClient, collectionName: string, authStore?: AuthStore) {
		this.http = http;
		this.collectionName = collectionName;
		this.authStore = authStore;
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

	// ── Auth Collection Methods ──

	/**
	 * Authenticate with email/password against this auth collection.
	 * Stores the returned token and user model in the auth store.
	 */
	async authWithPassword(
		identity: string,
		password: string,
		options?: RequestOptions,
	): Promise<({ token: string; record: ApiRecord } & Record<string, unknown>) | null> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + this.encodeId(this.collectionName) + "/auth-with-password",
			{ identity, password },
			options,
		);
		if (data && this.authStore) {
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
	): Promise<({ token: string; record: ApiRecord } & Record<string, unknown>) | null> {
		const data = await this.http.post<
			{ token: string; record: ApiRecord } & Record<string, unknown>
		>(
			"/" + this.encodeId(this.collectionName) + "/auth-refresh",
			undefined,
			options,
		);
		if (data && this.authStore) {
			this.authStore.set(data.token, data.record as unknown as AuthModel);
		}
		return data;
	}

	/**
	 * Get available auth methods for this collection.
	 */
	async authMethods(
		options?: RequestOptions,
	): Promise<Record<string, unknown> | null> {
		return this.http.get<Record<string, unknown>>(
			"/" + this.encodeId(this.collectionName) + "/auth-methods",
			options,
		);
	}
}
