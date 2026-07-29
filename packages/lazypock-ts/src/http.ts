// ── HTTP Client ─────────────────────────────────────────
// Only relies on globalThis.fetch — works in browser, React Native, and Node 18+

import { ApiError, type Method, type RequestOptions } from "./types";
import type { AuthStore } from "./auth";

/**
 * Low-level HTTP client wrapping `fetch` with automatic auth token injection.
 * Only relies on `globalThis.fetch` — works in browser, React Native, and Node 18+.
 */
export class HttpClient {
	private baseUrl: string;
	private authStore: AuthStore;
	private defaultFetch: typeof globalThis.fetch;

	/**
	 * @param baseUrl The API base URL (e.g. `http://localhost:4000/api`). Trailing slash stripped.
	 * @param authStore The auth store providing the token for Authorization headers.
	 */
	constructor(baseUrl: string, authStore: AuthStore) {
		this.baseUrl = baseUrl.replace(/\/+$/, "");
		this.authStore = authStore;
		this.defaultFetch = globalThis.fetch.bind(globalThis);
	}

	private async refreshAuth(): Promise<{
		token: string;
		record: Record<string, unknown>;
	} | null> {
		const collection = this.authStore.collectionName;
		if (!collection) return null;
		try {
			const url =
				this.baseUrl + "/" + encodeURIComponent(collection) + "/auth-refresh";
			const headers: Record<string, string> = {
				"Content-Type": "application/json",
			};
			if (this.authStore.token) {
				headers["Authorization"] = "Bearer " + this.authStore.token;
			}
			const res = await this.defaultFetch(url, {
				method: "POST",
				headers,
			});
			if (!res.ok) {
				this.authStore.clear();
				return null;
			}
			const data = (await res.json()) as Record<string, unknown>;
			if (data && typeof data.token === "string") {
				this.authStore.set(
					data.token,
					(data.record as Record<string, unknown> as any) ?? null,
				);
				return data as { token: string; record: Record<string, unknown> };
			}
			return null;
		} catch {
			return null;
		}
	}

	/**
	 * Make an HTTP request with automatic auth token injection and optional auto-refresh.
	 *
	 * @param method HTTP method.
	 * @param path URL path (appended to baseUrl).
	 * @param body JSON-serializable body, or FormData for file uploads.
	 * @param options Optional request options.
	 * @returns Parsed JSON response, or null for 204 No Content.
	 * @throws {ApiError} On non-2xx responses.
	 */
	async request<T = unknown>(
		method: Method,
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		// Auto-refresh if token is expired
		if (this.authStore.isExpired && this.authStore.collectionName) {
			await this.refreshAuth();
		}

		let url = this.baseUrl + path;
		if (options?.params) {
			const qs = new URLSearchParams(options.params).toString();
			if (qs) {
				url += (path.includes("?") ? "&" : "?") + qs;
			}
		}
		const headers: Record<string, string> = {
			...options?.headers,
		};

		// Don't set Content-Type for FormData (browser sets multipart boundary)
		if (!(body instanceof FormData)) {
			headers["Content-Type"] = "application/json";
		}

		if (this.authStore.token) {
			headers["Authorization"] = "Bearer " + this.authStore.token;
		}

		const init: RequestInit = {
			method,
			headers,
			signal: options?.signal,
		};

		if (body != null && method !== "GET" && method !== "DELETE") {
			if (body instanceof FormData) {
				init.body = body;
			} else {
				init.body = JSON.stringify(body);
			}
		}

		const fetcher = options?.fetch ?? this.defaultFetch;
		const res = await fetcher(url, init);

		if (res.status === 204) return null;

		// Safely parse JSON — some errored responses may have empty or non-JSON bodies
		let bodyText = "";
		let data: Record<string, unknown> = {};
		try {
			bodyText = await res.text();
			if (bodyText) {
				data = JSON.parse(bodyText) as Record<string, unknown>;
			}
		} catch {
			// Not JSON — keep data as empty object
		}

		if (!res.ok) {
			throw new ApiError(
				(typeof data.message === "string" ? data.message : res.statusText) ||
					`Request failed with status ${res.status}`,
				data,
				res.status,
			);
		}

		return data as T;
	}

	/**
	 * HTTP GET.
	 * @param path URL path.
	 * @param options Optional request options.
	 */
	get<T = unknown>(path: string, options?: RequestOptions): Promise<T | null> {
		return this.request<T>("GET", path, undefined, options);
	}

	/**
	 * HTTP POST.
	 * @param path URL path.
	 * @param body Optional request body.
	 * @param options Optional request options.
	 */
	post<T = unknown>(
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("POST", path, body, options);
	}

	/**
	 * HTTP PATCH.
	 * @param path URL path.
	 * @param body Optional request body.
	 * @param options Optional request options.
	 */
	patch<T = unknown>(
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("PATCH", path, body, options);
	}

	/**
	 * HTTP DELETE.
	 * @param path URL path.
	 * @param options Optional request options.
	 */
	delete<T = unknown>(
		path: string,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("DELETE", path, undefined, options);
	}
}
