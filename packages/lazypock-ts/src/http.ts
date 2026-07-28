// ── HTTP Client ─────────────────────────────────────────
// Only relies on globalThis.fetch — works in browser, React Native, and Node 18+

import { ApiError, type Method, type RequestOptions } from "./types";
import type { AuthStore } from "./auth";

export class HttpClient {
	private baseUrl: string;
	private authStore: AuthStore;
	private defaultFetch: typeof globalThis.fetch;

	constructor(baseUrl: string, authStore: AuthStore) {
		this.baseUrl = baseUrl.replace(/\/+$/, "");
		this.authStore = authStore;
		this.defaultFetch = globalThis.fetch.bind(globalThis);
	}

	async request<T = unknown>(
		method: Method,
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		const url = this.baseUrl + path;
		const headers: Record<string, string> = {
			"Content-Type": "application/json",
			...options?.headers,
		};

		if (this.authStore.token) {
			headers["Authorization"] = "Bearer " + this.authStore.token;
		}

		const init: RequestInit = {
			method,
			headers,
			signal: options?.signal,
		};

		if (body != null && method !== "GET" && method !== "DELETE") {
			init.body = JSON.stringify(body);
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

	get<T = unknown>(path: string, options?: RequestOptions): Promise<T | null> {
		return this.request<T>("GET", path, undefined, options);
	}

	post<T = unknown>(
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("POST", path, body, options);
	}

	patch<T = unknown>(
		path: string,
		body?: unknown,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("PATCH", path, body, options);
	}

	delete<T = unknown>(
		path: string,
		options?: RequestOptions,
	): Promise<T | null> {
		return this.request<T>("DELETE", path, undefined, options);
	}
}
