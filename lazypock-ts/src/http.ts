// ── HTTP Client ─────────────────────────────────────────
// Only relies on globalThis.fetch — works in browser, React Native, and Node 18+

import { ApiError, type Method, type RequestOptions } from "./types";
import type { AuthStore } from "./auth";

/**
 * Low-level HTTP client wrapping `fetch` with automatic auth token injection.
 * Only relies on `globalThis.fetch` — works in browser, React Native, and Node 18+.
 */
/** Detect whether an unknown thrown value is an abort/`AbortError`-style error. */
function isAbortError(err: unknown): boolean {
	return (
		err instanceof Error &&
		(err.name === "AbortError" || err.message === "Aborted")
	);
}

export class HttpClient {
	private baseUrl: string;
	private authStore: AuthStore;
	private defaultFetch: typeof globalThis.fetch;

	/**
	 * Abort controllers for in-flight requests, keyed by their cancellation key
	 * (default `METHOD path`). A new request with the same key aborts the
	 * previous one — PocketBase-style auto-cancellation of duplicated requests.
	 */
	private cancelControllers: Record<string, AbortController> = {};

	/** Global toggle for the auto-cancellation behaviour (default: on). */
	private enableAutoCancellation = true;

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
	 * Globally enable or disable auto-cancellation of duplicated pending requests.
	 * Fluent — returns `this` for chaining.
	 */
	autoCancellation(enable: boolean): this {
		this.enableAutoCancellation = !!enable;
		return this;
	}

	/**
	 * Abort a pending request identified by its cancellation key
	 * (default `METHOD path`, e.g. `"GET /api/posts"`). No-op if not pending.
	 */
	cancelRequest(requestKey: string): this {
		const controller = this.cancelControllers[requestKey];
		if (controller) {
			controller.abort();
			delete this.cancelControllers[requestKey];
		}
		return this;
	}

	/** Abort all pending requests. */
	cancelAllRequests(): this {
		for (const key in this.cancelControllers) {
			this.cancelControllers[key].abort();
		}
		this.cancelControllers = {};
		return this;
	}

	/**
	 * Make an HTTP request with automatic auth token injection and optional auto-refresh.
	 *
	 * Auto-cancellation: a request keyed by `options.requestKey` (default
	 * `METHOD path`) aborts any previous pending request with the same key,
	 * so only the last duplicate executes. Set `requestKey: null` or
	 * `autoCancel: false` to opt out per request.
	 *
	 * @param method HTTP method.
	 * @param path URL path (appended to baseUrl).
	 * @param body JSON-serializable body, or FormData for file uploads.
	 * @param options Optional request options.
	 * @returns Parsed JSON response, or null for 204 No Content.
	 * @throws {ApiError} On non-2xx responses or when the request is aborted
	 * (aborted requests throw an `ApiError` with `isAbort === true`).
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

		// Resolve the auto-cancellation key (PocketBase `requestKey` semantics):
		// - options.requestKey null  → disabled for this request
		// - options.requestKey string → use it verbatim
		// - options.autoCancel false → disabled (legacy compat)
		// - options.cancelKey string → use it verbatim (legacy compat)
		// - otherwise → default to `${method} ${path}`
		let requestKey: string | null =
			options?.requestKey === undefined
				? (options?.cancelKey ?? `${method} ${path}`)
				: options.requestKey;
		if (options?.autoCancel === false) requestKey = null;

		// Wire a fresh AbortController for this request, merging any caller signal.
		// When auto-cancellation is enabled, the previous pending request sharing
		// our key is aborted first (only the last duplicate executes).
		let controller: AbortController | null = null;
		const externalSignal = options?.signal;
		if (requestKey !== null) {
			if (this.enableAutoCancellation) {
				this.cancelRequest(requestKey);
			}
			controller = new AbortController();
			this.cancelControllers[requestKey] = controller;
			if (externalSignal?.aborted) {
				controller.abort();
			} else if (externalSignal) {
				externalSignal.addEventListener("abort", () => controller?.abort(), {
					once: true,
				});
			}
		}
		const signal = controller?.signal ?? externalSignal;

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
			signal,
		};

		if (body != null && method !== "GET" && method !== "DELETE") {
			if (body instanceof FormData) {
				init.body = body;
			} else {
				init.body = JSON.stringify(body);
			}
		}

		let res: Response | null = null;
		const fetcher = options?.fetch ?? this.defaultFetch;
		try {
			res = await fetcher(url, init);
		} catch (err) {
			// Aborted (auto-cancelled duplicate, manual cancel, or external signal)
			// → normalized ApiError with isAbort === true, like PocketBase.
			if (isAbortError(err)) {
				throw new ApiError(
					"The request was aborted (most likely auto-cancelled by a newer request with the same requestKey)",
					{},
					0,
					true,
				);
			}
			throw err;
		} finally {
			// The request has settled — no longer pending, so drop the controller
			// unless a newer request already replaced it (same key).
			if (
				requestKey !== null &&
				this.cancelControllers[requestKey] === controller
			) {
				delete this.cancelControllers[requestKey];
			}
		}

		if (res!.status === 204) return null;

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

		if (!res!.ok) {
			throw new ApiError(
				(typeof data.message === "string" ? data.message : res!.statusText) ||
					`Request failed with status ${res!.status}`,
				data,
				res!.status,
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
