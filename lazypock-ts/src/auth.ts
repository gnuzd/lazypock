// ── Auth Store ──────────────────────────────────────────
// Pluggable storage adapter: swap for AsyncStorage on RN, localStorage on web, etc.

/**
 * Interface for pluggable persistence backends.
 * Swap for `AsyncStorage` on React Native, `localStorage` on web, etc.
 */
export interface StorageAdapter {
	/** Retrieve a stored value by key. */
	get(key: string): string | null | Promise<string | null>;
	/** Persist a key-value pair. */
	set(key: string, value: string): void | Promise<void>;
	/** Remove a stored value by key. */
	remove(key: string): void | Promise<void>;
}

/** Shape of an authenticated user record (from auth collections). */
export interface AuthModel {
	id: string;
	[key: string]: unknown;
}

/** Callback signature for auth state changes. */
export type AuthListener = (model: AuthModel | null, token: string) => void;

// Server token TTL: 7 days (matches Phoenix.Token max_age)
const TOKEN_TTL_MS = 7 * 24 * 60 * 60 * 1000;

export class AuthStore {
	private _token = "";
	private _model: AuthModel | null = null;
	private _tokenExpiresAt: number | null = null;
	private _collectionName: string | null = null;
	private listeners = new Set<AuthListener>();
	private storage: StorageAdapter;

	/**
	 * Create an AuthStore with optional custom storage adapter.
	 * @param storage Persistence backend. Defaults to `memoryStorage` (localStorage fallback).
	 */
	constructor(storage?: StorageAdapter) {
		this.storage = storage ?? {
			get: (_key: string) => null,
			set: () => {},
			remove: () => {},
		};
	}

	/** The current JWT token string, or empty string if not authenticated. */
	get token(): string {
		return this._token;
	}

	/** The current authenticated user record, or null. */
	get model(): AuthModel | null {
		return this._model;
	}

	/** Whether a token exists (does not check expiry). */
	get isValid(): boolean {
		return !!this._token;
	}

	/**
	 * Whether the current token has expired (with a 30-second buffer).
	 * Returns false when no expiry has been recorded (e.g. superuser tokens).
	 */
	get isExpired(): boolean {
		return (
			this._tokenExpiresAt !== null &&
			Date.now() >= this._tokenExpiresAt - 30000
		);
	}

	/** The auth collection name used for automatic token refresh. */
	get collectionName(): string | null {
		return this._collectionName;
	}

	/**
	 * Set the auth collection name (used internally by auto-refresh).
	 * @param name The collection name, or null for superuser tokens.
	 */
	setCollectionName(name: string | null): void {
		this._collectionName = name;
	}

	/**
	 * Load persisted auth state from storage.
	 * Should be called once at application startup.
	 */
	async init(): Promise<void> {
		const [token, model, expiresAt] = await Promise.all([
			this.storage.get("auth_token"),
			this.storage.get("auth_model"),
			this.storage.get("auth_expires_at"),
		]);
		if (token) this._token = token;
		if (expiresAt) this._tokenExpiresAt = parseInt(expiresAt, 10) || null;
		if (model) {
			try {
				this._model = JSON.parse(model);
			} catch {
				// ignore corrupt data
			}
		}
	}

	/**
	 * Update the current auth token and model, persist to storage, and notify listeners.
	 * @param token The JWT token string.
	 * @param model The authenticated user record, or null for superusers.
	 */
	set(token: string, model: AuthModel | null): void {
		this._token = token;
		this._model = model;
		this._tokenExpiresAt = Date.now() + TOKEN_TTL_MS;
		void Promise.all([
			this.storage.set("auth_token", token),
			this.storage.set("auth_expires_at", String(this._tokenExpiresAt)),
			model
				? this.storage.set("auth_model", JSON.stringify(model))
				: this.storage.remove("auth_model"),
		]);
		this.notify();
	}

	/**
	 * Clear all auth state (token, model, expiry) and notify listeners.
	 */
	clear(): void {
		this._token = "";
		this._model = null;
		this._tokenExpiresAt = null;
		this._collectionName = null;
		void Promise.all([
			this.storage.remove("auth_token"),
			this.storage.remove("auth_expires_at"),
			this.storage.remove("auth_model"),
		]);
		this.notify();
	}

	/**
	 * Register a listener for auth state changes.
	 * @param fn Callback invoked with (model, token) on every change.
	 * @returns An unsubscribe function.
	 */
	onChange(fn: AuthListener): () => void {
		this.listeners.add(fn);
		return () => this.listeners.delete(fn);
	}

	private notify(): void {
		for (const fn of this.listeners) {
			fn(this._model, this._token);
		}
	}
}

// ── Memory-only storage (default, works everywhere) ─────
/** Default storage adapter using `localStorage` with graceful fallback. */
export const memoryStorage: StorageAdapter = {
	get(key: string) {
		try {
			return localStorage.getItem(key);
		} catch {
			return null;
		}
	},
	set(key: string, value: string) {
		try {
			localStorage.setItem(key, value);
		} catch {
			// ignore
		}
	},
	remove(key: string) {
		try {
			localStorage.removeItem(key);
		} catch {
			// ignore
		}
	},
};
