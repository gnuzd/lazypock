// ── API Client ──────────────────────────────────────────
// Matches PocketBase's response shape: { items, page, perPage, totalItems, totalPages }

const BASE = "/api";

class ApiClient {
	constructor(base) {
		this.base = base;
	}

	get token() {
		return localStorage.getItem("token") || "";
	}

	set token(val) {
		if (val) {
			localStorage.setItem("token", val);
		} else {
			localStorage.removeItem("token");
		}
	}

	get headers() {
		const h = { "Content-Type": "application/json" };
		if (this.token) h["Authorization"] = "Bearer " + this.token;
		return h;
	}

	async request(method, path, body) {
		const opts = { method, headers: this.headers };
		if (body) opts.body = JSON.stringify(body);
		const res = await fetch(this.base + path, opts);
		if (res.status === 204) return null;
		const data = await res.json();
		if (!res.ok)
			throw new ApiError(data.message || res.statusText, data.data, res.status);
		return data;
	}

	get(path) {
		return this.request("GET", path);
	}
	post(path, b) {
		return this.request("POST", path, b);
	}
	patch(path, b) {
		return this.request("PATCH", path, b);
	}
	delete(path) {
		return this.request("DELETE", path);
	}

	// -- Health --
	async health() {
		return this.get("/health");
	}

	// -- Auth --
	async login(email, password) {
		const data = await this.post("/superusers/login", { email, password });
		this.token = data.token;
		return data;
	}
	async me() {
		return this.get("/superusers/me");
	}
	logout() {
		this.token = "";
	}

	// -- Collections --
	async listCollections(q) {
		return this.get("/collections" + (q ? "?" + q : ""));
	}
	async getCollection(id) {
		return this.get("/collections/" + encodeURIComponent(id));
	}
	async createCollection(b) {
		return this.post("/collections", b);
	}
	async updateCollection(id, b) {
		return this.patch("/collections/" + encodeURIComponent(id), b);
	}
	async deleteCollection(id) {
		return this.delete("/collections/" + encodeURIComponent(id));
	}

	// -- Records --
	async listRecords(coll, params) {
		const qs = params ? "?" + new URLSearchParams(params).toString() : "";
		return this.get("/" + encodeURIComponent(coll) + qs);
	}
	async getRecord(coll, id) {
		return this.get(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
		);
	}
	async createRecord(coll, b) {
		return this.post("/" + encodeURIComponent(coll), b);
	}
	async updateRecord(coll, id, b) {
		return this.patch(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
			b,
		);
	}
	async deleteRecord(coll, id) {
		return this.delete(
			"/" + encodeURIComponent(coll) + "/" + encodeURIComponent(id),
		);
	}
}

class ApiError extends Error {
	constructor(message, data, status) {
		super(message);
		this.data = data;
		this.status = status;
	}
}

export const api = new ApiClient(BASE);
