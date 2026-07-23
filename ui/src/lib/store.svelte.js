// ── Global Store (Svelte 5 runes) ──────────────────────

import { api } from "./api.js";

let ready = $state(false);
let superuser = $state(null);
let page = $state(null);
let collections = $state([]);
let activeCollectionId = $state("");
let settings = $state({});

export const store = {
	get ready() {
		return ready;
	},
	set ready(v) {
		ready = v;
	},

	get superuser() {
		return superuser;
	},
	set superuser(v) {
		superuser = v;
	},

	get page() {
		return page;
	},
	set page(v) {
		page = v;
	},

	get collections() {
		return collections;
	},
	set collections(v) {
		collections = v;
	},

	get activeCollectionId() {
		return activeCollectionId;
	},
	set activeCollectionId(v) {
		activeCollectionId = v;
	},

	get activeCollection() {
		return collections.find(
			(c) => c.id === activeCollectionId || c.name === activeCollectionId,
		);
	},

	get settings() {
		return settings;
	},
	set settings(v) {
		settings = v;
	},

	async init() {
		const token = localStorage.getItem("token");
		if (token) {
			try {
				const su = await api.me();
				store.superuser = su;
			} catch {
				api.logout();
			}
		}
		store.ready = true;
	},

	async loadCollections() {
		try {
			const result = await api.listCollections("page=1&perPage=200");
			store.collections = result.items || [];
			if (!store.activeCollectionId && store.collections.length > 0) {
				store.activeCollectionId = store.collections[0].id;
			}
		} catch (e) {
			console.error("loadCollections:", e);
		}
	},

	async loadSettings() {
		// stub — will implement settings endpoint later
	},
};
