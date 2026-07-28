// ── Auth Store ──────────────────────────────────────────
// Pluggable storage adapter: swap for AsyncStorage on RN, localStorage on web, etc.

export interface StorageAdapter {
	get(key: string): string | null | Promise<string | null>;
	set(key: string, value: string): void | Promise<void>;
	remove(key: string): void | Promise<void>;
}

export interface AuthModel {
	id: string;
	[key: string]: unknown;
}

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

	constructor(storage?: StorageAdapter) {
		this.storage = storage ?? {
			get: (_key: string) => null,
			set: () => {},
			remove: () => {},
		};
	}

	get token(): string {
		return this._token;
	}

	get model(): AuthModel | null {
		return this._model;
	}

	get isValid(): boolean {
		return !!this._token;
	}

	get isExpired(): boolean {
		return (
			this._tokenExpiresAt !== null &&
			Date.now() >= this._tokenExpiresAt - 30000
		);
	}

	get collectionName(): string | null {
		return this._collectionName;
	}

	setCollectionName(name: string | null): void {
		this._collectionName = name;
	}

	/** Load persisted auth from storage */
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
