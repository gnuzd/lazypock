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

export class AuthStore {
	private _token = "";
	private _model: AuthModel | null = null;
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

	/** Load persisted auth from storage */
	async init(): Promise<void> {
		const [token, model] = await Promise.all([
			this.storage.get("auth_token"),
			this.storage.get("auth_model"),
		]);
		if (token) this._token = token;
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
		void Promise.all([
			this.storage.set("auth_token", token),
			model
				? this.storage.set("auth_model", JSON.stringify(model))
				: this.storage.remove("auth_model"),
		]);
		this.notify();
	}

	clear(): void {
		this._token = "";
		this._model = null;
		void Promise.all([
			this.storage.remove("auth_token"),
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
